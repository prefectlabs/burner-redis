use bytes::Bytes;
use mlua::prelude::*;
use ordered_float::OrderedFloat;
use sha1::{Digest, Sha1};
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::broadcast;

use crate::store::{PubSubMessage, ValueData, ValueEntry};

/// Represents a Redis command return value for Lua-Redis type conversion.
#[derive(Debug, Clone)]
pub enum RedisValue {
    BulkString(Bytes),
    Integer(i64),
    Array(Vec<RedisValue>),
    Nil,
    Error(String),
    Status(String),
}

impl IntoLua for RedisValue {
    fn into_lua(self, lua: &Lua) -> LuaResult<LuaValue> {
        match self {
            RedisValue::BulkString(b) => {
                let s = lua.create_string(b.as_ref())?;
                Ok(LuaValue::String(s))
            }
            RedisValue::Integer(n) => Ok(LuaValue::Integer(n)),
            RedisValue::Array(items) => {
                let table = lua.create_table()?;
                for (i, item) in items.into_iter().enumerate() {
                    table.set(i + 1, item.into_lua(lua)?)?;
                }
                Ok(LuaValue::Table(table))
            }
            RedisValue::Nil => Ok(LuaValue::Boolean(false)),
            RedisValue::Error(msg) => {
                // Errors should be handled by the caller (redis.call raises, redis.pcall wraps)
                // If we get here, wrap as a table with err field
                let table = lua.create_table()?;
                table.set("err", msg)?;
                Ok(LuaValue::Table(table))
            }
            RedisValue::Status(s) => {
                let table = lua.create_table()?;
                table.set("ok", s)?;
                Ok(LuaValue::Table(table))
            }
        }
    }
}

/// Convert a Lua value back to a RedisValue.
fn lua_to_redis_value(val: LuaValue) -> RedisValue {
    match val {
        LuaValue::String(s) => RedisValue::BulkString(Bytes::from(s.as_bytes().to_vec())),
        LuaValue::Integer(n) => RedisValue::Integer(n),
        LuaValue::Number(n) => RedisValue::Integer(n as i64),
        LuaValue::Boolean(true) => RedisValue::Integer(1),
        LuaValue::Boolean(false) => RedisValue::Nil,
        LuaValue::Nil => RedisValue::Nil,
        LuaValue::Table(table) => {
            // Check for err key first
            if let Ok(err) = table.get::<LuaValue>("err".to_string()) {
                if let LuaValue::String(s) = err {
                    return RedisValue::Error(String::from_utf8_lossy(&s.as_bytes()).to_string());
                }
            }
            // Check for ok key
            if let Ok(ok) = table.get::<LuaValue>("ok".to_string()) {
                if let LuaValue::String(s) = ok {
                    return RedisValue::Status(String::from_utf8_lossy(&s.as_bytes()).to_string());
                }
            }
            // Treat as array: iterate sequential integer keys 1..n
            let mut arr = Vec::new();
            let mut i = 1;
            loop {
                match table.get::<LuaValue>(i) {
                    Ok(LuaValue::Nil) => break,
                    Ok(v) => arr.push(lua_to_redis_value(v)),
                    Err(_) => break,
                }
                i += 1;
            }
            RedisValue::Array(arr)
        }
        _ => RedisValue::Nil,
    }
}

pub struct LuaEngine;

impl LuaEngine {
    /// Compute SHA1 hex digest of a script.
    pub fn sha1_hex(script: &str) -> String {
        let mut hasher = Sha1::new();
        hasher.update(script.as_bytes());
        let result = hasher.finalize();
        format!("{:x}", result)
    }

    /// Execute a Lua script with access to KEYS, ARGV, and redis.call()/redis.pcall().
    ///
    /// `data` is the ALREADY WRITE-LOCKED store data HashMap -- the caller (Store::eval)
    /// acquires the write lock and passes the mutable reference. This ensures atomicity.
    /// `pubsub_tx` is an optional broadcast sender for PUBLISH command support in Lua scripts.
    /// Execute a Lua script. Returns (RedisValue, bool) where bool indicates XADD occurred.
    pub fn execute(
        script: &str,
        keys: Vec<Bytes>,
        args: Vec<Bytes>,
        data: &mut HashMap<Bytes, ValueEntry>,
        pubsub_tx: Option<&broadcast::Sender<PubSubMessage>>,
    ) -> Result<(RedisValue, bool), String> {
        // Create a fresh Lua VM per execution (isolation, no state leakage)
        let lua = Lua::new();

        // Use RefCell to allow mutable access from within Lua callbacks
        let data_cell = RefCell::new(data);
        // Clone the broadcast sender for use inside Lua closures
        let pubsub_tx_clone = pubsub_tx.cloned();
        // Track if any XADD occurred during script execution
        let had_xadd = std::cell::Cell::new(false);

        let scope_result: LuaResult<RedisValue> = lua.scope(|scope| {
            // Set up KEYS global
            let keys_table = lua.create_table()?;
            for (i, key) in keys.iter().enumerate() {
                keys_table.set(i + 1, lua.create_string(key.as_ref())?)?;
            }
            lua.globals().set("KEYS", keys_table)?;

            // Set up ARGV global
            let argv_table = lua.create_table()?;
            for (i, arg) in args.iter().enumerate() {
                argv_table.set(i + 1, lua.create_string(arg.as_ref())?)?;
            }
            lua.globals().set("ARGV", argv_table)?;

            // Create redis table with call and pcall
            let redis_table = lua.create_table()?;

            // redis.call() - raises Lua error on command failure
            let call_fn = scope
                .create_function_mut(|lua_ctx, args: LuaMultiValue| {
                    let args_vec: Vec<LuaValue> = args.into_vec();
                    if args_vec.is_empty() {
                        return Err(LuaError::RuntimeError(
                            "ERR wrong number of arguments for 'redis.call'".to_string(),
                        ));
                    }

                    // First arg is command name
                    let cmd_name = match &args_vec[0] {
                        LuaValue::String(s) => String::from_utf8_lossy(&s.as_bytes()).to_uppercase(),
                        _ => {
                            return Err(LuaError::RuntimeError(
                                "ERR first argument must be a string".to_string(),
                            ))
                        }
                    };

                    // Remaining args are command arguments
                    let cmd_args: Vec<Bytes> = args_vec[1..]
                        .iter()
                        .map(|v| match v {
                            LuaValue::String(s) => Bytes::from(s.as_bytes().to_vec()),
                            LuaValue::Integer(n) => Bytes::from(n.to_string().into_bytes()),
                            LuaValue::Number(n) => Bytes::from(n.to_string().into_bytes()),
                            _ => Bytes::new(),
                        })
                        .collect();

                    let mut data_ref = data_cell.borrow_mut();
                    let pubsub_tx_ref = pubsub_tx_clone.as_ref();
                    let result = dispatch_command(&cmd_name, &cmd_args, *data_ref, pubsub_tx_ref);

                    match result {
                        Ok((RedisValue::Error(msg), _)) => Err(LuaError::RuntimeError(msg)),
                        Ok((val, xadd_flag)) => {
                            if xadd_flag { had_xadd.set(true); }
                            val.into_lua(lua_ctx)
                        },
                        Err(msg) => Err(LuaError::RuntimeError(msg)),
                    }
                })?;

            redis_table.set("call", call_fn)?;

            // redis.pcall() - returns error table instead of raising
            let pcall_fn = scope
                .create_function_mut(|lua_ctx, args: LuaMultiValue| {
                    let args_vec: Vec<LuaValue> = args.into_vec();
                    if args_vec.is_empty() {
                        let table = lua_ctx.create_table()?;
                        table.set("err", "ERR wrong number of arguments for 'redis.pcall'")?;
                        return Ok(LuaValue::Table(table));
                    }

                    // First arg is command name
                    let cmd_name = match &args_vec[0] {
                        LuaValue::String(s) => String::from_utf8_lossy(&s.as_bytes()).to_uppercase(),
                        _ => {
                            let table = lua_ctx.create_table()?;
                            table.set("err", "ERR first argument must be a string")?;
                            return Ok(LuaValue::Table(table));
                        }
                    };

                    // Remaining args are command arguments
                    let cmd_args: Vec<Bytes> = args_vec[1..]
                        .iter()
                        .map(|v| match v {
                            LuaValue::String(s) => Bytes::from(s.as_bytes().to_vec()),
                            LuaValue::Integer(n) => Bytes::from(n.to_string().into_bytes()),
                            LuaValue::Number(n) => Bytes::from(n.to_string().into_bytes()),
                            _ => Bytes::new(),
                        })
                        .collect();

                    let mut data_ref = data_cell.borrow_mut();
                    let pubsub_tx_ref = pubsub_tx_clone.as_ref();
                    let result = dispatch_command(&cmd_name, &cmd_args, *data_ref, pubsub_tx_ref);

                    match result {
                        Ok((RedisValue::Error(msg), _)) => {
                            let table = lua_ctx.create_table()?;
                            table.set("err", msg)?;
                            Ok(LuaValue::Table(table))
                        }
                        Ok((val, xadd_flag)) => {
                            if xadd_flag { had_xadd.set(true); }
                            val.into_lua(lua_ctx)
                        },
                        Err(msg) => {
                            let table = lua_ctx.create_table()?;
                            table.set("err", msg)?;
                            Ok(LuaValue::Table(table))
                        }
                    }
                })?;

            redis_table.set("pcall", pcall_fn)?;

            lua.globals().set("redis", redis_table)?;

            // Lua 5.4 compatibility: Redis uses Lua 5.1 where `unpack` is a global.
            // In Lua 5.4, it was moved to `table.unpack`. Provide the global alias.
            lua.load("unpack = unpack or table.unpack").exec()?;

            // Execute the script and capture return value
            let result: LuaValue = lua.load(script).eval()?;

            Ok(lua_to_redis_value(result))
        });

        scope_result.map(|v| (v, had_xadd.get())).map_err(|e| e.to_string())
    }
}

/// Dispatch a Redis command to the appropriate operation on the raw data HashMap.
/// This operates directly on the write-locked data for atomicity during Lua execution.
/// `pubsub_tx` is an optional broadcast sender for PUBLISH support in Lua scripts.
/// Returns (RedisValue, bool) where the bool indicates if an XADD occurred (for stream notification).
fn dispatch_command(
    cmd: &str,
    args: &[Bytes],
    data: &mut HashMap<Bytes, ValueEntry>,
    pubsub_tx: Option<&broadcast::Sender<PubSubMessage>>,
) -> Result<(RedisValue, bool), String> {
    let is_xadd = cmd == "XADD";
    let result = dispatch_command_inner(cmd, args, data, pubsub_tx)?;
    // Signal that an XADD occurred only if the command succeeded (not an error)
    let had_xadd = is_xadd && !matches!(result, RedisValue::Error(_));
    Ok((result, had_xadd))
}

/// Inner dispatch that returns just the RedisValue.
fn dispatch_command_inner(
    cmd: &str,
    args: &[Bytes],
    data: &mut HashMap<Bytes, ValueEntry>,
    pubsub_tx: Option<&broadcast::Sender<PubSubMessage>>,
) -> Result<RedisValue, String> {
    match cmd {
        // ── String commands ──────────────────────────────────────────
        "GET" => {
            if args.len() != 1 {
                return Ok(RedisValue::Error(
                    "ERR wrong number of arguments for 'get' command".to_string(),
                ));
            }
            let key = &args[0];
            // Passive expiration
            if let Some(entry) = data.get(key) {
                if entry.is_expired() {
                    data.remove(key);
                    return Ok(RedisValue::Nil);
                }
            }
            match data.get(key) {
                None => Ok(RedisValue::Nil),
                Some(entry) => match &entry.data {
                    ValueData::String(v) => Ok(RedisValue::BulkString(v.clone())),
                    _ => Ok(RedisValue::Error(
                        "WRONGTYPE Operation against a key holding the wrong kind of value"
                            .to_string(),
                    )),
                },
            }
        }

        "SET" => {
            if args.len() < 2 {
                return Ok(RedisValue::Error(
                    "ERR wrong number of arguments for 'set' command".to_string(),
                ));
            }
            let key = args[0].clone();
            let value = args[1].clone();
            let mut nx = false;
            let mut xx = false;
            let mut ttl: Option<Duration> = None;

            // Parse optional flags from args[2..]
            let mut i = 2;
            while i < args.len() {
                let flag = String::from_utf8_lossy(&args[i]).to_uppercase();
                match flag.as_str() {
                    "NX" => {
                        nx = true;
                        i += 1;
                    }
                    "XX" => {
                        xx = true;
                        i += 1;
                    }
                    "EX" => {
                        i += 1;
                        if i >= args.len() {
                            return Ok(RedisValue::Error(
                                "ERR syntax error".to_string(),
                            ));
                        }
                        let secs: u64 = String::from_utf8_lossy(&args[i])
                            .parse()
                            .map_err(|_| "ERR value is not an integer or out of range".to_string())?;
                        ttl = Some(Duration::from_secs(secs));
                        i += 1;
                    }
                    "PX" => {
                        i += 1;
                        if i >= args.len() {
                            return Ok(RedisValue::Error(
                                "ERR syntax error".to_string(),
                            ));
                        }
                        let ms: u64 = String::from_utf8_lossy(&args[i])
                            .parse()
                            .map_err(|_| "ERR value is not an integer or out of range".to_string())?;
                        ttl = Some(Duration::from_millis(ms));
                        i += 1;
                    }
                    _ => {
                        i += 1;
                    }
                }
            }

            // Check NX/XX conditions
            let key_exists = data
                .get(&key)
                .map(|e| !e.is_expired())
                .unwrap_or(false);

            if nx && key_exists {
                return Ok(RedisValue::Nil);
            }
            if xx && !key_exists {
                return Ok(RedisValue::Nil);
            }

            // Remove expired key if present
            if let Some(entry) = data.get(&key) {
                if entry.is_expired() {
                    data.remove(&key);
                }
            }

            let entry = ValueEntry::new(value, ttl);
            data.insert(key, entry);
            Ok(RedisValue::Status("OK".to_string()))
        }

        "DEL" => {
            if args.is_empty() {
                return Ok(RedisValue::Error(
                    "ERR wrong number of arguments for 'del' command".to_string(),
                ));
            }
            let mut count = 0i64;
            for key in args {
                if let Some(entry) = data.get(key) {
                    if !entry.is_expired() {
                        count += 1;
                    }
                }
                data.remove(key);
            }
            Ok(RedisValue::Integer(count))
        }

        "EXISTS" => {
            if args.is_empty() {
                return Ok(RedisValue::Error(
                    "ERR wrong number of arguments for 'exists' command".to_string(),
                ));
            }
            let mut count = 0i64;
            for key in args {
                // Passive expiration
                if let Some(entry) = data.get(key) {
                    if entry.is_expired() {
                        data.remove(key);
                        continue;
                    }
                    count += 1;
                }
            }
            Ok(RedisValue::Integer(count))
        }

        // ── Hash commands ────────────────────────────────────────────
        "HSET" => {
            if args.len() < 3 || (args.len() - 1) % 2 != 0 {
                return Ok(RedisValue::Error(
                    "ERR wrong number of arguments for 'hset' command".to_string(),
                ));
            }
            let key = args[0].clone();

            // Passive expiration
            if let Some(entry) = data.get(&key) {
                if entry.is_expired() {
                    data.remove(&key);
                }
            }

            let entry = data.entry(key).or_insert_with(ValueEntry::new_hash);
            match entry.data {
                ValueData::Hash(ref mut map) => {
                    let mut new_count = 0i64;
                    let mut i = 1;
                    while i < args.len() {
                        let field = args[i].clone();
                        let value = args[i + 1].clone();
                        if !map.contains_key(&field) {
                            new_count += 1;
                        }
                        map.insert(field, value);
                        i += 2;
                    }
                    Ok(RedisValue::Integer(new_count))
                }
                _ => Ok(RedisValue::Error(
                    "WRONGTYPE Operation against a key holding the wrong kind of value".to_string(),
                )),
            }
        }

        "HGET" => {
            if args.len() != 2 {
                return Ok(RedisValue::Error(
                    "ERR wrong number of arguments for 'hget' command".to_string(),
                ));
            }
            let key = &args[0];
            let field = &args[1];

            // Passive expiration
            if let Some(entry) = data.get(key) {
                if entry.is_expired() {
                    data.remove(key);
                    return Ok(RedisValue::Nil);
                }
            }

            match data.get(key) {
                None => Ok(RedisValue::Nil),
                Some(entry) => match &entry.data {
                    ValueData::Hash(map) => match map.get(field) {
                        Some(v) => Ok(RedisValue::BulkString(v.clone())),
                        None => Ok(RedisValue::Nil),
                    },
                    _ => Ok(RedisValue::Error(
                        "WRONGTYPE Operation against a key holding the wrong kind of value"
                            .to_string(),
                    )),
                },
            }
        }

        "HDEL" => {
            if args.len() < 2 {
                return Ok(RedisValue::Error(
                    "ERR wrong number of arguments for 'hdel' command".to_string(),
                ));
            }
            let key = &args[0];

            // Passive expiration
            if let Some(entry) = data.get(key) {
                if entry.is_expired() {
                    data.remove(key);
                    return Ok(RedisValue::Integer(0));
                }
            }

            match data.get_mut(key) {
                None => Ok(RedisValue::Integer(0)),
                Some(entry) => match entry.data {
                    ValueData::Hash(ref mut map) => {
                        let mut count = 0i64;
                        for field in &args[1..] {
                            if map.remove(field).is_some() {
                                count += 1;
                            }
                        }
                        Ok(RedisValue::Integer(count))
                    }
                    _ => Ok(RedisValue::Error(
                        "WRONGTYPE Operation against a key holding the wrong kind of value"
                            .to_string(),
                    )),
                },
            }
        }

        "HVALS" => {
            if args.len() != 1 {
                return Ok(RedisValue::Error(
                    "ERR wrong number of arguments for 'hvals' command".to_string(),
                ));
            }
            let key = &args[0];

            // Passive expiration
            if let Some(entry) = data.get(key) {
                if entry.is_expired() {
                    data.remove(key);
                    return Ok(RedisValue::Array(Vec::new()));
                }
            }

            match data.get(key) {
                None => Ok(RedisValue::Array(Vec::new())),
                Some(entry) => match &entry.data {
                    ValueData::Hash(map) => {
                        let vals: Vec<RedisValue> = map
                            .values()
                            .map(|v| RedisValue::BulkString(v.clone()))
                            .collect();
                        Ok(RedisValue::Array(vals))
                    }
                    _ => Ok(RedisValue::Error(
                        "WRONGTYPE Operation against a key holding the wrong kind of value"
                            .to_string(),
                    )),
                },
            }
        }

        "HGETALL" => {
            if args.len() != 1 {
                return Ok(RedisValue::Error(
                    "ERR wrong number of arguments for 'hgetall' command".to_string(),
                ));
            }
            let key = &args[0];

            // Passive expiration
            if let Some(entry) = data.get(key) {
                if entry.is_expired() {
                    data.remove(key);
                    return Ok(RedisValue::Array(Vec::new()));
                }
            }

            match data.get(key) {
                None => Ok(RedisValue::Array(Vec::new())),
                Some(entry) => match &entry.data {
                    ValueData::Hash(map) => {
                        // Return alternating field/value list (Redis wire format for Lua)
                        let mut result = Vec::new();
                        for (field, value) in map {
                            result.push(RedisValue::BulkString(field.clone()));
                            result.push(RedisValue::BulkString(value.clone()));
                        }
                        Ok(RedisValue::Array(result))
                    }
                    _ => Ok(RedisValue::Error(
                        "WRONGTYPE Operation against a key holding the wrong kind of value"
                            .to_string(),
                    )),
                },
            }
        }

        "HEXISTS" => {
            if args.len() != 2 {
                return Ok(RedisValue::Error(
                    "ERR wrong number of arguments for 'hexists' command".to_string(),
                ));
            }
            let key = &args[0];
            let field = &args[1];

            // Passive expiration
            if let Some(entry) = data.get(key) {
                if entry.is_expired() {
                    data.remove(key);
                    return Ok(RedisValue::Integer(0));
                }
            }

            match data.get(key) {
                None => Ok(RedisValue::Integer(0)),
                Some(entry) => match &entry.data {
                    ValueData::Hash(map) => {
                        if map.contains_key(field) {
                            Ok(RedisValue::Integer(1))
                        } else {
                            Ok(RedisValue::Integer(0))
                        }
                    }
                    _ => Ok(RedisValue::Error(
                        "WRONGTYPE Operation against a key holding the wrong kind of value"
                            .to_string(),
                    )),
                },
            }
        }

        "HINCRBY" => {
            if args.len() != 3 {
                return Ok(RedisValue::Error(
                    "ERR wrong number of arguments for 'hincrby' command".to_string(),
                ));
            }
            let key = args[0].clone();
            let field = &args[1];
            let increment: i64 = String::from_utf8_lossy(&args[2])
                .parse()
                .map_err(|_| "ERR value is not an integer or out of range".to_string())?;

            // Passive expiration
            if let Some(entry) = data.get(&key) {
                if entry.is_expired() {
                    data.remove(&key);
                }
            }

            let entry = data.entry(key).or_insert_with(ValueEntry::new_hash);
            match entry.data {
                ValueData::Hash(ref mut map) => {
                    let current = map
                        .get(field)
                        .and_then(|v| String::from_utf8_lossy(v).parse::<i64>().ok())
                        .unwrap_or(0);
                    let new_val = current + increment;
                    map.insert(field.clone(), Bytes::from(new_val.to_string()));
                    Ok(RedisValue::Integer(new_val))
                }
                _ => Ok(RedisValue::Error(
                    "WRONGTYPE Operation against a key holding the wrong kind of value".to_string(),
                )),
            }
        }

        // ── Set commands ─────────────────────────────────────────────
        "SADD" => {
            if args.len() < 2 {
                return Ok(RedisValue::Error(
                    "ERR wrong number of arguments for 'sadd' command".to_string(),
                ));
            }
            let key = args[0].clone();

            // Passive expiration
            if let Some(entry) = data.get(&key) {
                if entry.is_expired() {
                    data.remove(&key);
                }
            }

            let entry = data.entry(key).or_insert_with(ValueEntry::new_set);
            match entry.data {
                ValueData::Set(ref mut set) => {
                    let mut new_count = 0i64;
                    for member in &args[1..] {
                        if set.insert(member.clone()) {
                            new_count += 1;
                        }
                    }
                    Ok(RedisValue::Integer(new_count))
                }
                _ => Ok(RedisValue::Error(
                    "WRONGTYPE Operation against a key holding the wrong kind of value".to_string(),
                )),
            }
        }

        "SMEMBERS" => {
            if args.len() != 1 {
                return Ok(RedisValue::Error(
                    "ERR wrong number of arguments for 'smembers' command".to_string(),
                ));
            }
            let key = &args[0];

            // Passive expiration
            if let Some(entry) = data.get(key) {
                if entry.is_expired() {
                    data.remove(key);
                    return Ok(RedisValue::Array(Vec::new()));
                }
            }

            match data.get(key) {
                None => Ok(RedisValue::Array(Vec::new())),
                Some(entry) => match &entry.data {
                    ValueData::Set(set) => {
                        let members: Vec<RedisValue> = set
                            .iter()
                            .map(|m| RedisValue::BulkString(m.clone()))
                            .collect();
                        Ok(RedisValue::Array(members))
                    }
                    _ => Ok(RedisValue::Error(
                        "WRONGTYPE Operation against a key holding the wrong kind of value"
                            .to_string(),
                    )),
                },
            }
        }

        "SISMEMBER" => {
            if args.len() != 2 {
                return Ok(RedisValue::Error(
                    "ERR wrong number of arguments for 'sismember' command".to_string(),
                ));
            }
            let key = &args[0];
            let member = &args[1];

            // Passive expiration
            if let Some(entry) = data.get(key) {
                if entry.is_expired() {
                    data.remove(key);
                    return Ok(RedisValue::Integer(0));
                }
            }

            match data.get(key) {
                None => Ok(RedisValue::Integer(0)),
                Some(entry) => match &entry.data {
                    ValueData::Set(set) => {
                        if set.contains(member) {
                            Ok(RedisValue::Integer(1))
                        } else {
                            Ok(RedisValue::Integer(0))
                        }
                    }
                    _ => Ok(RedisValue::Error(
                        "WRONGTYPE Operation against a key holding the wrong kind of value"
                            .to_string(),
                    )),
                },
            }
        }

        "SREM" => {
            if args.len() < 2 {
                return Ok(RedisValue::Error(
                    "ERR wrong number of arguments for 'srem' command".to_string(),
                ));
            }
            let key = &args[0];

            // Passive expiration
            if let Some(entry) = data.get(key) {
                if entry.is_expired() {
                    data.remove(key);
                    return Ok(RedisValue::Integer(0));
                }
            }

            match data.get_mut(key) {
                None => Ok(RedisValue::Integer(0)),
                Some(entry) => match entry.data {
                    ValueData::Set(ref mut set) => {
                        let mut count = 0i64;
                        for member in &args[1..] {
                            if set.remove(member) {
                                count += 1;
                            }
                        }
                        Ok(RedisValue::Integer(count))
                    }
                    _ => Ok(RedisValue::Error(
                        "WRONGTYPE Operation against a key holding the wrong kind of value"
                            .to_string(),
                    )),
                },
            }
        }

        // ── Sorted set commands ──────────────────────────────────────
        "ZADD" => {
            if args.len() < 3 || (args.len() - 1) % 2 != 0 {
                return Ok(RedisValue::Error(
                    "ERR wrong number of arguments for 'zadd' command".to_string(),
                ));
            }
            let key = args[0].clone();

            // Passive expiration
            if let Some(entry) = data.get(&key) {
                if entry.is_expired() {
                    data.remove(&key);
                }
            }

            // Parse score-member pairs from args[1..]
            let entry = data
                .entry(key)
                .or_insert_with(ValueEntry::new_sorted_set);
            match entry.data {
                ValueData::SortedSet(ref mut zset) => {
                    let mut added = 0i64;
                    let mut i = 1;
                    while i + 1 < args.len() {
                        let score_str = String::from_utf8_lossy(&args[i]);
                        let score: f64 = score_str.parse().map_err(|_| {
                            "ERR value is not a valid float".to_string()
                        })?;
                        let member = args[i + 1].clone();
                        if zset.insert(member, score) {
                            added += 1;
                        }
                        i += 2;
                    }
                    Ok(RedisValue::Integer(added))
                }
                _ => Ok(RedisValue::Error(
                    "WRONGTYPE Operation against a key holding the wrong kind of value".to_string(),
                )),
            }
        }

        "ZREM" => {
            if args.len() < 2 {
                return Ok(RedisValue::Error(
                    "ERR wrong number of arguments for 'zrem' command".to_string(),
                ));
            }
            let key = &args[0];

            // Passive expiration
            if let Some(entry) = data.get(key) {
                if entry.is_expired() {
                    data.remove(key);
                    return Ok(RedisValue::Integer(0));
                }
            }

            match data.get_mut(key) {
                None => Ok(RedisValue::Integer(0)),
                Some(entry) => match entry.data {
                    ValueData::SortedSet(ref mut zset) => {
                        let mut count = 0i64;
                        for member in &args[1..] {
                            if zset.remove(member) {
                                count += 1;
                            }
                        }
                        Ok(RedisValue::Integer(count))
                    }
                    _ => Ok(RedisValue::Error(
                        "WRONGTYPE Operation against a key holding the wrong kind of value"
                            .to_string(),
                    )),
                },
            }
        }

        "ZRANGE" => {
            if args.len() < 3 {
                return Ok(RedisValue::Error(
                    "ERR wrong number of arguments for 'zrange' command".to_string(),
                ));
            }
            let key = &args[0];
            let start: i64 = String::from_utf8_lossy(&args[1])
                .parse()
                .map_err(|_| "ERR value is not an integer or out of range".to_string())?;
            let stop: i64 = String::from_utf8_lossy(&args[2])
                .parse()
                .map_err(|_| "ERR value is not an integer or out of range".to_string())?;

            // Passive expiration
            if let Some(entry) = data.get(key) {
                if entry.is_expired() {
                    data.remove(key);
                    return Ok(RedisValue::Array(Vec::new()));
                }
            }

            match data.get(key) {
                None => Ok(RedisValue::Array(Vec::new())),
                Some(entry) => match &entry.data {
                    ValueData::SortedSet(zset) => {
                        let len = zset.len() as i64;
                        if len == 0 {
                            return Ok(RedisValue::Array(Vec::new()));
                        }

                        let mut real_start = if start < 0 { len + start } else { start };
                        let mut real_stop = if stop < 0 { len + stop } else { stop };

                        if real_start < 0 {
                            real_start = 0;
                        }
                        if real_stop >= len {
                            real_stop = len - 1;
                        }
                        if real_start > real_stop || real_start >= len {
                            return Ok(RedisValue::Array(Vec::new()));
                        }

                        let result: Vec<RedisValue> = zset
                            .by_score
                            .iter()
                            .skip(real_start as usize)
                            .take((real_stop - real_start + 1) as usize)
                            .map(|((_, member), _)| RedisValue::BulkString(member.clone()))
                            .collect();

                        Ok(RedisValue::Array(result))
                    }
                    _ => Ok(RedisValue::Error(
                        "WRONGTYPE Operation against a key holding the wrong kind of value"
                            .to_string(),
                    )),
                },
            }
        }

        "ZRANGEBYSCORE" => {
            if args.len() < 3 {
                return Ok(RedisValue::Error(
                    "ERR wrong number of arguments for 'zrangebyscore' command".to_string(),
                ));
            }
            let key = &args[0];
            let min = parse_score_arg(&args[1]);
            let max = parse_score_arg(&args[2]);

            // Passive expiration
            if let Some(entry) = data.get(key) {
                if entry.is_expired() {
                    data.remove(key);
                    return Ok(RedisValue::Array(Vec::new()));
                }
            }

            match data.get(key) {
                None => Ok(RedisValue::Array(Vec::new())),
                Some(entry) => match &entry.data {
                    ValueData::SortedSet(zset) => {
                        let lower =
                            std::ops::Bound::Included((OrderedFloat(min), Bytes::new()));
                        let result: Vec<RedisValue> = zset
                            .by_score
                            .range((lower, std::ops::Bound::Unbounded))
                            .take_while(|((score, _), _)| score.0 <= max)
                            .map(|((_, member), _)| RedisValue::BulkString(member.clone()))
                            .collect();
                        Ok(RedisValue::Array(result))
                    }
                    _ => Ok(RedisValue::Error(
                        "WRONGTYPE Operation against a key holding the wrong kind of value"
                            .to_string(),
                    )),
                },
            }
        }

        "ZREMRANGEBYSCORE" => {
            if args.len() < 3 {
                return Ok(RedisValue::Error(
                    "ERR wrong number of arguments for 'zremrangebyscore' command".to_string(),
                ));
            }
            let key = &args[0];
            let min = parse_score_arg(&args[1]);
            let max = parse_score_arg(&args[2]);

            // Passive expiration
            if let Some(entry) = data.get(key) {
                if entry.is_expired() {
                    data.remove(key);
                    return Ok(RedisValue::Integer(0));
                }
            }

            match data.get_mut(key) {
                None => Ok(RedisValue::Integer(0)),
                Some(entry) => match entry.data {
                    ValueData::SortedSet(ref mut zset) => {
                        let lower =
                            std::ops::Bound::Included((OrderedFloat(min), Bytes::new()));
                        let to_remove: Vec<Bytes> = zset
                            .by_score
                            .range((lower, std::ops::Bound::Unbounded))
                            .take_while(|((score, _), _)| score.0 <= max)
                            .map(|((_, member), _)| member.clone())
                            .collect();

                        let count = to_remove.len() as i64;
                        for member in &to_remove {
                            zset.remove(member);
                        }
                        Ok(RedisValue::Integer(count))
                    }
                    _ => Ok(RedisValue::Error(
                        "WRONGTYPE Operation against a key holding the wrong kind of value"
                            .to_string(),
                    )),
                },
            }
        }

        "ZCARD" => {
            if args.len() != 1 {
                return Ok(RedisValue::Error(
                    "ERR wrong number of arguments for 'zcard' command".to_string(),
                ));
            }
            let key = &args[0];

            // Passive expiration
            if let Some(entry) = data.get(key) {
                if entry.is_expired() {
                    data.remove(key);
                    return Ok(RedisValue::Integer(0));
                }
            }

            match data.get(key) {
                None => Ok(RedisValue::Integer(0)),
                Some(entry) => match &entry.data {
                    ValueData::SortedSet(zset) => Ok(RedisValue::Integer(zset.len() as i64)),
                    _ => Ok(RedisValue::Error(
                        "WRONGTYPE Operation against a key holding the wrong kind of value"
                            .to_string(),
                    )),
                },
            }
        }

        "ZSCORE" => {
            if args.len() != 2 {
                return Ok(RedisValue::Error(
                    "ERR wrong number of arguments for 'zscore' command".to_string(),
                ));
            }
            let key = &args[0];
            let member = &args[1];

            // Passive expiration
            if let Some(entry) = data.get(key) {
                if entry.is_expired() {
                    data.remove(key);
                    return Ok(RedisValue::Nil);
                }
            }

            match data.get(key) {
                None => Ok(RedisValue::Nil),
                Some(entry) => match &entry.data {
                    ValueData::SortedSet(zset) => match zset.by_member.get(member) {
                        Some(&score) => Ok(RedisValue::BulkString(Bytes::from(score.to_string()))),
                        None => Ok(RedisValue::Nil),
                    },
                    _ => Ok(RedisValue::Error(
                        "WRONGTYPE Operation against a key holding the wrong kind of value"
                            .to_string(),
                    )),
                },
            }
        }

        // ── Key commands ────────────────────────────────────────────
        "EXPIRE" => {
            if args.len() != 2 {
                return Ok(RedisValue::Error(
                    "ERR wrong number of arguments for 'expire' command".to_string(),
                ));
            }
            let key = &args[0];
            let seconds: u64 = String::from_utf8_lossy(&args[1])
                .parse()
                .map_err(|_| "ERR value is not an integer or out of range".to_string())?;

            // Passive expiration
            if let Some(entry) = data.get(key) {
                if entry.is_expired() {
                    data.remove(key);
                    return Ok(RedisValue::Integer(0));
                }
            }

            match data.get_mut(key) {
                None => Ok(RedisValue::Integer(0)),
                Some(entry) => {
                    entry.expires_at =
                        Some(std::time::Instant::now() + Duration::from_secs(seconds));
                    Ok(RedisValue::Integer(1))
                }
            }
        }

        "PEXPIRE" => {
            if args.len() != 2 {
                return Ok(RedisValue::Error(
                    "ERR wrong number of arguments for 'pexpire' command".to_string(),
                ));
            }
            let key = &args[0];
            let ms: u64 = String::from_utf8_lossy(&args[1])
                .parse()
                .map_err(|_| "ERR value is not an integer or out of range".to_string())?;

            // Passive expiration
            if let Some(entry) = data.get(key) {
                if entry.is_expired() {
                    data.remove(key);
                    return Ok(RedisValue::Integer(0));
                }
            }

            match data.get_mut(key) {
                None => Ok(RedisValue::Integer(0)),
                Some(entry) => {
                    entry.expires_at =
                        Some(std::time::Instant::now() + Duration::from_millis(ms));
                    Ok(RedisValue::Integer(1))
                }
            }
        }

        // ── Stream commands ──────────────────────────────────────────
        "XADD" => {
            if args.len() < 4 || (args.len() - 2) % 2 != 0 {
                return Ok(RedisValue::Error(
                    "ERR wrong number of arguments for 'xadd' command".to_string(),
                ));
            }
            let key = args[0].clone();
            let id_str = String::from_utf8_lossy(&args[1]);

            // Passive expiration
            if let Some(entry) = data.get(&key) {
                if entry.is_expired() {
                    data.remove(&key);
                }
            }

            let entry = data
                .entry(key)
                .or_insert_with(ValueEntry::new_stream);

            match entry.data {
                ValueData::Stream(ref mut stream) => {
                    let new_id = if id_str == "*" {
                        // Auto-generate ID
                        let ms = std::time::SystemTime::UNIX_EPOCH
                            .elapsed()
                            .unwrap()
                            .as_millis() as u64;
                        if ms > stream.last_id.0 {
                            (ms, 0)
                        } else {
                            (stream.last_id.0, stream.last_id.1 + 1)
                        }
                    } else {
                        // Parse explicit ID
                        let parts: Vec<&str> = id_str.splitn(2, '-').collect();
                        if parts.len() != 2 {
                            return Ok(RedisValue::Error(
                                "ERR Invalid stream ID specified as stream command argument"
                                    .to_string(),
                            ));
                        }
                        let ms: u64 = parts[0].parse().map_err(|_| {
                            "ERR Invalid stream ID specified as stream command argument".to_string()
                        })?;
                        let seq: u64 = parts[1].parse().map_err(|_| {
                            "ERR Invalid stream ID specified as stream command argument".to_string()
                        })?;
                        let explicit_id = (ms, seq);
                        if explicit_id <= stream.last_id {
                            return Ok(RedisValue::Error(
                                "ERR The ID specified in XADD is equal or smaller than the target stream top item"
                                    .to_string(),
                            ));
                        }
                        explicit_id
                    };

                    // Parse fields
                    let mut fields = HashMap::new();
                    let mut i = 2;
                    while i + 1 < args.len() {
                        fields.insert(args[i].clone(), args[i + 1].clone());
                        i += 2;
                    }

                    stream.entries.insert(new_id, fields);
                    stream.last_id = new_id;

                    let id_string = format!("{}-{}", new_id.0, new_id.1);
                    Ok(RedisValue::BulkString(Bytes::from(id_string)))
                }
                _ => Ok(RedisValue::Error(
                    "WRONGTYPE Operation against a key holding the wrong kind of value".to_string(),
                )),
            }
        }

        "XREAD" => {
            // XREAD [COUNT n] STREAMS key1 [key2 ...] id1 [id2 ...]
            if args.is_empty() {
                return Ok(RedisValue::Error(
                    "ERR wrong number of arguments for 'xread' command".to_string(),
                ));
            }

            let mut count: Option<usize> = None;
            let mut i = 0;

            // Parse optional COUNT
            while i < args.len() {
                let token = String::from_utf8_lossy(&args[i]).to_uppercase();
                if token == "COUNT" {
                    i += 1;
                    if i >= args.len() {
                        return Ok(RedisValue::Error("ERR syntax error".to_string()));
                    }
                    count = Some(
                        String::from_utf8_lossy(&args[i])
                            .parse::<usize>()
                            .map_err(|_| "ERR value is not an integer or out of range".to_string())?,
                    );
                    i += 1;
                } else if token == "STREAMS" {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }

            // Remaining args after STREAMS: first half are keys, second half are IDs
            let remaining = &args[i..];
            if remaining.is_empty() || remaining.len() % 2 != 0 {
                return Ok(RedisValue::Error(
                    "ERR Unbalanced XREAD list of streams: for each stream key an ID must be specified"
                        .to_string(),
                ));
            }

            let half = remaining.len() / 2;
            let keys_slice = &remaining[..half];
            let ids_slice = &remaining[half..];

            let mut result = Vec::new();
            for (key, id_bytes) in keys_slice.iter().zip(ids_slice.iter()) {
                let id_str = String::from_utf8_lossy(id_bytes);
                let start_id = if id_str == "0" || id_str == "0-0" {
                    (0u64, 0u64)
                } else {
                    let parts: Vec<&str> = id_str.splitn(2, '-').collect();
                    if parts.len() == 2 {
                        let ms: u64 = parts[0].parse().unwrap_or(0);
                        let seq: u64 = parts[1].parse().unwrap_or(0);
                        (ms, seq)
                    } else {
                        let ms: u64 = parts[0].parse().unwrap_or(0);
                        (ms, 0)
                    }
                };

                // Passive expiration
                if let Some(entry) = data.get(key) {
                    if entry.is_expired() {
                        data.remove(key);
                        continue;
                    }
                }

                match data.get(key) {
                    None => continue,
                    Some(entry) => match &entry.data {
                        ValueData::Stream(stream) => {
                            let entries: Vec<RedisValue> = stream
                                .entries
                                .range((
                                    std::ops::Bound::Excluded(start_id),
                                    std::ops::Bound::Unbounded,
                                ))
                                .take(count.unwrap_or(usize::MAX))
                                .map(|(id, fields)| {
                                    let id_str = format!("{}-{}", id.0, id.1);
                                    let mut field_arr = Vec::new();
                                    for (k, v) in fields {
                                        field_arr.push(RedisValue::BulkString(k.clone()));
                                        field_arr.push(RedisValue::BulkString(v.clone()));
                                    }
                                    RedisValue::Array(vec![
                                        RedisValue::BulkString(Bytes::from(id_str)),
                                        RedisValue::Array(field_arr),
                                    ])
                                })
                                .collect();

                            if !entries.is_empty() {
                                result.push(RedisValue::Array(vec![
                                    RedisValue::BulkString(key.clone()),
                                    RedisValue::Array(entries),
                                ]));
                            }
                        }
                        _ => {
                            return Ok(RedisValue::Error(
                                "WRONGTYPE Operation against a key holding the wrong kind of value"
                                    .to_string(),
                            ))
                        }
                    },
                }
            }

            if result.is_empty() {
                Ok(RedisValue::Nil)
            } else {
                Ok(RedisValue::Array(result))
            }
        }

        "XDEL" => {
            if args.len() < 2 {
                return Ok(RedisValue::Error(
                    "ERR wrong number of arguments for 'xdel' command".to_string(),
                ));
            }
            let key = &args[0];

            // Passive expiration
            if let Some(entry) = data.get(key) {
                if entry.is_expired() {
                    data.remove(key);
                    return Ok(RedisValue::Integer(0));
                }
            }

            match data.get_mut(key) {
                None => Ok(RedisValue::Integer(0)),
                Some(entry) => match entry.data {
                    ValueData::Stream(ref mut stream) => {
                        let mut count = 0i64;
                        for id_bytes in &args[1..] {
                            let id_str = String::from_utf8_lossy(id_bytes);
                            let parts: Vec<&str> = id_str.splitn(2, '-').collect();
                            if parts.len() == 2 {
                                let ms: u64 = parts[0].parse().unwrap_or(0);
                                let seq: u64 = parts[1].parse().unwrap_or(0);
                                if stream.entries.remove(&(ms, seq)).is_some() {
                                    count += 1;
                                }
                            }
                        }
                        Ok(RedisValue::Integer(count))
                    }
                    _ => Ok(RedisValue::Error(
                        "WRONGTYPE Operation against a key holding the wrong kind of value"
                            .to_string(),
                    )),
                },
            }
        }

        "XACK" => {
            // XACK key group id [id ...]
            if args.len() < 3 {
                return Ok(RedisValue::Error(
                    "ERR wrong number of arguments for 'xack' command".to_string(),
                ));
            }
            let key = &args[0];
            let group = &args[1];

            // Passive expiration
            if let Some(entry) = data.get(key) {
                if entry.is_expired() {
                    data.remove(key);
                    return Ok(RedisValue::Integer(0));
                }
            }

            match data.get_mut(key) {
                None => Ok(RedisValue::Integer(0)),
                Some(entry) => match entry.data {
                    ValueData::Stream(ref mut stream) => {
                        let cg = match stream.groups.get_mut(group) {
                            Some(g) => g,
                            None => return Ok(RedisValue::Integer(0)),
                        };
                        let mut count = 0i64;
                        for id_bytes in &args[2..] {
                            let id_str = String::from_utf8_lossy(id_bytes);
                            let parts: Vec<&str> = id_str.splitn(2, '-').collect();
                            if parts.len() == 2 {
                                let ms: u64 = parts[0].parse().unwrap_or(0);
                                let seq: u64 = parts[1].parse().unwrap_or(0);
                                let stream_id = (ms, seq);
                                // Search all consumers for this pending entry
                                for consumer in cg.consumers.values_mut() {
                                    if consumer.pending.remove(&stream_id).is_some() {
                                        count += 1;
                                        break;
                                    }
                                }
                            }
                        }
                        Ok(RedisValue::Integer(count))
                    }
                    _ => Ok(RedisValue::Error(
                        "WRONGTYPE Operation against a key holding the wrong kind of value"
                            .to_string(),
                    )),
                },
            }
        }

        "XCLAIM" => {
            // XCLAIM key group consumer min-idle-time id [id ...] [IDLE ms] [FORCE] [JUSTID] [RETRYCOUNT n]
            if args.len() < 5 {
                return Ok(RedisValue::Error(
                    "ERR wrong number of arguments for 'xclaim' command".to_string(),
                ));
            }
            let key = &args[0];
            let group = &args[1];
            let consumer = args[2].clone();
            let min_idle_str = String::from_utf8_lossy(&args[3]);
            let min_idle_time: u64 = min_idle_str.parse().map_err(|_| {
                "ERR Invalid min-idle-time argument for XCLAIM".to_string()
            })?;

            // Parse remaining args: IDs and optional flags
            let mut ids = Vec::new();
            let mut idle: Option<u64> = None;
            let mut force = false;
            let mut justid = false;
            let mut retrycount: Option<u64> = None;
            let mut i = 4;
            while i < args.len() {
                let arg_upper = String::from_utf8_lossy(&args[i]).to_uppercase();
                match arg_upper.as_str() {
                    "IDLE" => {
                        if i + 1 < args.len() {
                            idle = Some(
                                String::from_utf8_lossy(&args[i + 1])
                                    .parse()
                                    .unwrap_or(0),
                            );
                            i += 2;
                            continue;
                        }
                        i += 1;
                        continue;
                    }
                    "RETRYCOUNT" => {
                        if i + 1 < args.len() {
                            retrycount = Some(
                                String::from_utf8_lossy(&args[i + 1])
                                    .parse()
                                    .unwrap_or(0),
                            );
                            i += 2;
                            continue;
                        }
                        i += 1;
                        continue;
                    }
                    "FORCE" => {
                        force = true;
                        i += 1;
                        continue;
                    }
                    "JUSTID" => {
                        justid = true;
                        i += 1;
                        continue;
                    }
                    _ => {
                        // Must be a stream ID
                        let id_str = String::from_utf8_lossy(&args[i]);
                        if let Some(parsed) = crate::commands::streams::parse_stream_id(&id_str) {
                            ids.push(parsed);
                        }
                        i += 1;
                    }
                }
            }

            // Passive expiration
            if let Some(entry) = data.get(key) {
                if entry.is_expired() {
                    data.remove(key);
                    return Ok(RedisValue::Error(format!(
                        "NOGROUP No such key '{}' or consumer group '{}' in XCLAIM",
                        String::from_utf8_lossy(key),
                        String::from_utf8_lossy(group),
                    )));
                }
            }

            match data.get_mut(key) {
                None => Ok(RedisValue::Error(format!(
                    "NOGROUP No such key '{}' or consumer group '{}' in XCLAIM",
                    String::from_utf8_lossy(key),
                    String::from_utf8_lossy(group),
                ))),
                Some(entry) => match entry.data {
                    ValueData::Stream(ref mut stream) => {
                        let cg = match stream.groups.get_mut(group) {
                            Some(g) => g,
                            None => {
                                return Ok(RedisValue::Error(format!(
                                    "NOGROUP No such key '{}' or consumer group '{}' in XCLAIM",
                                    String::from_utf8_lossy(key),
                                    String::from_utf8_lossy(group),
                                )));
                            }
                        };

                        let now = std::time::Instant::now();
                        let min_idle = std::time::Duration::from_millis(min_idle_time);
                        let mut claimed = Vec::new();

                        for &id in &ids {
                            // Find entry in any consumer's PEL
                            let mut found_consumer: Option<Bytes> = None;
                            let mut found_entry: Option<crate::store::PendingEntry> = None;
                            for (cname, c) in cg.consumers.iter() {
                                if let Some(pe) = c.pending.get(&id) {
                                    let idle_dur = now.duration_since(pe.delivery_time);
                                    if idle_dur >= min_idle || force {
                                        found_consumer = Some(cname.clone());
                                        found_entry = Some(pe.clone());
                                    }
                                    break;
                                }
                            }

                            // Force create if not found
                            if found_consumer.is_none() && force {
                                if stream.entries.contains_key(&id) {
                                    let new_dt = match idle {
                                        Some(ms) => now - std::time::Duration::from_millis(ms),
                                        None => now,
                                    };
                                    let target = cg
                                        .consumers
                                        .entry(consumer.clone())
                                        .or_insert_with(|| crate::store::Consumer {
                                            pending: HashMap::new(),
                                        });
                                    target.pending.insert(
                                        id,
                                        crate::store::PendingEntry {
                                            delivery_time: new_dt,
                                            delivery_count: retrycount.unwrap_or(1),
                                        },
                                    );
                                    if justid {
                                        claimed.push(RedisValue::BulkString(Bytes::from(
                                            format!("{}-{}", id.0, id.1),
                                        )));
                                    } else if let Some(fields) = stream.entries.get(&id) {
                                        let mut items = Vec::new();
                                        items.push(RedisValue::BulkString(Bytes::from(
                                            format!("{}-{}", id.0, id.1),
                                        )));
                                        let mut field_items = Vec::new();
                                        for (fk, fv) in fields {
                                            field_items
                                                .push(RedisValue::BulkString(fk.clone()));
                                            field_items
                                                .push(RedisValue::BulkString(fv.clone()));
                                        }
                                        items.push(RedisValue::Array(field_items));
                                        claimed.push(RedisValue::Array(items));
                                    }
                                }
                                continue;
                            }

                            if let (Some(from_consumer), Some(pe)) =
                                (found_consumer, found_entry)
                            {
                                if let Some(orig) = cg.consumers.get_mut(&from_consumer) {
                                    orig.pending.remove(&id);
                                }
                                let new_dt = match idle {
                                    Some(ms) => now - std::time::Duration::from_millis(ms),
                                    None => pe.delivery_time,
                                };
                                let new_dc = retrycount.unwrap_or(pe.delivery_count + 1);
                                let target = cg
                                    .consumers
                                    .entry(consumer.clone())
                                    .or_insert_with(|| crate::store::Consumer {
                                        pending: HashMap::new(),
                                    });
                                target.pending.insert(
                                    id,
                                    crate::store::PendingEntry {
                                        delivery_time: new_dt,
                                        delivery_count: new_dc,
                                    },
                                );
                                if justid {
                                    claimed.push(RedisValue::BulkString(Bytes::from(
                                        format!("{}-{}", id.0, id.1),
                                    )));
                                } else if let Some(fields) = stream.entries.get(&id) {
                                    let mut items = Vec::new();
                                    items.push(RedisValue::BulkString(Bytes::from(
                                        format!("{}-{}", id.0, id.1),
                                    )));
                                    let mut field_items = Vec::new();
                                    for (fk, fv) in fields {
                                        field_items.push(RedisValue::BulkString(fk.clone()));
                                        field_items.push(RedisValue::BulkString(fv.clone()));
                                    }
                                    items.push(RedisValue::Array(field_items));
                                    claimed.push(RedisValue::Array(items));
                                }
                            }
                        }

                        Ok(RedisValue::Array(claimed))
                    }
                    _ => Ok(RedisValue::Error(
                        "WRONGTYPE Operation against a key holding the wrong kind of value"
                            .to_string(),
                    )),
                },
            }
        }

        // ── Pub/Sub commands ─────────────────────────────────────────
        "PUBLISH" => {
            if args.len() != 2 {
                return Ok(RedisValue::Error(
                    "ERR wrong number of arguments for 'publish' command".to_string(),
                ));
            }
            let channel = &args[0];
            let message = &args[1];

            match pubsub_tx {
                Some(tx) => {
                    // Send message through broadcast channel and return receiver count
                    let _ = tx.send(PubSubMessage {
                        kind: "message".to_string(),
                        pattern: None,
                        channel: channel.clone(),
                        data: message.clone(),
                    });
                    Ok(RedisValue::Integer(tx.receiver_count() as i64))
                }
                None => {
                    // No pubsub sender available
                    Ok(RedisValue::Integer(0))
                }
            }
        }

        _ => Ok(RedisValue::Error(format!("ERR unknown command '{}'", cmd))),
    }
}

/// Parse a score argument string (supports "-inf", "+inf", "inf", "(exclusive" prefix).
fn parse_score_arg(arg: &Bytes) -> f64 {
    let s = String::from_utf8_lossy(arg);
    let s = s.trim();
    match s {
        "-inf" => f64::NEG_INFINITY,
        "+inf" | "inf" => f64::INFINITY,
        _ => {
            if let Some(stripped) = s.strip_prefix('(') {
                // Exclusive bound - for simplicity we treat as inclusive
                // (full exclusive support would need Bound enum in range queries)
                stripped.parse::<f64>().unwrap_or(0.0)
            } else {
                s.parse::<f64>().unwrap_or(0.0)
            }
        }
    }
}
