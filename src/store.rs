use bytes::Bytes;
use ordered_float::OrderedFloat;
use parking_lot::RwLock;
use serde::{Serialize, Deserialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Bound;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, Notify};

use crate::commands::pubsub::glob_match;
use crate::commands::streams::StreamId;
use crate::scripting::{LuaEngine, RedisValue};

/// Dual-index sorted set matching Redis's skiplist+dict pattern.
/// BTreeMap for score-ordered range queries, HashMap for O(1) member->score lookup.
#[derive(Clone, Debug)]
pub struct SortedSet {
    pub by_score: BTreeMap<(OrderedFloat<f64>, Bytes), ()>,
    pub by_member: HashMap<Bytes, f64>,
}

impl SortedSet {
    pub fn new() -> Self {
        SortedSet {
            by_score: BTreeMap::new(),
            by_member: HashMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.by_member.len()
    }

    /// Insert a member with a score. Returns true if the member is NEW (not an update).
    /// Handles removing old score entry from by_score when updating.
    pub fn insert(&mut self, member: Bytes, score: f64) -> bool {
        if let Some(&old_score) = self.by_member.get(&member) {
            // Member exists -- remove old score entry, insert new one
            self.by_score
                .remove(&(OrderedFloat(old_score), member.clone()));
            self.by_score
                .insert((OrderedFloat(score), member.clone()), ());
            self.by_member.insert(member, score);
            false // not new
        } else {
            self.by_score
                .insert((OrderedFloat(score), member.clone()), ());
            self.by_member.insert(member, score);
            true // new member
        }
    }

    /// Remove a member. Returns true if member existed.
    pub fn remove(&mut self, member: &Bytes) -> bool {
        if let Some(score) = self.by_member.remove(member) {
            self.by_score
                .remove(&(OrderedFloat(score), member.clone()));
            true
        } else {
            false
        }
    }
}

/// Snapshot of a stream's metadata returned by XINFO STREAM.
/// Used by the Python binding to build a dict matching redis-py's keys.
#[derive(Clone, Debug)]
pub struct XInfoStreamSnapshot {
    pub length: usize,
    pub last_id: StreamId,
    pub groups_count: usize,
    pub first_entry: Option<(StreamId, HashMap<Bytes, Bytes>)>,
    pub last_entry: Option<(StreamId, HashMap<Bytes, Bytes>)>,
}

/// Redis stream data structure: ordered log of field-value entries keyed by StreamId.
#[derive(Clone, Debug)]
pub struct Stream {
    pub entries: BTreeMap<StreamId, HashMap<Bytes, Bytes>>,
    pub last_id: StreamId,
    pub groups: HashMap<Bytes, ConsumerGroup>,
}

impl Stream {
    pub fn new() -> Self {
        Stream {
            entries: BTreeMap::new(),
            last_id: (0, 0),
            groups: HashMap::new(),
        }
    }
}

/// A consumer group within a stream, tracking delivered IDs and per-consumer pending entries.
#[derive(Clone, Debug)]
pub struct ConsumerGroup {
    pub last_delivered_id: StreamId,
    pub consumers: HashMap<Bytes, Consumer>,
}

/// A consumer within a consumer group, tracking pending (unacknowledged) entries.
#[derive(Clone, Debug)]
pub struct Consumer {
    pub pending: HashMap<StreamId, PendingEntry>,
}

/// A pending entry in a consumer's PEL (pending entries list).
#[derive(Clone, Debug)]
pub struct PendingEntry {
    pub delivery_time: Instant,
    pub delivery_count: u64,
}

/// Represents the different data types a Redis key can hold.
#[derive(Clone, Debug)]
pub enum ValueData {
    /// Redis string value (raw bytes).
    String(Bytes),
    /// Redis hash value (field -> value mapping).
    Hash(HashMap<Bytes, Bytes>),
    /// Redis set value (unordered collection of unique members).
    Set(HashSet<Bytes>),
    /// Redis sorted set value (dual-index: score-ordered + member lookup).
    SortedSet(SortedSet),
    /// Redis stream value (ordered log of field-value entries).
    Stream(Stream),
}

/// A value entry in the store, containing typed data and optional expiration.
#[derive(Clone, Debug)]
pub struct ValueEntry {
    pub data: ValueData,
    pub expires_at: Option<Instant>,
}

impl ValueEntry {
    /// Create a new String-typed entry (backward compatible with original API).
    pub fn new(data: Bytes, ttl: Option<Duration>) -> Self {
        let expires_at = ttl.map(|d| Instant::now() + d);
        ValueEntry {
            data: ValueData::String(data),
            expires_at,
        }
    }

    /// Create a new empty Hash-typed entry with no expiration.
    pub fn new_hash() -> Self {
        ValueEntry {
            data: ValueData::Hash(HashMap::new()),
            expires_at: None,
        }
    }

    /// Create a new empty Set-typed entry with no expiration.
    pub fn new_set() -> Self {
        ValueEntry {
            data: ValueData::Set(HashSet::new()),
            expires_at: None,
        }
    }

    /// Create a new empty SortedSet-typed entry with no expiration.
    pub fn new_sorted_set() -> Self {
        ValueEntry {
            data: ValueData::SortedSet(SortedSet::new()),
            expires_at: None,
        }
    }

    /// Create a new empty Stream-typed entry with no expiration.
    pub fn new_stream() -> Self {
        ValueEntry {
            data: ValueData::Stream(Stream::new()),
            expires_at: None,
        }
    }

    pub fn is_expired(&self) -> bool {
        self.expires_at
            .map(|exp| Instant::now() >= exp)
            .unwrap_or(false)
    }
}

/// Errors that can occur during store operations.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("WRONGTYPE Operation against a key holding the wrong kind of value")]
    WrongType,
    /// NOGROUP error for missing key or consumer group.
    /// Parameters are `(group, key)` to preserve call-site order; the Display
    /// format reorders them to match Redis canonical phrasing
    /// "No such key '<key>' or consumer group '<group>'". The per-command
    /// suffix (" in XPENDING" / " in XREADGROUP" / etc.) is appended by the
    /// binding layer; the store stays command-agnostic.
    #[error("NOGROUP No such key '{1}' or consumer group '{0}'")]
    NoGroup(String, String),
    #[error("BUSYGROUP Consumer Group name already exists")]
    BusyGroup,
    #[error("ERR The XGROUP subcommand requires the key to exist")]
    KeyNotFound,
}

/// A pub/sub message delivered through the broadcast channel.
#[derive(Clone, Debug)]
pub struct PubSubMessage {
    pub kind: String,           // "message" or "pmessage"
    pub pattern: Option<Bytes>, // pattern that matched (for pmessage only)
    pub channel: Bytes,         // channel name
    pub data: Bytes,            // message payload
}

/// Registry tracking all active pub/sub subscriptions.
/// Separate from keyspace data -- pub/sub is orthogonal to key-value storage.
pub struct PubSubRegistry {
    /// Global broadcast sender -- all messages flow through here
    pub tx: broadcast::Sender<PubSubMessage>,
    /// Channel name -> set of subscriber IDs
    channel_subscribers: HashMap<Bytes, HashSet<u64>>,
    /// Pattern (glob) -> set of subscriber IDs
    pattern_subscribers: HashMap<Bytes, HashSet<u64>>,
    /// Subscriber ID -> set of channels subscribed
    subscriber_channels: HashMap<u64, HashSet<Bytes>>,
    /// Subscriber ID -> set of patterns subscribed
    subscriber_patterns: HashMap<u64, HashSet<Bytes>>,
    /// Next subscriber ID counter
    next_id: AtomicU64,
}

impl PubSubRegistry {
    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel(4096);
        PubSubRegistry {
            tx,
            channel_subscribers: HashMap::new(),
            pattern_subscribers: HashMap::new(),
            subscriber_channels: HashMap::new(),
            subscriber_patterns: HashMap::new(),
            next_id: AtomicU64::new(0),
        }
    }
}

pub struct Store {
    data: RwLock<HashMap<Bytes, ValueEntry>>,
    scripts: RwLock<HashMap<String, String>>,
    pub(crate) pubsub: RwLock<PubSubRegistry>,
    stream_notify: Arc<Notify>,
}

impl Store {
    pub fn new() -> Self {
        Store {
            data: RwLock::new(HashMap::new()),
            scripts: RwLock::new(HashMap::new()),
            pubsub: RwLock::new(PubSubRegistry::new()),
            stream_notify: Arc::new(Notify::new()),
        }
    }

    /// Get a reference to the stream notification handle for async waiting.
    pub fn stream_notify(&self) -> Arc<Notify> {
        self.stream_notify.clone()
    }

    // ── Persistence Methods ─────────────────────────────────────────

    /// Save the store to a file using crash-safe write (write-tmp, fsync, rename).
    pub fn save(&self, path: &str) -> Result<(), crate::persistence::PersistenceError> {
        crate::persistence::save_to_path(self, path)
    }

    /// Load data from a persistence file into this store, replacing current contents.
    /// Returns Ok(true) if data was loaded, Ok(false) if the file was missing.
    pub fn load_into(&self, path: &str) -> Result<bool, crate::persistence::PersistenceError> {
        match crate::persistence::load_from_path(path)? {
            Some(snapshot) => {
                let (data_map, scripts_map) = snapshot.into_runtime();
                *self.data.write() = data_map;
                *self.scripts.write() = scripts_map;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Get a write lock on the data map. Used by persistence for loading.
    pub fn data_write(
        &self,
    ) -> parking_lot::RwLockWriteGuard<'_, HashMap<Bytes, ValueEntry>> {
        self.data.write()
    }

    /// Get a write lock on the scripts map. Used by persistence for loading.
    pub fn scripts_write(
        &self,
    ) -> parking_lot::RwLockWriteGuard<'_, HashMap<String, String>> {
        self.scripts.write()
    }

    /// GET: Returns the value for a String-typed key, or None if missing/expired/wrong type.
    /// Passive expiration: removes expired keys on access.
    /// Returns None for Hash/Set keys (matches Redis behavior where GET on non-string returns error,
    /// but the WRONGTYPE error is raised at the Python layer).
    pub fn get(&self, key: &Bytes) -> Option<Bytes> {
        // First check with a read lock
        {
            let data = self.data.read();
            match data.get(key) {
                None => return None,
                Some(entry) if !entry.is_expired() => {
                    if let ValueData::String(ref v) = entry.data {
                        return Some(v.clone());
                    }
                    // Non-string type: return None (WRONGTYPE handled at Python layer)
                    return None;
                }
                Some(_) => {} // expired, fall through to remove
            }
        }
        // Key is expired -- upgrade to write lock and remove
        let mut data = self.data.write();
        if let Some(entry) = data.get(key) {
            if entry.is_expired() {
                data.remove(key);
            }
        }
        None
    }

    /// SET: Stores a key-value pair with optional TTL and conditional flags.
    /// Returns true on success, false when NX/XX condition fails.
    /// SET always overwrites any existing key regardless of its value type (matches Redis behavior).
    pub fn set(
        &self,
        key: Bytes,
        value: Bytes,
        ttl: Option<Duration>,
        nx: bool,
        xx: bool,
    ) -> bool {
        let mut data = self.data.write();

        // Check NX/XX conditions (treat expired keys as non-existent)
        let key_exists = data
            .get(&key)
            .map(|e| !e.is_expired())
            .unwrap_or(false);

        if nx && key_exists {
            return false; // NX: only set if NOT exists, but key exists
        }
        if xx && !key_exists {
            return false; // XX: only set if exists, but key doesn't exist
        }

        let entry = ValueEntry::new(value, ttl);
        data.insert(key, entry);
        true
    }

    /// DELETE: Removes one or more keys. Returns count of keys that existed (non-expired).
    pub fn delete(&self, keys: &[Bytes]) -> i64 {
        let mut data = self.data.write();
        let mut count = 0i64;
        for key in keys {
            if let Some(entry) = data.get(key) {
                if !entry.is_expired() {
                    count += 1;
                }
            }
            data.remove(key);
        }
        count
    }

    /// EXISTS: Returns count of keys that exist (non-expired).
    /// Note: a key counted multiple times if passed multiple times (matches Redis behavior).
    pub fn exists(&self, keys: &[Bytes]) -> i64 {
        let data = self.data.read();
        let mut count = 0i64;
        for key in keys {
            if let Some(entry) = data.get(key) {
                if !entry.is_expired() {
                    count += 1;
                }
            }
        }
        count
    }

    // ── Key Enumeration & Multi-Key Operations ────────────────────────

    /// KEYS: Returns all non-expired keys matching a glob pattern.
    pub fn keys(&self, pattern: &[u8]) -> Vec<Bytes> {
        let data = self.data.read();
        let mut result = Vec::new();
        for (key, entry) in data.iter() {
            if !entry.is_expired() && glob_match(pattern, key.as_ref()) {
                result.push(key.clone());
            }
        }
        result
    }

    /// TTL: Returns remaining time-to-live in seconds.
    /// Returns -2 if key does not exist (or is expired).
    /// Returns -1 if key exists but has no TTL set.
    /// Returns positive integer: remaining seconds (truncated, matching Redis).
    pub fn ttl(&self, key: &Bytes) -> i64 {
        let mut data = self.data.write();
        match data.get(key) {
            None => -2,
            Some(entry) if entry.is_expired() => {
                data.remove(key);
                -2
            }
            Some(entry) => match entry.expires_at {
                None => -1,
                Some(exp) => {
                    match exp.checked_duration_since(Instant::now()) {
                        Some(remaining) => remaining.as_secs() as i64,
                        None => {
                            // Expired between check and computation
                            data.remove(key);
                            -2
                        }
                    }
                }
            },
        }
    }

    /// MGET: Returns values for multiple keys at once in a single read lock.
    /// Returns None for missing, expired, or non-string keys (matches GET behavior).
    pub fn mget(&self, keys: &[Bytes]) -> Vec<Option<Bytes>> {
        let data = self.data.read();
        keys.iter().map(|key| {
            match data.get(key) {
                Some(entry) if !entry.is_expired() => {
                    if let ValueData::String(ref v) = entry.data {
                        Some(v.clone())
                    } else {
                        None
                    }
                }
                _ => None,
            }
        }).collect()
    }

    // ── Hash Operations ──────────────────────────────────────────────

    /// HSET: Sets field-value pairs in a hash. Creates the hash if it doesn't exist.
    /// Returns the count of NEW fields added (fields that were updated are not counted).
    /// Returns Err(WrongType) if the key holds a non-hash value.
    pub fn hset(&self, key: Bytes, fields: Vec<(Bytes, Bytes)>) -> Result<i64, StoreError> {
        let mut data = self.data.write();

        // Passive expiration: remove expired keys
        if let Some(entry) = data.get(&key) {
            if entry.is_expired() {
                data.remove(&key);
            }
        }

        let entry = data
            .entry(key)
            .or_insert_with(ValueEntry::new_hash);

        match entry.data {
            ValueData::Hash(ref mut map) => {
                let mut new_count = 0i64;
                for (field, value) in fields {
                    if !map.contains_key(&field) {
                        new_count += 1;
                    }
                    map.insert(field, value);
                }
                Ok(new_count)
            }
            _ => Err(StoreError::WrongType),
        }
    }

    /// HGET: Returns the value of a field in a hash.
    /// Returns Ok(None) if the key or field doesn't exist.
    /// Returns Err(WrongType) if the key holds a non-hash value.
    pub fn hget(&self, key: &Bytes, field: &Bytes) -> Result<Option<Bytes>, StoreError> {
        let mut data = self.data.write();

        // Passive expiration
        if let Some(entry) = data.get(key) {
            if entry.is_expired() {
                data.remove(key);
                return Ok(None);
            }
        }

        match data.get(key) {
            None => Ok(None),
            Some(entry) => match &entry.data {
                ValueData::Hash(map) => Ok(map.get(field).cloned()),
                _ => Err(StoreError::WrongType),
            },
        }
    }

    /// HDEL: Removes fields from a hash. Returns count of fields that were removed.
    /// Returns Ok(0) if the key doesn't exist.
    /// Returns Err(WrongType) if the key holds a non-hash value.
    pub fn hdel(&self, key: &Bytes, fields: &[Bytes]) -> Result<i64, StoreError> {
        let mut data = self.data.write();

        // Passive expiration
        if let Some(entry) = data.get(key) {
            if entry.is_expired() {
                data.remove(key);
                return Ok(0);
            }
        }

        match data.get_mut(key) {
            None => Ok(0),
            Some(entry) => match entry.data {
                ValueData::Hash(ref mut map) => {
                    let mut count = 0i64;
                    for field in fields {
                        if map.remove(field).is_some() {
                            count += 1;
                        }
                    }
                    Ok(count)
                }
                _ => Err(StoreError::WrongType),
            },
        }
    }

    /// HVALS: Returns all values in a hash.
    /// Returns Ok(empty vec) if the key doesn't exist.
    /// Returns Err(WrongType) if the key holds a non-hash value.
    pub fn hvals(&self, key: &Bytes) -> Result<Vec<Bytes>, StoreError> {
        let mut data = self.data.write();

        // Passive expiration
        if let Some(entry) = data.get(key) {
            if entry.is_expired() {
                data.remove(key);
                return Ok(Vec::new());
            }
        }

        match data.get(key) {
            None => Ok(Vec::new()),
            Some(entry) => match &entry.data {
                ValueData::Hash(map) => Ok(map.values().cloned().collect()),
                _ => Err(StoreError::WrongType),
            },
        }
    }

    /// HGETALL: Returns all field-value pairs in a hash as a HashMap.
    /// Returns Ok(empty map) if the key doesn't exist.
    /// Returns Err(WrongType) if the key holds a non-hash value.
    pub fn hgetall(&self, key: &Bytes) -> Result<HashMap<Bytes, Bytes>, StoreError> {
        let mut data = self.data.write();

        // Passive expiration
        if let Some(entry) = data.get(key) {
            if entry.is_expired() {
                data.remove(key);
                return Ok(HashMap::new());
            }
        }

        match data.get(key) {
            None => Ok(HashMap::new()),
            Some(entry) => match &entry.data {
                ValueData::Hash(map) => Ok(map.clone()),
                _ => Err(StoreError::WrongType),
            },
        }
    }

    /// HEXISTS: Returns true if the field exists in the hash.
    /// Returns Ok(false) if the key doesn't exist.
    /// Returns Err(WrongType) if the key holds a non-hash value.
    pub fn hexists(&self, key: &Bytes, field: &Bytes) -> Result<bool, StoreError> {
        let mut data = self.data.write();

        // Passive expiration
        if let Some(entry) = data.get(key) {
            if entry.is_expired() {
                data.remove(key);
                return Ok(false);
            }
        }

        match data.get(key) {
            None => Ok(false),
            Some(entry) => match &entry.data {
                ValueData::Hash(map) => Ok(map.contains_key(field)),
                _ => Err(StoreError::WrongType),
            },
        }
    }

    /// HINCRBY: Increment the integer value of a hash field by the given amount.
    /// Creates the hash and/or field if they don't exist (starting from 0).
    /// Returns the new value after incrementing.
    /// Returns Err(WrongType) if the key holds a non-hash value.
    pub fn hincrby(&self, key: Bytes, field: Bytes, increment: i64) -> Result<i64, StoreError> {
        let mut data = self.data.write();

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
                    .get(&field)
                    .and_then(|v| String::from_utf8_lossy(v).parse::<i64>().ok())
                    .unwrap_or(0);
                let new_val = current + increment;
                map.insert(field, Bytes::from(new_val.to_string()));
                Ok(new_val)
            }
            _ => Err(StoreError::WrongType),
        }
    }

    // ── Set Operations ───────────────────────────────────────────────

    /// SADD: Adds members to a set. Creates the set if it doesn't exist.
    /// Returns the count of NEW members added (existing members are not counted).
    /// Returns Err(WrongType) if the key holds a non-set value.
    pub fn sadd(&self, key: Bytes, members: Vec<Bytes>) -> Result<i64, StoreError> {
        let mut data = self.data.write();

        // Passive expiration
        if let Some(entry) = data.get(&key) {
            if entry.is_expired() {
                data.remove(&key);
            }
        }

        let entry = data
            .entry(key)
            .or_insert_with(ValueEntry::new_set);

        match entry.data {
            ValueData::Set(ref mut set) => {
                let mut new_count = 0i64;
                for member in members {
                    if set.insert(member) {
                        new_count += 1;
                    }
                }
                Ok(new_count)
            }
            _ => Err(StoreError::WrongType),
        }
    }

    /// SMEMBERS: Returns all members of a set.
    /// Returns Ok(empty vec) if the key doesn't exist.
    /// Returns Err(WrongType) if the key holds a non-set value.
    pub fn smembers(&self, key: &Bytes) -> Result<Vec<Bytes>, StoreError> {
        let mut data = self.data.write();

        // Passive expiration
        if let Some(entry) = data.get(key) {
            if entry.is_expired() {
                data.remove(key);
                return Ok(Vec::new());
            }
        }

        match data.get(key) {
            None => Ok(Vec::new()),
            Some(entry) => match &entry.data {
                ValueData::Set(set) => Ok(set.iter().cloned().collect()),
                _ => Err(StoreError::WrongType),
            },
        }
    }

    /// SISMEMBER: Returns true if the member exists in the set.
    /// Returns Ok(false) if the key doesn't exist.
    /// Returns Err(WrongType) if the key holds a non-set value.
    pub fn sismember(&self, key: &Bytes, member: &Bytes) -> Result<bool, StoreError> {
        let mut data = self.data.write();

        // Passive expiration
        if let Some(entry) = data.get(key) {
            if entry.is_expired() {
                data.remove(key);
                return Ok(false);
            }
        }

        match data.get(key) {
            None => Ok(false),
            Some(entry) => match &entry.data {
                ValueData::Set(set) => Ok(set.contains(member)),
                _ => Err(StoreError::WrongType),
            },
        }
    }

    /// SREM: Removes members from a set. Returns count of members that were removed.
    /// Returns Ok(0) if the key doesn't exist.
    /// Returns Err(WrongType) if the key holds a non-set value.
    pub fn srem(&self, key: &Bytes, members: &[Bytes]) -> Result<i64, StoreError> {
        let mut data = self.data.write();

        // Passive expiration
        if let Some(entry) = data.get(key) {
            if entry.is_expired() {
                data.remove(key);
                return Ok(0);
            }
        }

        match data.get_mut(key) {
            None => Ok(0),
            Some(entry) => match entry.data {
                ValueData::Set(ref mut set) => {
                    let mut count = 0i64;
                    for member in members {
                        if set.remove(member) {
                            count += 1;
                        }
                    }
                    Ok(count)
                }
                _ => Err(StoreError::WrongType),
            },
        }
    }

    // ── Sorted Set Operations ─────────────────────────────────────────

    /// ZADD: Adds members with scores to a sorted set.
    /// Flags: nx (only add new), xx (only update existing), gt (only update if new score > old),
    /// lt (only update if new score < old), ch (return count of changed instead of new).
    /// Returns count of new members added (or changed members if ch=true).
    pub fn zadd(
        &self,
        key: Bytes,
        members: Vec<(f64, Bytes)>,
        nx: bool,
        xx: bool,
        gt: bool,
        lt: bool,
        ch: bool,
    ) -> Result<i64, StoreError> {
        let mut data = self.data.write();

        // Passive expiration: remove expired keys
        if let Some(entry) = data.get(&key) {
            if entry.is_expired() {
                data.remove(&key);
            }
        }

        let entry = data
            .entry(key)
            .or_insert_with(ValueEntry::new_sorted_set);

        match entry.data {
            ValueData::SortedSet(ref mut zset) => {
                let mut added = 0i64;
                let mut changed = 0i64;

                for (score, member) in members {
                    if let Some(&old_score) = zset.by_member.get(&member) {
                        // Member exists
                        if nx {
                            continue; // NX: only add new, skip existing
                        }
                        // Check GT/LT constraints
                        if gt && score <= old_score {
                            continue;
                        }
                        if lt && score >= old_score {
                            continue;
                        }
                        // Update the score
                        if score != old_score {
                            zset.insert(member, score);
                            changed += 1;
                        }
                    } else {
                        // Member is new
                        if xx {
                            continue; // XX: only update existing, skip new
                        }
                        zset.insert(member, score);
                        added += 1;
                        changed += 1;
                    }
                }

                if ch { Ok(changed) } else { Ok(added) }
            }
            _ => Err(StoreError::WrongType),
        }
    }

    /// ZREM: Removes members from a sorted set. Returns count of members removed.
    /// Returns Ok(0) if the key doesn't exist.
    /// Returns Err(WrongType) if the key holds a non-sorted-set value.
    pub fn zrem(&self, key: &Bytes, members: &[Bytes]) -> Result<i64, StoreError> {
        let mut data = self.data.write();

        // Passive expiration
        if let Some(entry) = data.get(key) {
            if entry.is_expired() {
                data.remove(key);
                return Ok(0);
            }
        }

        match data.get_mut(key) {
            None => Ok(0),
            Some(entry) => match entry.data {
                ValueData::SortedSet(ref mut zset) => {
                    let mut count = 0i64;
                    for member in members {
                        if zset.remove(member) {
                            count += 1;
                        }
                    }
                    Ok(count)
                }
                _ => Err(StoreError::WrongType),
            },
        }
    }

    /// ZRANGE: Returns members by index range (0-based, supports negative indices).
    /// When withscores=true, returns (member, Some(score)) pairs.
    /// Returns Ok(empty vec) if the key doesn't exist.
    /// Returns Err(WrongType) if the key holds a non-sorted-set value.
    pub fn zrange(
        &self,
        key: &Bytes,
        start: i64,
        stop: i64,
        withscores: bool,
    ) -> Result<Vec<(Bytes, Option<f64>)>, StoreError> {
        let mut data = self.data.write();

        // Passive expiration
        if let Some(entry) = data.get(key) {
            if entry.is_expired() {
                data.remove(key);
                return Ok(Vec::new());
            }
        }

        match data.get(key) {
            None => Ok(Vec::new()),
            Some(entry) => match &entry.data {
                ValueData::SortedSet(zset) => {
                    let len = zset.len() as i64;
                    if len == 0 {
                        return Ok(Vec::new());
                    }

                    // Convert negative indices
                    let mut real_start = if start < 0 { len + start } else { start };
                    let mut real_stop = if stop < 0 { len + stop } else { stop };

                    // Clamp
                    if real_start < 0 {
                        real_start = 0;
                    }
                    if real_stop >= len {
                        real_stop = len - 1;
                    }

                    if real_start > real_stop || real_start >= len {
                        return Ok(Vec::new());
                    }

                    let result: Vec<(Bytes, Option<f64>)> = zset
                        .by_score
                        .iter()
                        .skip(real_start as usize)
                        .take((real_stop - real_start + 1) as usize)
                        .map(|((score, member), _)| {
                            let s = if withscores { Some(score.0) } else { None };
                            (member.clone(), s)
                        })
                        .collect();

                    Ok(result)
                }
                _ => Err(StoreError::WrongType),
            },
        }
    }

    /// ZRANGEBYSCORE: Returns members with scores in [min, max] range.
    /// min and max are f64 where -inf = f64::NEG_INFINITY and +inf = f64::INFINITY.
    /// Returns Ok(empty vec) if the key doesn't exist.
    /// Returns Err(WrongType) if the key holds a non-sorted-set value.
    pub fn zrangebyscore(
        &self,
        key: &Bytes,
        min: f64,
        max: f64,
        withscores: bool,
    ) -> Result<Vec<(Bytes, Option<f64>)>, StoreError> {
        let mut data = self.data.write();

        // Passive expiration
        if let Some(entry) = data.get(key) {
            if entry.is_expired() {
                data.remove(key);
                return Ok(Vec::new());
            }
        }

        match data.get(key) {
            None => Ok(Vec::new()),
            Some(entry) => match &entry.data {
                ValueData::SortedSet(zset) => {
                    let lower = Bound::Included((OrderedFloat(min), Bytes::new()));
                    let result: Vec<(Bytes, Option<f64>)> = zset
                        .by_score
                        .range((lower, Bound::Unbounded))
                        .take_while(|((score, _), _)| score.0 <= max)
                        .map(|((score, member), _)| {
                            let s = if withscores { Some(score.0) } else { None };
                            (member.clone(), s)
                        })
                        .collect();

                    Ok(result)
                }
                _ => Err(StoreError::WrongType),
            },
        }
    }

    /// ZRANGESTORE: Stores the result of a ZRANGEBYSCORE into a destination key.
    /// Returns the count of elements stored.
    /// If src key is missing, returns 0. If src is wrong type, returns Err(WrongType).
    pub fn zrangestore(
        &self,
        dst: Bytes,
        src: &Bytes,
        min: f64,
        max: f64,
    ) -> Result<i64, StoreError> {
        let mut data = self.data.write();

        // Passive expiration on src
        if let Some(entry) = data.get(src) {
            if entry.is_expired() {
                data.remove(src);
            }
        }

        // Get members from source in score range
        let members_to_store: Vec<(f64, Bytes)> = match data.get(src) {
            None => Vec::new(),
            Some(entry) => match &entry.data {
                ValueData::SortedSet(zset) => {
                    let lower = Bound::Included((OrderedFloat(min), Bytes::new()));
                    zset.by_score
                        .range((lower, Bound::Unbounded))
                        .take_while(|((score, _), _)| score.0 <= max)
                        .map(|((score, member), _)| (score.0, member.clone()))
                        .collect()
                }
                _ => return Err(StoreError::WrongType),
            },
        };

        let count = members_to_store.len() as i64;

        if count == 0 {
            // Remove destination if it exists (empty range means no key)
            data.remove(&dst);
        } else {
            // Create a new sorted set for the destination
            let mut new_zset = SortedSet::new();
            for (score, member) in members_to_store {
                new_zset.insert(member, score);
            }
            data.insert(
                dst,
                ValueEntry {
                    data: ValueData::SortedSet(new_zset),
                    expires_at: None,
                },
            );
        }

        Ok(count)
    }

    /// ZREMRANGEBYSCORE: Removes all members with scores in [min, max] range.
    /// Returns count of members removed.
    /// Returns Ok(0) if the key doesn't exist.
    /// Returns Err(WrongType) if the key holds a non-sorted-set value.
    pub fn zremrangebyscore(
        &self,
        key: &Bytes,
        min: f64,
        max: f64,
    ) -> Result<i64, StoreError> {
        let mut data = self.data.write();

        // Passive expiration
        if let Some(entry) = data.get(key) {
            if entry.is_expired() {
                data.remove(key);
                return Ok(0);
            }
        }

        match data.get_mut(key) {
            None => Ok(0),
            Some(entry) => match entry.data {
                ValueData::SortedSet(ref mut zset) => {
                    // Collect members to remove
                    let lower = Bound::Included((OrderedFloat(min), Bytes::new()));
                    let to_remove: Vec<Bytes> = zset
                        .by_score
                        .range((lower, Bound::Unbounded))
                        .take_while(|((score, _), _)| score.0 <= max)
                        .map(|((_, member), _)| member.clone())
                        .collect();

                    let count = to_remove.len() as i64;
                    for member in &to_remove {
                        zset.remove(member);
                    }
                    Ok(count)
                }
                _ => Err(StoreError::WrongType),
            },
        }
    }

    /// ZCARD: Returns the cardinality (member count) of a sorted set.
    /// Returns 0 if the key doesn't exist.
    /// Returns Err(WrongType) if the key holds a non-sorted-set value.
    pub fn zcard(&self, key: &Bytes) -> Result<i64, StoreError> {
        let mut data = self.data.write();

        // Passive expiration
        if let Some(entry) = data.get(key) {
            if entry.is_expired() {
                data.remove(key);
                return Ok(0);
            }
        }

        match data.get(key) {
            None => Ok(0),
            Some(entry) => match &entry.data {
                ValueData::SortedSet(zset) => Ok(zset.len() as i64),
                _ => Err(StoreError::WrongType),
            },
        }
    }

    /// ZSCORE: Returns the score of a member in a sorted set.
    /// Returns Ok(None) if the key or member doesn't exist.
    /// Returns Err(WrongType) if the key holds a non-sorted-set value.
    pub fn zscore(&self, key: &Bytes, member: &Bytes) -> Result<Option<f64>, StoreError> {
        let mut data = self.data.write();

        // Passive expiration
        if let Some(entry) = data.get(key) {
            if entry.is_expired() {
                data.remove(key);
                return Ok(None);
            }
        }

        match data.get(key) {
            None => Ok(None),
            Some(entry) => match &entry.data {
                ValueData::SortedSet(zset) => Ok(zset.by_member.get(member).copied()),
                _ => Err(StoreError::WrongType),
            },
        }
    }

    /// ZCOUNT: Returns the number of members in a sorted set with scores in [min, max] range.
    /// Returns 0 if the key doesn't exist.
    /// Returns Err(WrongType) if the key holds a non-sorted-set value.
    pub fn zcount(&self, key: &Bytes, min: f64, max: f64) -> Result<i64, StoreError> {
        let mut data = self.data.write();

        // Passive expiration
        if let Some(entry) = data.get(key) {
            if entry.is_expired() {
                data.remove(key);
                return Ok(0);
            }
        }

        match data.get(key) {
            None => Ok(0),
            Some(entry) => match &entry.data {
                ValueData::SortedSet(zset) => {
                    let lower = Bound::Included((OrderedFloat(min), Bytes::new()));
                    let count = zset
                        .by_score
                        .range((lower, Bound::Unbounded))
                        .take_while(|((score, _), _)| score.0 <= max)
                        .count();
                    Ok(count as i64)
                }
                _ => Err(StoreError::WrongType),
            },
        }
    }

    // ── Key Operations ───────────────────────────────────────────────

    /// EXPIRE: Set a timeout on a key in seconds. Returns true if the key exists and timeout was set.
    /// Returns false if the key does not exist.
    pub fn expire(&self, key: &Bytes, seconds: u64) -> bool {
        let mut data = self.data.write();

        // Passive expiration
        if let Some(entry) = data.get(key) {
            if entry.is_expired() {
                data.remove(key);
                return false;
            }
        }

        match data.get_mut(key) {
            None => false,
            Some(entry) => {
                entry.expires_at = Some(std::time::Instant::now() + Duration::from_secs(seconds));
                true
            }
        }
    }

    // ── Stream Operations ─────────────────────────────────────────────

    /// XADD: Appends an entry to a stream. Auto-generates a monotonic ID if none is provided.
    /// Returns the generated StreamId.
    /// Returns Err(WrongType) if the key holds a non-stream value.
    pub fn xadd(
        &self,
        key: Bytes,
        fields: HashMap<Bytes, Bytes>,
        id: Option<StreamId>,
    ) -> Result<StreamId, StoreError> {
        let mut data = self.data.write();

        // Passive expiration
        if let Some(entry) = data.get(&key) {
            if entry.is_expired() {
                data.remove(&key);
            }
        }

        let entry = data.entry(key).or_insert_with(ValueEntry::new_stream);

        match entry.data {
            ValueData::Stream(ref mut stream) => {
                let new_id = match id {
                    Some(explicit_id) => {
                        // Validate that explicit ID is greater than last_id
                        if explicit_id <= stream.last_id {
                            return Err(StoreError::WrongType); // Redis returns ERR but we reuse WrongType for simplicity
                        }
                        explicit_id
                    }
                    None => {
                        // Auto-generate ID using system time
                        let ms = std::time::SystemTime::UNIX_EPOCH
                            .elapsed()
                            .unwrap()
                            .as_millis() as u64;
                        if ms > stream.last_id.0 {
                            (ms, 0)
                        } else {
                            // Same or earlier millisecond: increment sequence
                            (stream.last_id.0, stream.last_id.1 + 1)
                        }
                    }
                };

                stream.entries.insert(new_id, fields);
                stream.last_id = new_id;
                // Wake any blocking XREADGROUP waiters
                self.stream_notify.notify_waiters();
                Ok(new_id)
            }
            _ => Err(StoreError::WrongType),
        }
    }

    /// XREAD: Reads entries from one or more streams after the given IDs.
    /// Returns a vec of (stream_name, entries) pairs for streams that have data.
    /// Skips streams that don't exist (does not include them in result).
    /// Returns Err(WrongType) if any key exists but is not a Stream.
    pub fn xread(
        &self,
        keys: &[Bytes],
        ids: &[StreamId],
        count: Option<usize>,
    ) -> Result<Vec<(Bytes, Vec<(StreamId, HashMap<Bytes, Bytes>)>)>, StoreError> {
        let mut data = self.data.write();
        let mut result = Vec::new();

        for (key, start_id) in keys.iter().zip(ids.iter()) {
            // Passive expiration
            if let Some(entry) = data.get(key) {
                if entry.is_expired() {
                    data.remove(key);
                    continue;
                }
            }

            match data.get(key) {
                None => continue, // Skip non-existent streams
                Some(entry) => match &entry.data {
                    ValueData::Stream(stream) => {
                        // Collect entries with id > start_id
                        let entries: Vec<(StreamId, HashMap<Bytes, Bytes>)> = stream
                            .entries
                            .range((
                                std::ops::Bound::Excluded(*start_id),
                                std::ops::Bound::Unbounded,
                            ))
                            .take(count.unwrap_or(usize::MAX))
                            .map(|(id, fields)| (*id, fields.clone()))
                            .collect();

                        if !entries.is_empty() {
                            result.push((key.clone(), entries));
                        }
                    }
                    _ => return Err(StoreError::WrongType),
                },
            }
        }

        Ok(result)
    }

    /// XLEN: Returns the number of entries in a stream.
    /// Returns 0 if the key doesn't exist.
    /// Returns Err(WrongType) if the key holds a non-stream value.
    pub fn xlen(&self, key: &Bytes) -> Result<usize, StoreError> {
        let mut data = self.data.write();

        // Passive expiration
        if let Some(entry) = data.get(key) {
            if entry.is_expired() {
                data.remove(key);
                return Ok(0);
            }
        }

        match data.get(key) {
            None => Ok(0),
            Some(entry) => match &entry.data {
                ValueData::Stream(stream) => Ok(stream.entries.len()),
                _ => Err(StoreError::WrongType),
            },
        }
    }

    /// Returns the current last-generated-id of a stream, or None if the key
    /// doesn't exist, has expired, or holds a non-stream value. Used by the
    /// XREAD Python binding to resolve "$" at call time.
    pub fn stream_last_id(&self, key: &Bytes) -> Option<StreamId> {
        let mut data = self.data.write();

        // Passive expiration
        if let Some(entry) = data.get(key) {
            if entry.is_expired() {
                data.remove(key);
                return None;
            }
        }

        match data.get(key)?.data {
            ValueData::Stream(ref s) => Some(s.last_id),
            _ => None,
        }
    }

    /// XTRIM: Trims a stream by maxlen or minid strategy.
    /// Returns the number of entries removed.
    /// Returns 0 if the key doesn't exist.
    /// Returns Err(WrongType) if the key holds a non-stream value.
    pub fn xtrim(
        &self,
        key: &Bytes,
        maxlen: Option<usize>,
        minid: Option<StreamId>,
    ) -> Result<usize, StoreError> {
        let mut data = self.data.write();

        // Passive expiration
        if let Some(entry) = data.get(key) {
            if entry.is_expired() {
                data.remove(key);
                return Ok(0);
            }
        }

        match data.get_mut(key) {
            None => Ok(0),
            Some(entry) => match entry.data {
                ValueData::Stream(ref mut stream) => {
                    let mut removed = 0usize;

                    if let Some(max) = maxlen {
                        while stream.entries.len() > max {
                            if let Some(first_key) = stream.entries.keys().next().copied() {
                                stream.entries.remove(&first_key);
                                removed += 1;
                            } else {
                                break;
                            }
                        }
                    }

                    if let Some(min) = minid {
                        // Remove all entries with id < minid
                        let to_remove: Vec<StreamId> = stream
                            .entries
                            .range(..min)
                            .map(|(id, _)| *id)
                            .collect();
                        removed += to_remove.len();
                        for id in to_remove {
                            stream.entries.remove(&id);
                        }
                    }

                    Ok(removed)
                }
                _ => Err(StoreError::WrongType),
            },
        }
    }

    /// XDEL: Deletes specific entries from a stream by ID.
    /// Returns the count of entries actually deleted.
    /// Returns 0 if the key doesn't exist.
    /// Returns Err(WrongType) if the key holds a non-stream value.
    pub fn xdel(&self, key: &Bytes, ids: &[StreamId]) -> Result<i64, StoreError> {
        let mut data = self.data.write();

        // Passive expiration
        if let Some(entry) = data.get(key) {
            if entry.is_expired() {
                data.remove(key);
                return Ok(0);
            }
        }

        match data.get_mut(key) {
            None => Ok(0),
            Some(entry) => match entry.data {
                ValueData::Stream(ref mut stream) => {
                    let mut count = 0i64;
                    for id in ids {
                        if stream.entries.remove(id).is_some() {
                            count += 1;
                        }
                    }
                    Ok(count)
                }
                _ => Err(StoreError::WrongType),
            },
        }
    }

    /// XRANGE: Returns stream entries in the given ID range [min, max].
    /// Supports "-" as minimum (0,0) and "+" as maximum (u64::MAX, u64::MAX).
    /// Optional count parameter limits the number of results.
    /// Returns Ok(empty vec) if the key doesn't exist.
    /// Returns Err(WrongType) if the key holds a non-stream value.
    pub fn xrange(
        &self,
        key: &Bytes,
        min: StreamId,
        max: StreamId,
        count: Option<usize>,
    ) -> Result<Vec<(StreamId, HashMap<Bytes, Bytes>)>, StoreError> {
        let mut data = self.data.write();

        // Passive expiration
        if let Some(entry) = data.get(key) {
            if entry.is_expired() {
                data.remove(key);
                return Ok(Vec::new());
            }
        }

        match data.get(key) {
            None => Ok(Vec::new()),
            Some(entry) => match &entry.data {
                ValueData::Stream(stream) => {
                    let entries: Vec<(StreamId, HashMap<Bytes, Bytes>)> = stream
                        .entries
                        .range(min..=max)
                        .take(count.unwrap_or(usize::MAX))
                        .map(|(id, fields)| (*id, fields.clone()))
                        .collect();
                    Ok(entries)
                }
                _ => Err(StoreError::WrongType),
            },
        }
    }

    /// Sweep up to 20 expired keys from the keyspace.
    /// Called periodically by the background expiration task.
    /// Returns the number of keys removed.
    pub fn sweep_expired(&self) -> usize {
        let mut data = self.data.write();
        let mut to_remove = Vec::new();
        let mut checked = 0;

        for (key, entry) in data.iter() {
            if entry.expires_at.is_some() {
                checked += 1;
                if entry.is_expired() {
                    to_remove.push(key.clone());
                }
                if checked >= 20 {
                    break;
                }
            }
        }

        let count = to_remove.len();
        for key in to_remove {
            data.remove(&key);
        }
        count
    }

    // ── Consumer Group Operations ─────────────────────────────────────

    /// XGROUP CREATE: Creates a consumer group on a stream.
    /// If mkstream is true and the key doesn't exist, creates an empty stream.
    /// The id parameter sets the last-delivered-id for the group.
    /// A sentinel value of (u64::MAX, u64::MAX) means "use stream's last_id" (i.e., "$").
    pub fn xgroup_create(
        &self,
        key: &Bytes,
        group: Bytes,
        id: StreamId,
        mkstream: bool,
    ) -> Result<(), StoreError> {
        let mut data = self.data.write();

        // Passive expiration
        if let Some(entry) = data.get(key) {
            if entry.is_expired() {
                data.remove(key);
            }
        }

        // If key doesn't exist, check mkstream
        if !data.contains_key(key) {
            if mkstream {
                data.insert(key.clone(), ValueEntry::new_stream());
            } else {
                return Err(StoreError::KeyNotFound);
            }
        }

        let entry = data.get_mut(key).unwrap();
        match entry.data {
            ValueData::Stream(ref mut stream) => {
                // Check for duplicate group name
                if stream.groups.contains_key(&group) {
                    return Err(StoreError::BusyGroup);
                }

                // Resolve "$" sentinel to stream's last_id
                let resolved_id = if id == (u64::MAX, u64::MAX) {
                    stream.last_id
                } else {
                    id
                };

                stream.groups.insert(
                    group,
                    ConsumerGroup {
                        last_delivered_id: resolved_id,
                        consumers: HashMap::new(),
                    },
                );
                Ok(())
            }
            _ => Err(StoreError::WrongType),
        }
    }

    /// XGROUP DESTROY: Removes a consumer group from a stream.
    /// Returns true if the group existed, false otherwise.
    pub fn xgroup_destroy(&self, key: &Bytes, group: &Bytes) -> Result<bool, StoreError> {
        let mut data = self.data.write();

        // Passive expiration
        if let Some(entry) = data.get(key) {
            if entry.is_expired() {
                data.remove(key);
                return Ok(false);
            }
        }

        match data.get_mut(key) {
            None => Ok(false),
            Some(entry) => match entry.data {
                ValueData::Stream(ref mut stream) => Ok(stream.groups.remove(group).is_some()),
                _ => Err(StoreError::WrongType),
            },
        }
    }

    /// XREADGROUP: Reads entries from streams as a consumer in a group.
    /// If id is ">" (represented as the string), delivers new messages after group's last_delivered_id.
    /// Otherwise, returns pending entries for this consumer with id >= the specified id.
    /// Requires write lock because it mutates PEL and last_delivered_id.
    pub fn xreadgroup(
        &self,
        group: &Bytes,
        consumer: &Bytes,
        keys: &[Bytes],
        ids: &[String],
        count: Option<usize>,
    ) -> Result<Vec<(Bytes, Vec<(StreamId, HashMap<Bytes, Bytes>)>)>, StoreError> {
        let mut data = self.data.write();
        let mut result = Vec::new();

        for (key, id_str) in keys.iter().zip(ids.iter()) {
            // Passive expiration
            if let Some(entry) = data.get(key) {
                if entry.is_expired() {
                    data.remove(key);
                }
            }

            // Key must exist
            let entry = match data.get_mut(key) {
                None => {
                    return Err(StoreError::NoGroup(
                        String::from_utf8_lossy(group.as_ref()).into_owned(),
                        String::from_utf8_lossy(key.as_ref()).into_owned(),
                    ));
                }
                Some(e) => e,
            };

            let stream = match entry.data {
                ValueData::Stream(ref mut s) => s,
                _ => return Err(StoreError::WrongType),
            };

            // Get the consumer group
            let cg = match stream.groups.get_mut(group) {
                None => {
                    return Err(StoreError::NoGroup(
                        String::from_utf8_lossy(group.as_ref()).into_owned(),
                        String::from_utf8_lossy(key.as_ref()).into_owned(),
                    ));
                }
                Some(g) => g,
            };

            if id_str == ">" {
                // Deliver NEW messages (entries after group.last_delivered_id)
                let entries: Vec<(StreamId, HashMap<Bytes, Bytes>)> = stream
                    .entries
                    .range((
                        std::ops::Bound::Excluded(cg.last_delivered_id),
                        std::ops::Bound::Unbounded,
                    ))
                    .take(count.unwrap_or(usize::MAX))
                    .map(|(id, fields)| (*id, fields.clone()))
                    .collect();

                // Update last_delivered_id and add to consumer's PEL
                if !entries.is_empty() {
                    // Auto-create consumer if not present
                    let consumer_entry = cg
                        .consumers
                        .entry(consumer.clone())
                        .or_insert_with(|| Consumer {
                            pending: HashMap::new(),
                        });

                    for (entry_id, _) in &entries {
                        cg.last_delivered_id = *entry_id;
                        consumer_entry.pending.insert(
                            *entry_id,
                            PendingEntry {
                                delivery_time: Instant::now(),
                                delivery_count: 1,
                            },
                        );
                    }

                    result.push((key.clone(), entries));
                }
            } else {
                // Return pending entries for this consumer with id >= parsed id
                let start_id: StreamId = if id_str == "0" || id_str == "0-0" {
                    (0, 0)
                } else {
                    crate::commands::streams::parse_stream_id(id_str).unwrap_or((0, 0))
                };

                // Get consumer's pending entries
                let consumer_entry = match cg.consumers.get(consumer) {
                    Some(c) => c,
                    None => {
                        // Consumer doesn't exist yet, no pending entries
                        continue;
                    }
                };

                // Collect pending entry IDs >= start_id that still exist in the stream
                let mut pending_ids: Vec<StreamId> = consumer_entry
                    .pending
                    .keys()
                    .filter(|id| **id >= start_id)
                    .copied()
                    .collect();
                pending_ids.sort();

                if let Some(max) = count {
                    pending_ids.truncate(max);
                }

                let entries: Vec<(StreamId, HashMap<Bytes, Bytes>)> = pending_ids
                    .into_iter()
                    .filter_map(|id| {
                        stream.entries.get(&id).map(|fields| (id, fields.clone()))
                    })
                    .collect();

                if !entries.is_empty() {
                    result.push((key.clone(), entries));
                }
            }
        }

        Ok(result)
    }

    /// XACK: Acknowledges messages in a consumer group, removing them from the PEL.
    /// Returns the count of messages that were actually acknowledged (removed from PEL).
    pub fn xack(
        &self,
        key: &Bytes,
        group: &Bytes,
        ids: &[StreamId],
    ) -> Result<i64, StoreError> {
        let mut data = self.data.write();

        // Passive expiration
        if let Some(entry) = data.get(key) {
            if entry.is_expired() {
                data.remove(key);
                return Ok(0);
            }
        }

        let entry = match data.get_mut(key) {
            None => return Ok(0),
            Some(e) => e,
        };

        let stream = match entry.data {
            ValueData::Stream(ref mut s) => s,
            _ => return Err(StoreError::WrongType),
        };

        // Get consumer group or return 0 if doesn't exist (Redis behavior)
        let cg = match stream.groups.get_mut(group) {
            None => return Ok(0),
            Some(g) => g,
        };

        let mut count = 0i64;
        for id in ids {
            // Iterate all consumers to find and remove the pending entry
            for consumer in cg.consumers.values_mut() {
                if consumer.pending.remove(id).is_some() {
                    count += 1;
                    break; // Each id can only be in one consumer's PEL
                }
            }
        }

        Ok(count)
    }

    /// XAUTOCLAIM: Reclaims idle pending messages from other consumers and transfers
    /// ownership to the claiming consumer. Returns (next_start_id, claimed_entries, deleted_ids).
    /// - next_start_id: (0,0) if all qualifying entries processed, otherwise next unprocessed ID.
    /// - claimed_entries: entries with field data that still exist in the stream.
    /// - deleted_ids: entry IDs that were in PEL but trimmed from the stream.
    pub fn xautoclaim(
        &self,
        key: &Bytes,
        group: &Bytes,
        consumer: Bytes,
        min_idle_time_ms: u64,
        start: StreamId,
        count: Option<usize>,
    ) -> Result<(StreamId, Vec<(StreamId, HashMap<Bytes, Bytes>)>, Vec<StreamId>), StoreError> {
        let mut data = self.data.write();

        // Passive expiration
        if let Some(entry) = data.get(key) {
            if entry.is_expired() {
                data.remove(key);
                return Err(StoreError::NoGroup(
                    String::from_utf8_lossy(group.as_ref()).into_owned(),
                    String::from_utf8_lossy(key.as_ref()).into_owned(),
                ));
            }
        }

        let entry = match data.get_mut(key) {
            None => {
                return Err(StoreError::NoGroup(
                    String::from_utf8_lossy(group.as_ref()).into_owned(),
                    String::from_utf8_lossy(key.as_ref()).into_owned(),
                ));
            }
            Some(e) => e,
        };

        let stream = match entry.data {
            ValueData::Stream(ref mut s) => s,
            _ => return Err(StoreError::WrongType),
        };

        let cg = match stream.groups.get_mut(group) {
            None => {
                return Err(StoreError::NoGroup(
                    String::from_utf8_lossy(group.as_ref()).into_owned(),
                    String::from_utf8_lossy(key.as_ref()).into_owned(),
                ));
            }
            Some(g) => g,
        };

        let now = Instant::now();
        let min_idle = Duration::from_millis(min_idle_time_ms);

        // Scan ALL consumers' PELs for qualifying entries (idle >= min_idle_time AND id >= start)
        let mut qualifying: Vec<(StreamId, u64, Bytes)> = Vec::new(); // (id, delivery_count, original_consumer)
        for (consumer_name, consumer_data) in cg.consumers.iter() {
            for (entry_id, pending_entry) in consumer_data.pending.iter() {
                if *entry_id >= start && now.duration_since(pending_entry.delivery_time) >= min_idle
                {
                    qualifying.push((*entry_id, pending_entry.delivery_count, consumer_name.clone()));
                }
            }
        }

        // Sort by StreamId for deterministic ordering
        qualifying.sort_by_key(|(id, _, _)| *id);

        // Apply count limit
        let max_count = count.unwrap_or(usize::MAX);
        let (to_process, remaining) = if qualifying.len() > max_count {
            (&qualifying[..max_count], Some(&qualifying[max_count..]))
        } else {
            (&qualifying[..], None)
        };

        // Determine next_start_id
        let next_start_id = match remaining {
            Some(rest) if !rest.is_empty() => rest[0].0,
            _ => (0, 0), // All processed, signal completion
        };

        // Process the entries to claim
        let mut claimed_entries: Vec<(StreamId, HashMap<Bytes, Bytes>)> = Vec::new();
        let mut deleted_ids: Vec<StreamId> = Vec::new();

        for (entry_id, old_delivery_count, original_consumer) in to_process {
            // Remove from original consumer's PEL
            if let Some(orig) = cg.consumers.get_mut(original_consumer) {
                orig.pending.remove(entry_id);
            }

            // Check if entry still exists in stream
            if let Some(fields) = stream.entries.get(entry_id) {
                claimed_entries.push((*entry_id, fields.clone()));
            } else {
                deleted_ids.push(*entry_id);
            }

            // Add to claiming consumer's PEL unconditionally, even when the
            // stream entry has been trimmed (deleted_ids path above). This matches
            // Redis 7+ behaviour: XAUTOCLAIM transfers the PEL entry to the new
            // consumer and reports it in the deleted_ids list so the caller can
            // immediately XACK it. Without this, a trimmed entry would remain
            // permanently un-acked if the original consumer disappeared.
            let claiming_consumer = cg
                .consumers
                .entry(consumer.clone())
                .or_insert_with(|| Consumer {
                    pending: HashMap::new(),
                });
            claiming_consumer.pending.insert(
                *entry_id,
                PendingEntry {
                    delivery_time: Instant::now(),
                    delivery_count: old_delivery_count + 1,
                },
            );
        }

        Ok((next_start_id, claimed_entries, deleted_ids))
    }

    /// XCLAIM: Transfer ownership of pending stream entries to a different consumer.
    /// Moves PEL entries from their current consumer to the target consumer.
    /// Only claims entries that have been idle for at least min_idle_time_ms.
    /// If `idle` is Some, resets the entry's idle time to the specified ms value.
    /// If `force` is true, creates the PEL entry even if it doesn't exist in any consumer's PEL.
    /// Returns the claimed entries with their field data (or just IDs if justid is true).
    pub fn xclaim(
        &self,
        key: &Bytes,
        group: &Bytes,
        consumer: Bytes,
        min_idle_time_ms: u64,
        ids: &[StreamId],
        idle: Option<u64>,
        _time: Option<u64>,
        retrycount: Option<u64>,
        force: bool,
        justid: bool,
    ) -> Result<Vec<(StreamId, Option<HashMap<Bytes, Bytes>>)>, StoreError> {
        let mut data = self.data.write();

        // Passive expiration
        if let Some(entry) = data.get(key) {
            if entry.is_expired() {
                data.remove(key);
                return Err(StoreError::NoGroup(
                    String::from_utf8_lossy(group.as_ref()).into_owned(),
                    String::from_utf8_lossy(key.as_ref()).into_owned(),
                ));
            }
        }

        let entry = match data.get_mut(key) {
            None => {
                return Err(StoreError::NoGroup(
                    String::from_utf8_lossy(group.as_ref()).into_owned(),
                    String::from_utf8_lossy(key.as_ref()).into_owned(),
                ));
            }
            Some(e) => e,
        };

        let stream = match entry.data {
            ValueData::Stream(ref mut s) => s,
            _ => return Err(StoreError::WrongType),
        };

        let cg = match stream.groups.get_mut(group) {
            None => {
                return Err(StoreError::NoGroup(
                    String::from_utf8_lossy(group.as_ref()).into_owned(),
                    String::from_utf8_lossy(key.as_ref()).into_owned(),
                ));
            }
            Some(g) => g,
        };

        let now = Instant::now();
        let min_idle = Duration::from_millis(min_idle_time_ms);
        let mut claimed = Vec::new();

        for &id in ids {
            // Find the entry in any consumer's PEL
            let mut found_consumer: Option<Bytes> = None;
            let mut found_entry: Option<PendingEntry> = None;
            for (cname, c) in cg.consumers.iter() {
                if let Some(pe) = c.pending.get(&id) {
                    let idle_duration = now.duration_since(pe.delivery_time);
                    if idle_duration >= min_idle || force {
                        found_consumer = Some(cname.clone());
                        found_entry = Some(pe.clone());
                    }
                    break;
                }
            }

            // If force is set and entry not found in any PEL but exists in stream, create it
            if found_consumer.is_none() && force {
                if stream.entries.contains_key(&id) {
                    let new_delivery_time = match idle {
                        Some(idle_ms) => now - Duration::from_millis(idle_ms),
                        None => now,
                    };
                    let target = cg
                        .consumers
                        .entry(consumer.clone())
                        .or_insert_with(|| Consumer {
                            pending: HashMap::new(),
                        });
                    target.pending.insert(
                        id,
                        PendingEntry {
                            delivery_time: new_delivery_time,
                            delivery_count: retrycount.unwrap_or(1),
                        },
                    );
                    if justid {
                        claimed.push((id, None));
                    } else if let Some(fields) = stream.entries.get(&id) {
                        claimed.push((id, Some(fields.clone())));
                    }
                }
                continue;
            }

            if let (Some(from_consumer), Some(pe)) = (found_consumer, found_entry) {
                // Remove from source consumer's PEL
                if let Some(orig) = cg.consumers.get_mut(&from_consumer) {
                    orig.pending.remove(&id);
                }

                // Determine new delivery time based on idle parameter
                let new_delivery_time = match idle {
                    Some(idle_ms) => now - Duration::from_millis(idle_ms),
                    None => pe.delivery_time, // Keep original
                };
                let new_delivery_count = retrycount.unwrap_or(pe.delivery_count + 1);

                // Add to target consumer's PEL
                let target = cg
                    .consumers
                    .entry(consumer.clone())
                    .or_insert_with(|| Consumer {
                        pending: HashMap::new(),
                    });
                target.pending.insert(
                    id,
                    PendingEntry {
                        delivery_time: new_delivery_time,
                        delivery_count: new_delivery_count,
                    },
                );

                // Return the entry data
                if justid {
                    claimed.push((id, None));
                } else if let Some(fields) = stream.entries.get(&id) {
                    claimed.push((id, Some(fields.clone())));
                }
            }
        }

        Ok(claimed)
    }

    /// XINFO GROUPS: Returns metadata about all consumer groups on a stream.
    /// Each group entry contains: name, consumers count, pending count, last-delivered-id.
    pub fn xinfo_groups(&self, key: &Bytes) -> Result<Vec<HashMap<String, String>>, StoreError> {
        let mut data = self.data.write();

        // Passive expiration
        if let Some(entry) = data.get(key) {
            if entry.is_expired() {
                data.remove(key);
                return Ok(Vec::new());
            }
        }

        let entry = match data.get(key) {
            None => return Ok(Vec::new()),
            Some(e) => e,
        };

        let stream = match &entry.data {
            ValueData::Stream(s) => s,
            _ => return Err(StoreError::WrongType),
        };

        let mut result = Vec::new();
        for (group_name, group) in &stream.groups {
            let mut info = HashMap::new();
            info.insert(
                "name".to_string(),
                String::from_utf8_lossy(group_name.as_ref()).into_owned(),
            );
            info.insert("consumers".to_string(), group.consumers.len().to_string());
            let total_pending: usize = group.consumers.values().map(|c| c.pending.len()).sum();
            info.insert("pending".to_string(), total_pending.to_string());
            info.insert(
                "last-delivered-id".to_string(),
                crate::commands::streams::format_stream_id(group.last_delivered_id),
            );
            result.push(info);
        }

        Ok(result)
    }

    /// XINFO STREAM: Returns a snapshot of stream-level metadata: length,
    /// last-generated-id, group count, and the first/last entries.
    /// Returns Ok(None) if the key does not exist (or is expired).
    /// Returns Err(WrongType) if the key holds a non-stream value.
    pub fn xinfo_stream(&self, key: &Bytes) -> Result<Option<XInfoStreamSnapshot>, StoreError> {
        let mut data = self.data.write();

        // Passive expiration
        if let Some(entry) = data.get(key) {
            if entry.is_expired() {
                data.remove(key);
                return Ok(None);
            }
        }

        let entry = match data.get(key) {
            None => return Ok(None),
            Some(e) => e,
        };

        let stream = match &entry.data {
            ValueData::Stream(s) => s,
            _ => return Err(StoreError::WrongType),
        };

        let first = stream
            .entries
            .iter()
            .next()
            .map(|(id, fields)| (*id, fields.clone()));
        let last = stream
            .entries
            .iter()
            .next_back()
            .map(|(id, fields)| (*id, fields.clone()));

        Ok(Some(XInfoStreamSnapshot {
            length: stream.entries.len(),
            last_id: stream.last_id,
            groups_count: stream.groups.len(),
            first_entry: first,
            last_entry: last,
        }))
    }

    /// XINFO CONSUMERS: Returns metadata about all consumers in a specific consumer group.
    /// Each consumer entry contains: name, pending count, idle time (ms since last delivery).
    pub fn xinfo_consumers(
        &self,
        key: &Bytes,
        group: &Bytes,
    ) -> Result<Vec<HashMap<String, String>>, StoreError> {
        let mut data = self.data.write();

        // Passive expiration
        if let Some(entry) = data.get(key) {
            if entry.is_expired() {
                data.remove(key);
                return Err(StoreError::NoGroup(
                    String::from_utf8_lossy(group.as_ref()).into_owned(),
                    String::from_utf8_lossy(key.as_ref()).into_owned(),
                ));
            }
        }

        let entry = match data.get(key) {
            None => {
                return Err(StoreError::NoGroup(
                    String::from_utf8_lossy(group.as_ref()).into_owned(),
                    String::from_utf8_lossy(key.as_ref()).into_owned(),
                ));
            }
            Some(e) => e,
        };

        let stream = match &entry.data {
            ValueData::Stream(s) => s,
            _ => return Err(StoreError::WrongType),
        };

        let cg = match stream.groups.get(group) {
            None => {
                return Err(StoreError::NoGroup(
                    String::from_utf8_lossy(group.as_ref()).into_owned(),
                    String::from_utf8_lossy(key.as_ref()).into_owned(),
                ));
            }
            Some(g) => g,
        };

        let now = Instant::now();
        let mut result = Vec::new();
        for (consumer_name, consumer_data) in &cg.consumers {
            let mut info = HashMap::new();
            info.insert(
                "name".to_string(),
                String::from_utf8_lossy(consumer_name.as_ref()).into_owned(),
            );
            info.insert("pending".to_string(), consumer_data.pending.len().to_string());

            // Idle: ms since most recent delivery_time in this consumer's PEL
            let idle_ms = if consumer_data.pending.is_empty() {
                0u128
            } else {
                consumer_data
                    .pending
                    .values()
                    .map(|pe| now.duration_since(pe.delivery_time).as_millis())
                    .min()
                    .unwrap_or(0)
            };
            info.insert("idle".to_string(), idle_ms.to_string());
            result.push(info);
        }

        Ok(result)
    }

    /// XPENDING RANGE: Returns detailed pending entry information with filtering.
    /// Each result contains (entry_id, consumer_name, idle_time_ms, delivery_count).
    pub fn xpending_range(
        &self,
        key: &Bytes,
        group: &Bytes,
        min_id: StreamId,
        max_id: StreamId,
        count: usize,
        consumer_filter: Option<&Bytes>,
        min_idle_ms: Option<u64>,
    ) -> Result<Vec<(StreamId, Bytes, u128, u64)>, StoreError> {
        let mut data = self.data.write();

        // Passive expiration
        if let Some(entry) = data.get(key) {
            if entry.is_expired() {
                data.remove(key);
                return Err(StoreError::NoGroup(
                    String::from_utf8_lossy(group.as_ref()).into_owned(),
                    String::from_utf8_lossy(key.as_ref()).into_owned(),
                ));
            }
        }

        let entry = match data.get(key) {
            None => {
                return Err(StoreError::NoGroup(
                    String::from_utf8_lossy(group.as_ref()).into_owned(),
                    String::from_utf8_lossy(key.as_ref()).into_owned(),
                ));
            }
            Some(e) => e,
        };

        let stream = match &entry.data {
            ValueData::Stream(s) => s,
            _ => return Err(StoreError::WrongType),
        };

        let cg = match stream.groups.get(group) {
            None => {
                return Err(StoreError::NoGroup(
                    String::from_utf8_lossy(group.as_ref()).into_owned(),
                    String::from_utf8_lossy(key.as_ref()).into_owned(),
                ));
            }
            Some(g) => g,
        };

        let now = Instant::now();
        let mut results: Vec<(StreamId, Bytes, u128, u64)> = Vec::new();

        // Iterate consumers: either the filtered one or all
        let consumers_to_check: Vec<(&Bytes, &Consumer)> = match consumer_filter {
            Some(name) => {
                if let Some(consumer) = cg.consumers.get(name) {
                    vec![(name, consumer)]
                } else {
                    vec![]
                }
            }
            None => cg.consumers.iter().collect(),
        };

        for (consumer_name, consumer_data) in consumers_to_check {
            for (entry_id, pe) in &consumer_data.pending {
                // Filter by ID range
                if *entry_id < min_id || *entry_id > max_id {
                    continue;
                }

                let idle_ms = now.duration_since(pe.delivery_time).as_millis();

                // Filter by minimum idle time
                if let Some(min_idle) = min_idle_ms {
                    if idle_ms < min_idle as u128 {
                        continue;
                    }
                }

                results.push((*entry_id, consumer_name.clone(), idle_ms, pe.delivery_count));
            }
        }

        // Sort by StreamId ascending
        results.sort_by_key(|(id, _, _, _)| *id);

        // Truncate to count
        results.truncate(count);

        Ok(results)
    }

    /// XPENDING summary form: Returns aggregated pending message info for a consumer group.
    /// Returns (total_pending, min_id, max_id, per_consumer_counts).
    pub fn xpending_summary(
        &self,
        key: &Bytes,
        group: &Bytes,
    ) -> Result<(usize, Option<StreamId>, Option<StreamId>, Vec<(Bytes, usize)>), StoreError> {
        let mut data = self.data.write();

        // Passive expiration
        if let Some(entry) = data.get(key) {
            if entry.is_expired() {
                data.remove(key);
                return Err(StoreError::NoGroup(
                    String::from_utf8_lossy(group.as_ref()).into_owned(),
                    String::from_utf8_lossy(key.as_ref()).into_owned(),
                ));
            }
        }

        let entry = match data.get(key) {
            None => return Err(StoreError::NoGroup(
                String::from_utf8_lossy(group.as_ref()).into_owned(),
                String::from_utf8_lossy(key.as_ref()).into_owned(),
            )),
            Some(e) => e,
        };

        let stream = match &entry.data {
            ValueData::Stream(s) => s,
            _ => return Err(StoreError::WrongType),
        };

        let cg = match stream.groups.get(group) {
            None => return Err(StoreError::NoGroup(
                String::from_utf8_lossy(group.as_ref()).into_owned(),
                String::from_utf8_lossy(key.as_ref()).into_owned(),
            )),
            Some(g) => g,
        };

        let mut total_pending: usize = 0;
        let mut min_id: Option<StreamId> = None;
        let mut max_id: Option<StreamId> = None;
        let mut consumer_counts: Vec<(Bytes, usize)> = Vec::new();

        for (consumer_name, consumer) in &cg.consumers {
            let count = consumer.pending.len();
            if count > 0 {
                consumer_counts.push((consumer_name.clone(), count));
                total_pending += count;
                for (entry_id, _) in &consumer.pending {
                    match min_id {
                        None => min_id = Some(*entry_id),
                        Some(m) if *entry_id < m => min_id = Some(*entry_id),
                        _ => {}
                    }
                    match max_id {
                        None => max_id = Some(*entry_id),
                        Some(m) if *entry_id > m => max_id = Some(*entry_id),
                        _ => {}
                    }
                }
            }
        }

        Ok((total_pending, min_id, max_id, consumer_counts))
    }

    // ── Lua Scripting Operations ───────────────────────��─────────────

    /// SCRIPT LOAD: Cache a Lua script by its SHA1 hash. Returns the SHA1 hex digest.
    pub fn script_load(&self, script: &str) -> String {
        let sha1 = LuaEngine::sha1_hex(script);
        self.scripts.write().insert(sha1.clone(), script.to_string());
        sha1
    }

    /// SCRIPT EXISTS: Check if scripts are cached by SHA1 hash.
    /// Returns a Vec<bool> indicating presence for each input SHA.
    pub fn script_exists(&self, shas: &[String]) -> Vec<bool> {
        let scripts = self.scripts.read();
        shas.iter().map(|sha| scripts.contains_key(sha)).collect()
    }

    /// EVAL: Execute a Lua script atomically. Auto-caches the script.
    /// Holds write lock on data for entire script duration (atomicity guarantee).
    pub fn eval(&self, script: &str, keys: Vec<Bytes>, args: Vec<Bytes>) -> Result<RedisValue, String> {
        // Auto-cache the script
        let sha1 = LuaEngine::sha1_hex(script);
        self.scripts.write().insert(sha1, script.to_string());

        // Clone broadcast sender BEFORE acquiring data write lock (deadlock prevention)
        let pubsub_tx = self.pubsub_sender();

        let (result, had_xadd) = {
            // Acquire write lock on data -- held for entire script execution
            let mut data = self.data.write();
            LuaEngine::execute(script, keys, args, &mut *data, Some(&pubsub_tx))?
            // data write lock drops here
        };
        if had_xadd {
            self.stream_notify.notify_waiters();
        }
        Ok(result)
    }

    /// EVALSHA: Execute a cached Lua script by SHA1 hash.
    /// Returns NOSCRIPT error if the SHA is not in the cache.
    /// Holds write lock on data for entire script duration (atomicity guarantee).
    pub fn evalsha(&self, sha: &str, keys: Vec<Bytes>, args: Vec<Bytes>) -> Result<RedisValue, String> {
        // Look up script in cache (acquire and release scripts lock before data lock)
        let script = {
            let scripts = self.scripts.read();
            match scripts.get(sha) {
                Some(s) => s.clone(),
                None => return Err("NOSCRIPT No matching script. Use EVAL.".to_string()),
            }
        };

        // Clone broadcast sender BEFORE acquiring data write lock (deadlock prevention)
        let pubsub_tx = self.pubsub_sender();

        let (result, had_xadd) = {
            // Acquire write lock on data -- held for entire script execution
            let mut data = self.data.write();
            LuaEngine::execute(&script, keys, args, &mut *data, Some(&pubsub_tx))?
            // data write lock drops here
        };
        if had_xadd {
            self.stream_notify.notify_waiters();
        }
        Ok(result)
    }

    // -- Pub/Sub Methods --

    /// Get a clone of the broadcast sender for use in contexts that cannot hold the pubsub lock
    /// (e.g., Lua script dispatch which holds the data write lock).
    pub fn pubsub_sender(&self) -> broadcast::Sender<PubSubMessage> {
        self.pubsub.read().tx.clone()
    }

    /// Create a new subscriber, returning (subscriber_id, broadcast::Receiver).
    pub fn new_subscriber(&self) -> (u64, broadcast::Receiver<PubSubMessage>) {
        let registry = self.pubsub.read();
        let id = registry.next_id.fetch_add(1, Ordering::Relaxed);
        let rx = registry.tx.subscribe();
        (id, rx)
    }

    /// SUBSCRIBE: Register a subscriber for exact channels.
    /// Returns Vec<(channel_name, total_subscription_count_for_this_subscriber)>.
    pub fn subscribe(&self, subscriber_id: u64, channels: Vec<Bytes>) -> Vec<(Bytes, i64)> {
        let mut registry = self.pubsub.write();
        let mut results = Vec::new();
        for channel in channels {
            registry.channel_subscribers
                .entry(channel.clone())
                .or_default()
                .insert(subscriber_id);
            registry.subscriber_channels
                .entry(subscriber_id)
                .or_default()
                .insert(channel.clone());
            let total = registry.subscriber_channels.get(&subscriber_id).map(|c| c.len()).unwrap_or(0)
                + registry.subscriber_patterns.get(&subscriber_id).map(|p| p.len()).unwrap_or(0);
            results.push((channel, total as i64));
        }
        results
    }

    /// UNSUBSCRIBE: Remove a subscriber from exact channels.
    /// If channels is empty, unsubscribe from ALL channels.
    /// Returns Vec<(channel_name, remaining_subscription_count)>.
    pub fn unsubscribe(&self, subscriber_id: u64, channels: Vec<Bytes>) -> Vec<(Bytes, i64)> {
        let mut registry = self.pubsub.write();
        let channels_to_remove = if channels.is_empty() {
            registry.subscriber_channels.get(&subscriber_id)
                .cloned().unwrap_or_default().into_iter().collect::<Vec<_>>()
        } else {
            channels
        };
        let mut results = Vec::new();
        for channel in channels_to_remove {
            if let Some(subs) = registry.channel_subscribers.get_mut(&channel) {
                subs.remove(&subscriber_id);
                if subs.is_empty() {
                    registry.channel_subscribers.remove(&channel);
                }
            }
            if let Some(chans) = registry.subscriber_channels.get_mut(&subscriber_id) {
                chans.remove(&channel);
            }
            let total = registry.subscriber_channels.get(&subscriber_id).map(|c| c.len()).unwrap_or(0)
                + registry.subscriber_patterns.get(&subscriber_id).map(|p| p.len()).unwrap_or(0);
            results.push((channel, total as i64));
        }
        // Clean up empty subscriber_channels entry to prevent unbounded growth
        if registry.subscriber_channels
            .get(&subscriber_id)
            .map(|s| s.is_empty())
            .unwrap_or(true)
        {
            registry.subscriber_channels.remove(&subscriber_id);
        }
        results
    }

    /// PSUBSCRIBE: Register a subscriber for glob patterns.
    /// Returns Vec<(pattern, total_subscription_count)>.
    pub fn psubscribe(&self, subscriber_id: u64, patterns: Vec<Bytes>) -> Vec<(Bytes, i64)> {
        let mut registry = self.pubsub.write();
        let mut results = Vec::new();
        for pattern in patterns {
            registry.pattern_subscribers
                .entry(pattern.clone())
                .or_default()
                .insert(subscriber_id);
            registry.subscriber_patterns
                .entry(subscriber_id)
                .or_default()
                .insert(pattern.clone());
            let total = registry.subscriber_channels.get(&subscriber_id).map(|c| c.len()).unwrap_or(0)
                + registry.subscriber_patterns.get(&subscriber_id).map(|p| p.len()).unwrap_or(0);
            results.push((pattern, total as i64));
        }
        results
    }

    /// PUNSUBSCRIBE: Remove a subscriber from glob patterns.
    /// If patterns is empty, unsubscribe from ALL patterns.
    /// Returns Vec<(pattern, remaining_subscription_count)>.
    pub fn punsubscribe(&self, subscriber_id: u64, patterns: Vec<Bytes>) -> Vec<(Bytes, i64)> {
        let mut registry = self.pubsub.write();
        let patterns_to_remove = if patterns.is_empty() {
            registry.subscriber_patterns.get(&subscriber_id)
                .cloned().unwrap_or_default().into_iter().collect::<Vec<_>>()
        } else {
            patterns
        };
        let mut results = Vec::new();
        for pattern in patterns_to_remove {
            if let Some(subs) = registry.pattern_subscribers.get_mut(&pattern) {
                subs.remove(&subscriber_id);
                if subs.is_empty() {
                    registry.pattern_subscribers.remove(&pattern);
                }
            }
            if let Some(pats) = registry.subscriber_patterns.get_mut(&subscriber_id) {
                pats.remove(&pattern);
            }
            let total = registry.subscriber_channels.get(&subscriber_id).map(|c| c.len()).unwrap_or(0)
                + registry.subscriber_patterns.get(&subscriber_id).map(|p| p.len()).unwrap_or(0);
            results.push((pattern, total as i64));
        }
        // Clean up empty subscriber_patterns entry to prevent unbounded growth
        if registry.subscriber_patterns
            .get(&subscriber_id)
            .map(|s| s.is_empty())
            .unwrap_or(true)
        {
            registry.subscriber_patterns.remove(&subscriber_id);
        }
        results
    }

    /// PUBLISH: Send a message to a channel. Returns total subscriber count that will receive it.
    /// Sends both "message" (for exact subscribers) and "pmessage" (for pattern subscribers).
    pub fn publish(&self, channel: Bytes, message: Bytes) -> i64 {
        let registry = self.pubsub.read();

        // Count exact channel subscribers
        let channel_count = registry.channel_subscribers
            .get(&channel)
            .map(|s| s.len() as i64)
            .unwrap_or(0);

        // Count and send to pattern subscribers
        let mut pattern_count: i64 = 0;
        for (pattern, subs) in &registry.pattern_subscribers {
            if crate::commands::pubsub::glob_match(pattern, &channel) {
                pattern_count += subs.len() as i64;
                // Send pmessage for each matching pattern
                let _ = registry.tx.send(PubSubMessage {
                    kind: "pmessage".to_string(),
                    pattern: Some(pattern.clone()),
                    channel: channel.clone(),
                    data: message.clone(),
                });
            }
        }

        // Send regular "message" event only when there are exact-channel subscribers
        if channel_count > 0 {
            let _ = registry.tx.send(PubSubMessage {
                kind: "message".to_string(),
                pattern: None,
                channel: channel.clone(),
                data: message.clone(),
            });
        }

        channel_count + pattern_count
    }

    /// PUBSUB CHANNELS: Return channels with active subscriptions matching the optional glob pattern.
    pub fn pubsub_channels(&self, pattern: Option<&Bytes>) -> Vec<Bytes> {
        let registry = self.pubsub.read();
        let mut channels: Vec<Bytes> = registry.channel_subscribers.keys()
            .filter(|ch| {
                match pattern {
                    Some(pat) => crate::commands::pubsub::glob_match(pat, ch),
                    None => true,
                }
            })
            .cloned()
            .collect();
        channels.sort();
        channels
    }

    /// PUBSUB NUMSUB: Return (channel, subscriber_count) for each requested channel.
    pub fn pubsub_numsub(&self, channels: Vec<Bytes>) -> Vec<(Bytes, i64)> {
        let registry = self.pubsub.read();
        channels.into_iter()
            .map(|ch| {
                let count = registry.channel_subscribers.get(&ch)
                    .map(|s| s.len() as i64)
                    .unwrap_or(0);
                (ch, count)
            })
            .collect()
    }

    /// PUBSUB NUMPAT: Return the total number of active pattern subscriptions.
    pub fn pubsub_numpat(&self) -> i64 {
        let registry = self.pubsub.read();
        registry.pattern_subscribers.values()
            .map(|s| s.len() as i64)
            .sum()
    }
}

// ── Persistable Snapshot Types ──────────────────────────────────────────
// These mirror the runtime Store types but use serde-friendly primitives:
// - Vec<u8> instead of Bytes (which has no native Serialize/Deserialize)
// - Option<u64> (ms remaining) instead of Option<Instant> (non-serializable)
// - u64 delivery_count only instead of PendingEntry with Instant

/// Persistable version of the entire store snapshot.
#[derive(Serialize, Deserialize)]
pub struct PersistableStore {
    pub entries: Vec<(Vec<u8>, PersistableEntry)>,
    pub scripts: Vec<(String, String)>,
}

/// Persistable version of a ValueEntry.
#[derive(Serialize, Deserialize)]
pub struct PersistableEntry {
    pub data: PersistableValueData,
    /// TTL remaining in milliseconds. None = no expiry.
    pub ttl_remaining_ms: Option<u64>,
}

/// Persistable version of ValueData.
#[derive(Serialize, Deserialize)]
pub enum PersistableValueData {
    String(Vec<u8>),
    Hash(Vec<(Vec<u8>, Vec<u8>)>),
    Set(Vec<Vec<u8>>),
    SortedSet(PersistableSortedSet),
    Stream(PersistableStream),
}

/// Persistable version of SortedSet.
#[derive(Serialize, Deserialize)]
pub struct PersistableSortedSet {
    /// (score, member) pairs
    pub members: Vec<(f64, Vec<u8>)>,
}

/// Persistable version of Stream.
#[derive(Serialize, Deserialize)]
pub struct PersistableStream {
    /// Stream entries: (id, fields) where id is (ms, seq) and fields are (key, value) pairs
    pub entries: Vec<((u64, u64), Vec<(Vec<u8>, Vec<u8>)>)>,
    pub last_id: (u64, u64),
    pub groups: Vec<(Vec<u8>, PersistableConsumerGroup)>,
}

/// Persistable version of ConsumerGroup.
#[derive(Serialize, Deserialize)]
pub struct PersistableConsumerGroup {
    pub last_delivered_id: (u64, u64),
    pub consumers: Vec<(Vec<u8>, PersistableConsumer)>,
}

/// Persistable version of Consumer.
#[derive(Serialize, Deserialize)]
pub struct PersistableConsumer {
    /// Pending entries: (stream_id, delivery_count). delivery_time is reset to now() on load.
    pub pending: Vec<((u64, u64), u64)>,
}

// ── Snapshot Conversion: Runtime -> Persistable ─────────────────────────

impl PersistableStore {
    /// Create a persistable snapshot from the runtime Store.
    /// Filters out expired entries. Acquires read locks.
    pub fn from_store(store: &Store) -> Self {
        let data = store.data.read();
        let scripts = store.scripts.read();

        let now = Instant::now();
        let entries: Vec<(Vec<u8>, PersistableEntry)> = data
            .iter()
            .filter(|(_, entry)| !entry.is_expired())
            .map(|(key, entry)| {
                let ttl_remaining_ms = entry.expires_at.and_then(|exp| {
                    if exp > now {
                        Some(exp.duration_since(now).as_millis() as u64)
                    } else {
                        None // already expired edge case
                    }
                });

                let pdata = match &entry.data {
                    ValueData::String(b) => PersistableValueData::String(b.to_vec()),
                    ValueData::Hash(map) => PersistableValueData::Hash(
                        map.iter()
                            .map(|(k, v)| (k.to_vec(), v.to_vec()))
                            .collect(),
                    ),
                    ValueData::Set(set) => {
                        PersistableValueData::Set(set.iter().map(|m| m.to_vec()).collect())
                    }
                    ValueData::SortedSet(ss) => {
                        PersistableValueData::SortedSet(PersistableSortedSet {
                            members: ss
                                .by_member
                                .iter()
                                .map(|(m, &s)| (s, m.to_vec()))
                                .collect(),
                        })
                    }
                    ValueData::Stream(stream) => {
                        PersistableValueData::Stream(PersistableStream {
                            entries: stream
                                .entries
                                .iter()
                                .map(|(&id, fields)| {
                                    let f: Vec<(Vec<u8>, Vec<u8>)> = fields
                                        .iter()
                                        .map(|(k, v)| (k.to_vec(), v.to_vec()))
                                        .collect();
                                    (id, f)
                                })
                                .collect(),
                            last_id: stream.last_id,
                            groups: stream
                                .groups
                                .iter()
                                .map(|(name, group)| {
                                    (
                                        name.to_vec(),
                                        PersistableConsumerGroup {
                                            last_delivered_id: group.last_delivered_id,
                                            consumers: group
                                                .consumers
                                                .iter()
                                                .map(|(cname, consumer)| {
                                                    (
                                                        cname.to_vec(),
                                                        PersistableConsumer {
                                                            pending: consumer
                                                                .pending
                                                                .iter()
                                                                .map(|(&sid, pe)| {
                                                                    (sid, pe.delivery_count)
                                                                })
                                                                .collect(),
                                                        },
                                                    )
                                                })
                                                .collect(),
                                        },
                                    )
                                })
                                .collect(),
                        })
                    }
                };

                (key.to_vec(), PersistableEntry {
                    data: pdata,
                    ttl_remaining_ms,
                })
            })
            .collect();

        let scripts_vec: Vec<(String, String)> = scripts
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        PersistableStore {
            entries,
            scripts: scripts_vec,
        }
    }

    /// Restore runtime data from a persistable snapshot.
    /// Returns (data_map, scripts_map) ready to be loaded into a Store.
    pub fn into_runtime(self) -> (HashMap<Bytes, ValueEntry>, HashMap<String, String>) {
        let now = Instant::now();

        let mut data = HashMap::new();
        for (key_bytes, pentry) in self.entries {
            let expires_at = pentry.ttl_remaining_ms.and_then(|ms| {
                if ms == 0 {
                    None // already expired at save time edge case
                } else {
                    Some(now + Duration::from_millis(ms))
                }
            });

            let vdata = match pentry.data {
                PersistableValueData::String(b) => ValueData::String(Bytes::from(b)),
                PersistableValueData::Hash(pairs) => {
                    let map: HashMap<Bytes, Bytes> = pairs
                        .into_iter()
                        .map(|(k, v)| (Bytes::from(k), Bytes::from(v)))
                        .collect();
                    ValueData::Hash(map)
                }
                PersistableValueData::Set(members) => {
                    let set: HashSet<Bytes> =
                        members.into_iter().map(Bytes::from).collect();
                    ValueData::Set(set)
                }
                PersistableValueData::SortedSet(pss) => {
                    let mut ss = SortedSet::new();
                    for (score, member_bytes) in pss.members {
                        let member = Bytes::from(member_bytes);
                        ss.by_score
                            .insert((OrderedFloat(score), member.clone()), ());
                        ss.by_member.insert(member, score);
                    }
                    ValueData::SortedSet(ss)
                }
                PersistableValueData::Stream(pstream) => {
                    let entries: BTreeMap<StreamId, HashMap<Bytes, Bytes>> = pstream
                        .entries
                        .into_iter()
                        .map(|(id, fields)| {
                            let fmap: HashMap<Bytes, Bytes> = fields
                                .into_iter()
                                .map(|(k, v)| (Bytes::from(k), Bytes::from(v)))
                                .collect();
                            (id, fmap)
                        })
                        .collect();

                    let groups: HashMap<Bytes, ConsumerGroup> = pstream
                        .groups
                        .into_iter()
                        .map(|(name, pgroup)| {
                            let consumers: HashMap<Bytes, Consumer> = pgroup
                                .consumers
                                .into_iter()
                                .map(|(cname, pconsumer)| {
                                    let pending: HashMap<StreamId, PendingEntry> = pconsumer
                                        .pending
                                        .into_iter()
                                        .map(|(sid, count)| {
                                            (
                                                sid,
                                                PendingEntry {
                                                    delivery_time: now, // reset to now on load
                                                    delivery_count: count,
                                                },
                                            )
                                        })
                                        .collect();
                                    (Bytes::from(cname), Consumer { pending })
                                })
                                .collect();
                            (
                                Bytes::from(name),
                                ConsumerGroup {
                                    last_delivered_id: pgroup.last_delivered_id,
                                    consumers,
                                },
                            )
                        })
                        .collect();

                    ValueData::Stream(Stream {
                        entries,
                        last_id: pstream.last_id,
                        groups,
                    })
                }
            };

            data.insert(
                Bytes::from(key_bytes),
                ValueEntry {
                    data: vdata,
                    expires_at,
                },
            );
        }

        let scripts: HashMap<String, String> = self.scripts.into_iter().collect();

        (data, scripts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Existing String Tests ────────────────────────────────────────

    #[test]
    fn test_set_and_get() {
        let store = Store::new();
        let key = Bytes::from("key");
        let value = Bytes::from("value");
        assert!(store.set(key.clone(), value.clone(), None, false, false));
        assert_eq!(store.get(&key), Some(value));
    }

    #[test]
    fn test_get_missing_key() {
        let store = Store::new();
        assert_eq!(store.get(&Bytes::from("missing")), None);
    }

    #[test]
    fn test_set_nx_existing_key() {
        let store = Store::new();
        let key = Bytes::from("key");
        store.set(key.clone(), Bytes::from("v1"), None, false, false);
        assert!(!store.set(key.clone(), Bytes::from("v2"), None, true, false));
        assert_eq!(store.get(&key), Some(Bytes::from("v1")));
    }

    #[test]
    fn test_set_xx_missing_key() {
        let store = Store::new();
        let key = Bytes::from("key");
        assert!(!store.set(key.clone(), Bytes::from("v1"), None, false, true));
        assert_eq!(store.get(&key), None);
    }

    #[test]
    fn test_set_with_expired_ttl() {
        let store = Store::new();
        let key = Bytes::from("key");
        store.set(
            key.clone(),
            Bytes::from("v"),
            Some(Duration::from_millis(0)),
            false,
            false,
        );
        std::thread::sleep(Duration::from_millis(1));
        assert_eq!(store.get(&key), None);
    }

    #[test]
    fn test_delete_returns_count() {
        let store = Store::new();
        store.set(Bytes::from("a"), Bytes::from("1"), None, false, false);
        store.set(Bytes::from("b"), Bytes::from("2"), None, false, false);
        let count = store.delete(&[Bytes::from("a"), Bytes::from("b"), Bytes::from("c")]);
        assert_eq!(count, 2);
    }

    #[test]
    fn test_exists_returns_count() {
        let store = Store::new();
        store.set(Bytes::from("a"), Bytes::from("1"), None, false, false);
        store.set(Bytes::from("b"), Bytes::from("2"), None, false, false);
        let count = store.exists(&[Bytes::from("a"), Bytes::from("b"), Bytes::from("c")]);
        assert_eq!(count, 2);
    }

    // ── GET on non-string types returns None ─────────────────────────

    #[test]
    fn test_get_on_hash_key_returns_none() {
        let store = Store::new();
        let key = Bytes::from("myhash");
        store
            .hset(key.clone(), vec![(Bytes::from("f"), Bytes::from("v"))])
            .unwrap();
        // GET on a hash key returns None (WRONGTYPE handling is at Python layer)
        assert_eq!(store.get(&key), None);
    }

    #[test]
    fn test_get_on_set_key_returns_none() {
        let store = Store::new();
        let key = Bytes::from("myset");
        store.sadd(key.clone(), vec![Bytes::from("m")]).unwrap();
        assert_eq!(store.get(&key), None);
    }

    // ── SET overwrites any type ──────────────────────────────────────

    #[test]
    fn test_set_overwrites_hash() {
        let store = Store::new();
        let key = Bytes::from("mykey");
        store
            .hset(key.clone(), vec![(Bytes::from("f"), Bytes::from("v"))])
            .unwrap();
        assert!(store.set(key.clone(), Bytes::from("string_val"), None, false, false));
        assert_eq!(store.get(&key), Some(Bytes::from("string_val")));
    }

    #[test]
    fn test_set_overwrites_set() {
        let store = Store::new();
        let key = Bytes::from("mykey");
        store.sadd(key.clone(), vec![Bytes::from("m")]).unwrap();
        assert!(store.set(key.clone(), Bytes::from("string_val"), None, false, false));
        assert_eq!(store.get(&key), Some(Bytes::from("string_val")));
    }

    // ── Hash Tests ───────────────────────────────────────────────────

    #[test]
    fn test_hset_new_key() {
        let store = Store::new();
        let key = Bytes::from("h1");
        let result = store
            .hset(
                key,
                vec![
                    (Bytes::from("f1"), Bytes::from("v1")),
                    (Bytes::from("f2"), Bytes::from("v2")),
                ],
            )
            .unwrap();
        assert_eq!(result, 2); // 2 new fields
    }

    #[test]
    fn test_hset_existing_fields() {
        let store = Store::new();
        let key = Bytes::from("h1");
        store
            .hset(key.clone(), vec![(Bytes::from("f1"), Bytes::from("v1"))])
            .unwrap();
        // Update f1, add f2
        let result = store
            .hset(
                key,
                vec![
                    (Bytes::from("f1"), Bytes::from("v1_updated")),
                    (Bytes::from("f2"), Bytes::from("v2")),
                ],
            )
            .unwrap();
        assert_eq!(result, 1); // only f2 is new
    }

    #[test]
    fn test_hset_wrongtype() {
        let store = Store::new();
        let key = Bytes::from("string_key");
        store.set(key.clone(), Bytes::from("val"), None, false, false);
        let result = store.hset(key, vec![(Bytes::from("f"), Bytes::from("v"))]);
        assert!(matches!(result, Err(StoreError::WrongType)));
    }

    #[test]
    fn test_hget_existing() {
        let store = Store::new();
        let key = Bytes::from("h1");
        store
            .hset(key.clone(), vec![(Bytes::from("f1"), Bytes::from("v1"))])
            .unwrap();
        let val = store.hget(&key, &Bytes::from("f1")).unwrap();
        assert_eq!(val, Some(Bytes::from("v1")));
    }

    #[test]
    fn test_hget_missing_field() {
        let store = Store::new();
        let key = Bytes::from("h1");
        store
            .hset(key.clone(), vec![(Bytes::from("f1"), Bytes::from("v1"))])
            .unwrap();
        let val = store.hget(&key, &Bytes::from("f_missing")).unwrap();
        assert_eq!(val, None);
    }

    #[test]
    fn test_hget_missing_key() {
        let store = Store::new();
        let val = store.hget(&Bytes::from("no_such_key"), &Bytes::from("f")).unwrap();
        assert_eq!(val, None);
    }

    #[test]
    fn test_hget_wrongtype() {
        let store = Store::new();
        let key = Bytes::from("string_key");
        store.set(key.clone(), Bytes::from("val"), None, false, false);
        let result = store.hget(&key, &Bytes::from("f"));
        assert!(matches!(result, Err(StoreError::WrongType)));
    }

    #[test]
    fn test_hdel_existing_fields() {
        let store = Store::new();
        let key = Bytes::from("h1");
        store
            .hset(
                key.clone(),
                vec![
                    (Bytes::from("f1"), Bytes::from("v1")),
                    (Bytes::from("f2"), Bytes::from("v2")),
                    (Bytes::from("f3"), Bytes::from("v3")),
                ],
            )
            .unwrap();
        let count = store
            .hdel(&key, &[Bytes::from("f1"), Bytes::from("f3"), Bytes::from("f_missing")])
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_hdel_missing_key() {
        let store = Store::new();
        let count = store
            .hdel(&Bytes::from("no_key"), &[Bytes::from("f")])
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_hdel_wrongtype() {
        let store = Store::new();
        let key = Bytes::from("string_key");
        store.set(key.clone(), Bytes::from("val"), None, false, false);
        let result = store.hdel(&key, &[Bytes::from("f")]);
        assert!(matches!(result, Err(StoreError::WrongType)));
    }

    #[test]
    fn test_hvals_existing() {
        let store = Store::new();
        let key = Bytes::from("h1");
        store
            .hset(
                key.clone(),
                vec![
                    (Bytes::from("f1"), Bytes::from("v1")),
                    (Bytes::from("f2"), Bytes::from("v2")),
                ],
            )
            .unwrap();
        let mut vals = store.hvals(&key).unwrap();
        vals.sort(); // HashMap ordering is non-deterministic
        assert_eq!(vals, vec![Bytes::from("v1"), Bytes::from("v2")]);
    }

    #[test]
    fn test_hvals_empty_key() {
        let store = Store::new();
        let vals = store.hvals(&Bytes::from("no_key")).unwrap();
        assert!(vals.is_empty());
    }

    // ── Set Tests ────────────────────────────────────────────────────

    #[test]
    fn test_sadd_new_key() {
        let store = Store::new();
        let key = Bytes::from("s1");
        let count = store
            .sadd(key, vec![Bytes::from("a"), Bytes::from("b"), Bytes::from("c")])
            .unwrap();
        assert_eq!(count, 3);
    }

    #[test]
    fn test_sadd_existing_members() {
        let store = Store::new();
        let key = Bytes::from("s1");
        store
            .sadd(key.clone(), vec![Bytes::from("a"), Bytes::from("b")])
            .unwrap();
        // Add 'b' again and 'c' new
        let count = store
            .sadd(key, vec![Bytes::from("b"), Bytes::from("c")])
            .unwrap();
        assert_eq!(count, 1); // only 'c' is new
    }

    #[test]
    fn test_sadd_wrongtype() {
        let store = Store::new();
        let key = Bytes::from("string_key");
        store.set(key.clone(), Bytes::from("val"), None, false, false);
        let result = store.sadd(key, vec![Bytes::from("m")]);
        assert!(matches!(result, Err(StoreError::WrongType)));
    }

    #[test]
    fn test_smembers_existing() {
        let store = Store::new();
        let key = Bytes::from("s1");
        store
            .sadd(key.clone(), vec![Bytes::from("a"), Bytes::from("b")])
            .unwrap();
        let mut members = store.smembers(&key).unwrap();
        members.sort();
        assert_eq!(members, vec![Bytes::from("a"), Bytes::from("b")]);
    }

    #[test]
    fn test_smembers_missing_key() {
        let store = Store::new();
        let members = store.smembers(&Bytes::from("no_key")).unwrap();
        assert!(members.is_empty());
    }

    #[test]
    fn test_sismember_true() {
        let store = Store::new();
        let key = Bytes::from("s1");
        store.sadd(key.clone(), vec![Bytes::from("a")]).unwrap();
        assert!(store.sismember(&key, &Bytes::from("a")).unwrap());
    }

    #[test]
    fn test_sismember_false() {
        let store = Store::new();
        let key = Bytes::from("s1");
        store.sadd(key.clone(), vec![Bytes::from("a")]).unwrap();
        assert!(!store.sismember(&key, &Bytes::from("b")).unwrap());
    }

    #[test]
    fn test_sismember_missing_key() {
        let store = Store::new();
        assert!(!store.sismember(&Bytes::from("no_key"), &Bytes::from("a")).unwrap());
    }

    #[test]
    fn test_sismember_wrongtype() {
        let store = Store::new();
        let key = Bytes::from("string_key");
        store.set(key.clone(), Bytes::from("val"), None, false, false);
        let result = store.sismember(&key, &Bytes::from("m"));
        assert!(matches!(result, Err(StoreError::WrongType)));
    }

    #[test]
    fn test_srem_existing() {
        let store = Store::new();
        let key = Bytes::from("s1");
        store
            .sadd(key.clone(), vec![Bytes::from("a"), Bytes::from("b"), Bytes::from("c")])
            .unwrap();
        let count = store
            .srem(&key, &[Bytes::from("a"), Bytes::from("c"), Bytes::from("z")])
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_srem_missing_key() {
        let store = Store::new();
        let count = store
            .srem(&Bytes::from("no_key"), &[Bytes::from("a")])
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_srem_wrongtype() {
        let store = Store::new();
        let key = Bytes::from("string_key");
        store.set(key.clone(), Bytes::from("val"), None, false, false);
        let result = store.srem(&key, &[Bytes::from("m")]);
        assert!(matches!(result, Err(StoreError::WrongType)));
    }

    // ── Passive Expiration Tests for Hash/Set ────────────────────────

    #[test]
    fn test_hset_on_expired_key_creates_new_hash() {
        let store = Store::new();
        let key = Bytes::from("h_expired");
        // Insert a string with immediate expiry
        store.set(
            key.clone(),
            Bytes::from("old"),
            Some(Duration::from_millis(0)),
            false,
            false,
        );
        std::thread::sleep(Duration::from_millis(1));
        // hset should treat the expired key as non-existent and create a hash
        let count = store
            .hset(key.clone(), vec![(Bytes::from("f"), Bytes::from("v"))])
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(
            store.hget(&key, &Bytes::from("f")).unwrap(),
            Some(Bytes::from("v"))
        );
    }

    #[test]
    fn test_sadd_on_expired_key_creates_new_set() {
        let store = Store::new();
        let key = Bytes::from("s_expired");
        store.set(
            key.clone(),
            Bytes::from("old"),
            Some(Duration::from_millis(0)),
            false,
            false,
        );
        std::thread::sleep(Duration::from_millis(1));
        let count = store.sadd(key.clone(), vec![Bytes::from("m")]).unwrap();
        assert_eq!(count, 1);
        assert!(store.sismember(&key, &Bytes::from("m")).unwrap());
    }

    // ── Sorted Set Tests (ZADD) ─────────────────────────────────────

    #[test]
    fn test_zadd_new_members() {
        let store = Store::new();
        let key = Bytes::from("zs1");
        let count = store
            .zadd(
                key,
                vec![
                    (1.0, Bytes::from("a")),
                    (2.0, Bytes::from("b")),
                    (3.0, Bytes::from("c")),
                ],
                false, false, false, false, false,
            )
            .unwrap();
        assert_eq!(count, 3);
    }

    #[test]
    fn test_zadd_update_existing_score() {
        let store = Store::new();
        let key = Bytes::from("zs1");
        store
            .zadd(key.clone(), vec![(1.0, Bytes::from("a"))], false, false, false, false, false)
            .unwrap();
        // Update score for existing member -- returns 0 (not new)
        let count = store
            .zadd(key, vec![(5.0, Bytes::from("a"))], false, false, false, false, false)
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_zadd_nx_flag() {
        let store = Store::new();
        let key = Bytes::from("zs1");
        store
            .zadd(key.clone(), vec![(1.0, Bytes::from("a"))], false, false, false, false, false)
            .unwrap();
        // NX: only add new members, skip existing
        let count = store
            .zadd(
                key.clone(),
                vec![(5.0, Bytes::from("a")), (2.0, Bytes::from("b"))],
                true, false, false, false, false,
            )
            .unwrap();
        assert_eq!(count, 1); // only 'b' is new
        // 'a' should still have score 1.0
        let result = store.zrangebyscore(&key, 1.0, 1.0, true).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, Bytes::from("a"));
        assert_eq!(result[0].1, Some(1.0));
    }

    #[test]
    fn test_zadd_xx_flag() {
        let store = Store::new();
        let key = Bytes::from("zs1");
        store
            .zadd(key.clone(), vec![(1.0, Bytes::from("a"))], false, false, false, false, false)
            .unwrap();
        // XX: only update existing, skip new
        let count = store
            .zadd(
                key.clone(),
                vec![(5.0, Bytes::from("a")), (2.0, Bytes::from("b"))],
                false, true, false, false, false,
            )
            .unwrap();
        assert_eq!(count, 0); // 'b' is new but skipped due to XX, 'a' updated but not new
        // 'a' should have score 5.0 now
        let result = store.zrangebyscore(&key, 5.0, 5.0, true).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, Bytes::from("a"));
        // 'b' should not exist
        let all = store.zrange(&key, 0, -1, false).unwrap();
        assert_eq!(all.len(), 1);
    }

    #[test]
    fn test_zadd_gt_flag() {
        let store = Store::new();
        let key = Bytes::from("zs1");
        store
            .zadd(key.clone(), vec![(5.0, Bytes::from("a"))], false, false, false, false, false)
            .unwrap();
        // GT: only update if new score > old
        let count = store
            .zadd(key.clone(), vec![(3.0, Bytes::from("a"))], false, false, true, false, false)
            .unwrap();
        assert_eq!(count, 0); // 3.0 < 5.0, not updated
        // Score should still be 5.0
        let result = store.zrangebyscore(&key, 5.0, 5.0, true).unwrap();
        assert_eq!(result.len(), 1);

        // Now update with a higher score
        let count = store
            .zadd(key.clone(), vec![(10.0, Bytes::from("a"))], false, false, true, false, true)
            .unwrap();
        assert_eq!(count, 1); // changed (using CH flag)
        let result = store.zrangebyscore(&key, 10.0, 10.0, true).unwrap();
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_zadd_lt_flag() {
        let store = Store::new();
        let key = Bytes::from("zs1");
        store
            .zadd(key.clone(), vec![(5.0, Bytes::from("a"))], false, false, false, false, false)
            .unwrap();
        // LT: only update if new score < old
        let count = store
            .zadd(key.clone(), vec![(10.0, Bytes::from("a"))], false, false, false, true, false)
            .unwrap();
        assert_eq!(count, 0); // 10.0 > 5.0, not updated
        // Score should still be 5.0
        let result = store.zrangebyscore(&key, 5.0, 5.0, true).unwrap();
        assert_eq!(result.len(), 1);

        // Now update with a lower score
        let count = store
            .zadd(key.clone(), vec![(2.0, Bytes::from("a"))], false, false, false, true, true)
            .unwrap();
        assert_eq!(count, 1); // changed (using CH flag)
        let result = store.zrangebyscore(&key, 2.0, 2.0, true).unwrap();
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_zadd_ch_flag() {
        let store = Store::new();
        let key = Bytes::from("zs1");
        store
            .zadd(key.clone(), vec![(1.0, Bytes::from("a"))], false, false, false, false, false)
            .unwrap();
        // CH: returns count of changed (new + updated) instead of just new
        let count = store
            .zadd(
                key,
                vec![(5.0, Bytes::from("a")), (2.0, Bytes::from("b"))],
                false, false, false, false, true,
            )
            .unwrap();
        assert_eq!(count, 2); // 'a' updated + 'b' added = 2 changed
    }

    #[test]
    fn test_zadd_wrongtype() {
        let store = Store::new();
        let key = Bytes::from("string_key");
        store.set(key.clone(), Bytes::from("val"), None, false, false);
        let result = store.zadd(key, vec![(1.0, Bytes::from("a"))], false, false, false, false, false);
        assert!(matches!(result, Err(StoreError::WrongType)));
    }

    // ── Sorted Set Tests (ZREM) ─────────────────────────────────────

    #[test]
    fn test_zrem_existing_members() {
        let store = Store::new();
        let key = Bytes::from("zs1");
        store
            .zadd(
                key.clone(),
                vec![
                    (1.0, Bytes::from("a")),
                    (2.0, Bytes::from("b")),
                    (3.0, Bytes::from("c")),
                ],
                false, false, false, false, false,
            )
            .unwrap();
        let count = store
            .zrem(&key, &[Bytes::from("a"), Bytes::from("c")])
            .unwrap();
        assert_eq!(count, 2);
        // Only 'b' remains
        let result = store.zrange(&key, 0, -1, false).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, Bytes::from("b"));
    }

    #[test]
    fn test_zrem_missing_members() {
        let store = Store::new();
        let key = Bytes::from("zs1");
        store
            .zadd(key.clone(), vec![(1.0, Bytes::from("a"))], false, false, false, false, false)
            .unwrap();
        let count = store
            .zrem(&key, &[Bytes::from("x"), Bytes::from("y")])
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_zrem_missing_key() {
        let store = Store::new();
        let count = store
            .zrem(&Bytes::from("no_key"), &[Bytes::from("a")])
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_zrem_wrongtype() {
        let store = Store::new();
        let key = Bytes::from("string_key");
        store.set(key.clone(), Bytes::from("val"), None, false, false);
        let result = store.zrem(&key, &[Bytes::from("a")]);
        assert!(matches!(result, Err(StoreError::WrongType)));
    }

    // ── Sorted Set Tests (ZRANGE) ───────────────────────────────────

    #[test]
    fn test_zrange_full_range() {
        let store = Store::new();
        let key = Bytes::from("zs1");
        store
            .zadd(
                key.clone(),
                vec![
                    (3.0, Bytes::from("c")),
                    (1.0, Bytes::from("a")),
                    (2.0, Bytes::from("b")),
                ],
                false, false, false, false, false,
            )
            .unwrap();
        let result = store.zrange(&key, 0, -1, false).unwrap();
        assert_eq!(result.len(), 3);
        // Should be in score order: a(1), b(2), c(3)
        assert_eq!(result[0].0, Bytes::from("a"));
        assert_eq!(result[1].0, Bytes::from("b"));
        assert_eq!(result[2].0, Bytes::from("c"));
    }

    #[test]
    fn test_zrange_subset() {
        let store = Store::new();
        let key = Bytes::from("zs1");
        store
            .zadd(
                key.clone(),
                vec![
                    (1.0, Bytes::from("a")),
                    (2.0, Bytes::from("b")),
                    (3.0, Bytes::from("c")),
                    (4.0, Bytes::from("d")),
                ],
                false, false, false, false, false,
            )
            .unwrap();
        let result = store.zrange(&key, 1, 2, false).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, Bytes::from("b"));
        assert_eq!(result[1].0, Bytes::from("c"));
    }

    #[test]
    fn test_zrange_negative_indices() {
        let store = Store::new();
        let key = Bytes::from("zs1");
        store
            .zadd(
                key.clone(),
                vec![
                    (1.0, Bytes::from("a")),
                    (2.0, Bytes::from("b")),
                    (3.0, Bytes::from("c")),
                ],
                false, false, false, false, false,
            )
            .unwrap();
        // -2 to -1 = last two elements
        let result = store.zrange(&key, -2, -1, false).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, Bytes::from("b"));
        assert_eq!(result[1].0, Bytes::from("c"));
    }

    #[test]
    fn test_zrange_withscores() {
        let store = Store::new();
        let key = Bytes::from("zs1");
        store
            .zadd(
                key.clone(),
                vec![
                    (1.5, Bytes::from("a")),
                    (2.5, Bytes::from("b")),
                ],
                false, false, false, false, false,
            )
            .unwrap();
        let result = store.zrange(&key, 0, -1, true).unwrap();
        assert_eq!(result[0], (Bytes::from("a"), Some(1.5)));
        assert_eq!(result[1], (Bytes::from("b"), Some(2.5)));
    }

    #[test]
    fn test_zrange_empty_key() {
        let store = Store::new();
        let result = store.zrange(&Bytes::from("no_key"), 0, -1, false).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_zrange_out_of_bounds() {
        let store = Store::new();
        let key = Bytes::from("zs1");
        store
            .zadd(key.clone(), vec![(1.0, Bytes::from("a"))], false, false, false, false, false)
            .unwrap();
        // start > stop
        let result = store.zrange(&key, 5, 3, false).unwrap();
        assert!(result.is_empty());
        // start >= len
        let result = store.zrange(&key, 10, 20, false).unwrap();
        assert!(result.is_empty());
    }

    // ── Sorted Set Tests (ZRANGEBYSCORE) ────────────────────────────

    #[test]
    fn test_zrangebyscore_range() {
        let store = Store::new();
        let key = Bytes::from("zs1");
        store
            .zadd(
                key.clone(),
                vec![
                    (1.0, Bytes::from("a")),
                    (2.0, Bytes::from("b")),
                    (3.0, Bytes::from("c")),
                    (4.0, Bytes::from("d")),
                ],
                false, false, false, false, false,
            )
            .unwrap();
        let result = store.zrangebyscore(&key, 2.0, 3.0, false).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, Bytes::from("b"));
        assert_eq!(result[1].0, Bytes::from("c"));
    }

    #[test]
    fn test_zrangebyscore_inf() {
        let store = Store::new();
        let key = Bytes::from("zs1");
        store
            .zadd(
                key.clone(),
                vec![
                    (1.0, Bytes::from("a")),
                    (2.0, Bytes::from("b")),
                    (3.0, Bytes::from("c")),
                ],
                false, false, false, false, false,
            )
            .unwrap();
        // -inf to +inf returns all
        let result = store
            .zrangebyscore(&key, f64::NEG_INFINITY, f64::INFINITY, false)
            .unwrap();
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_zrangebyscore_withscores() {
        let store = Store::new();
        let key = Bytes::from("zs1");
        store
            .zadd(
                key.clone(),
                vec![
                    (1.0, Bytes::from("a")),
                    (2.0, Bytes::from("b")),
                ],
                false, false, false, false, false,
            )
            .unwrap();
        let result = store.zrangebyscore(&key, 1.0, 2.0, true).unwrap();
        assert_eq!(result[0], (Bytes::from("a"), Some(1.0)));
        assert_eq!(result[1], (Bytes::from("b"), Some(2.0)));
    }

    #[test]
    fn test_zrangebyscore_empty() {
        let store = Store::new();
        let key = Bytes::from("zs1");
        store
            .zadd(
                key.clone(),
                vec![(1.0, Bytes::from("a")), (2.0, Bytes::from("b"))],
                false, false, false, false, false,
            )
            .unwrap();
        let result = store.zrangebyscore(&key, 5.0, 10.0, false).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_zrangebyscore_missing_key() {
        let store = Store::new();
        let result = store
            .zrangebyscore(&Bytes::from("no_key"), 0.0, 100.0, false)
            .unwrap();
        assert!(result.is_empty());
    }

    // ── Sorted Set Tests (ZRANGESTORE) ──────────────────────────────

    #[test]
    fn test_zrangestore_basic() {
        let store = Store::new();
        let src = Bytes::from("src");
        let dst = Bytes::from("dst");
        store
            .zadd(
                src.clone(),
                vec![
                    (1.0, Bytes::from("a")),
                    (2.0, Bytes::from("b")),
                    (3.0, Bytes::from("c")),
                    (4.0, Bytes::from("d")),
                ],
                false, false, false, false, false,
            )
            .unwrap();
        let count = store.zrangestore(dst.clone(), &src, 2.0, 3.0).unwrap();
        assert_eq!(count, 2);
        // Verify destination has the correct members
        let result = store.zrange(&dst, 0, -1, true).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], (Bytes::from("b"), Some(2.0)));
        assert_eq!(result[1], (Bytes::from("c"), Some(3.0)));
    }

    #[test]
    fn test_zrangestore_empty_range() {
        let store = Store::new();
        let src = Bytes::from("src");
        let dst = Bytes::from("dst");
        store
            .zadd(
                src.clone(),
                vec![(1.0, Bytes::from("a"))],
                false, false, false, false, false,
            )
            .unwrap();
        let count = store.zrangestore(dst.clone(), &src, 5.0, 10.0).unwrap();
        assert_eq!(count, 0);
        // Destination should not exist
        let result = store.zrange(&dst, 0, -1, false).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_zrangestore_missing_src() {
        let store = Store::new();
        let count = store
            .zrangestore(Bytes::from("dst"), &Bytes::from("no_src"), 0.0, 100.0)
            .unwrap();
        assert_eq!(count, 0);
    }

    // ── Sorted Set Tests (ZREMRANGEBYSCORE) ─────────────────────────

    #[test]
    fn test_zremrangebyscore_basic() {
        let store = Store::new();
        let key = Bytes::from("zs1");
        store
            .zadd(
                key.clone(),
                vec![
                    (1.0, Bytes::from("a")),
                    (2.0, Bytes::from("b")),
                    (3.0, Bytes::from("c")),
                    (4.0, Bytes::from("d")),
                ],
                false, false, false, false, false,
            )
            .unwrap();
        let count = store.zremrangebyscore(&key, 2.0, 3.0).unwrap();
        assert_eq!(count, 2);
        // Only 'a' and 'd' should remain
        let result = store.zrange(&key, 0, -1, false).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, Bytes::from("a"));
        assert_eq!(result[1].0, Bytes::from("d"));
    }

    #[test]
    fn test_zremrangebyscore_no_matches() {
        let store = Store::new();
        let key = Bytes::from("zs1");
        store
            .zadd(
                key.clone(),
                vec![(1.0, Bytes::from("a")), (2.0, Bytes::from("b"))],
                false, false, false, false, false,
            )
            .unwrap();
        let count = store.zremrangebyscore(&key, 5.0, 10.0).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_zremrangebyscore_missing_key() {
        let store = Store::new();
        let count = store
            .zremrangebyscore(&Bytes::from("no_key"), 0.0, 100.0)
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_zremrangebyscore_wrongtype() {
        let store = Store::new();
        let key = Bytes::from("string_key");
        store.set(key.clone(), Bytes::from("val"), None, false, false);
        let result = store.zremrangebyscore(&key, 0.0, 10.0);
        assert!(matches!(result, Err(StoreError::WrongType)));
    }

    // ── GET on SortedSet returns None ───────────────────────────────

    #[test]
    fn test_get_on_sorted_set_returns_none() {
        let store = Store::new();
        let key = Bytes::from("myzset");
        store
            .zadd(key.clone(), vec![(1.0, Bytes::from("a"))], false, false, false, false, false)
            .unwrap();
        assert_eq!(store.get(&key), None);
    }

    // ── Passive Expiration for Sorted Sets ──────────────────────────

    #[test]
    fn test_zadd_on_expired_key_creates_new_sorted_set() {
        let store = Store::new();
        let key = Bytes::from("zs_expired");
        store.set(
            key.clone(),
            Bytes::from("old"),
            Some(Duration::from_millis(0)),
            false,
            false,
        );
        std::thread::sleep(Duration::from_millis(1));
        let count = store
            .zadd(key.clone(), vec![(1.0, Bytes::from("a"))], false, false, false, false, false)
            .unwrap();
        assert_eq!(count, 1);
        let result = store.zrange(&key, 0, -1, false).unwrap();
        assert_eq!(result.len(), 1);
    }

    // ── Sweep Expiration Tests ──────────────────────────────────────

    #[test]
    fn test_sweep_expired() {
        let store = Store::new();
        // Set 3 keys: 2 with very short TTL (already expired by the time we sweep), 1 with no TTL
        store.set(
            Bytes::from("exp1"), Bytes::from("v1"),
            Some(Duration::from_millis(1)), false, false,
        );
        store.set(
            Bytes::from("exp2"), Bytes::from("v2"),
            Some(Duration::from_millis(1)), false, false,
        );
        store.set(
            Bytes::from("persist"), Bytes::from("v3"),
            None, false, false,
        );

        // Wait for expiry
        std::thread::sleep(Duration::from_millis(10));

        let removed = store.sweep_expired();
        assert_eq!(removed, 2);

        // Persistent key still exists
        assert_eq!(store.get(&Bytes::from("persist")), Some(Bytes::from("v3")));
        // Expired keys are gone
        assert_eq!(store.get(&Bytes::from("exp1")), None);
        assert_eq!(store.get(&Bytes::from("exp2")), None);
    }

    #[test]
    fn test_sweep_max_20_keys() {
        let store = Store::new();
        // Create 30 keys with very short TTL
        for i in 0..30 {
            store.set(
                Bytes::from(format!("key{}", i)),
                Bytes::from("val"),
                Some(Duration::from_millis(1)),
                false,
                false,
            );
        }

        std::thread::sleep(Duration::from_millis(10));

        // First sweep should remove at most 20
        let removed = store.sweep_expired();
        assert!(removed <= 20, "sweep should remove at most 20 keys, removed {}", removed);
        assert!(removed > 0, "sweep should remove some expired keys");
    }

    // ── keys() tests ────────────────────────────────────────────────

    #[test]
    fn test_keys_star_returns_all() {
        let store = Store::new();
        store.set(Bytes::from("a"), Bytes::from("1"), None, false, false);
        store.set(Bytes::from("b"), Bytes::from("2"), None, false, false);
        store.set(Bytes::from("c"), Bytes::from("3"), None, false, false);
        let mut keys = store.keys(b"*");
        keys.sort();
        assert_eq!(keys, vec![Bytes::from("a"), Bytes::from("b"), Bytes::from("c")]);
    }

    #[test]
    fn test_keys_prefix_pattern() {
        let store = Store::new();
        store.set(Bytes::from("user:1"), Bytes::from("a"), None, false, false);
        store.set(Bytes::from("user:2"), Bytes::from("b"), None, false, false);
        store.set(Bytes::from("session:1"), Bytes::from("c"), None, false, false);
        let mut keys = store.keys(b"user:*");
        keys.sort();
        assert_eq!(keys, vec![Bytes::from("user:1"), Bytes::from("user:2")]);
    }

    #[test]
    fn test_keys_no_match() {
        let store = Store::new();
        store.set(Bytes::from("a"), Bytes::from("1"), None, false, false);
        let keys = store.keys(b"nonexistent*");
        assert!(keys.is_empty());
    }

    #[test]
    fn test_keys_excludes_expired() {
        let store = Store::new();
        store.set(Bytes::from("alive"), Bytes::from("1"), None, false, false);
        store.set(Bytes::from("dead"), Bytes::from("2"), Some(Duration::from_millis(1)), false, false);
        std::thread::sleep(Duration::from_millis(10));
        let keys = store.keys(b"*");
        assert_eq!(keys, vec![Bytes::from("alive")]);
    }

    // ── ttl() tests ─────────────────────────────────────────────────

    #[test]
    fn test_ttl_missing_key() {
        let store = Store::new();
        assert_eq!(store.ttl(&Bytes::from("missing")), -2);
    }

    #[test]
    fn test_ttl_no_expiry() {
        let store = Store::new();
        store.set(Bytes::from("k"), Bytes::from("v"), None, false, false);
        assert_eq!(store.ttl(&Bytes::from("k")), -1);
    }

    #[test]
    fn test_ttl_with_expiry() {
        let store = Store::new();
        store.set(Bytes::from("k"), Bytes::from("v"), Some(Duration::from_secs(30)), false, false);
        let ttl = store.ttl(&Bytes::from("k"));
        assert!(ttl > 0 && ttl <= 30, "expected positive TTL, got {}", ttl);
    }

    #[test]
    fn test_ttl_expired_key() {
        let store = Store::new();
        store.set(Bytes::from("k"), Bytes::from("v"), Some(Duration::from_millis(1)), false, false);
        std::thread::sleep(Duration::from_millis(10));
        assert_eq!(store.ttl(&Bytes::from("k")), -2);
    }

    // ── mget() tests ────────────────────────────────────────────────

    #[test]
    fn test_mget_mixed() {
        let store = Store::new();
        store.set(Bytes::from("k1"), Bytes::from("v1"), None, false, false);
        // k2 missing
        store.set(Bytes::from("k3"), Bytes::from("v3"), None, false, false);
        let result = store.mget(&[Bytes::from("k1"), Bytes::from("k2"), Bytes::from("k3")]);
        assert_eq!(result, vec![Some(Bytes::from("v1")), None, Some(Bytes::from("v3"))]);
    }

    #[test]
    fn test_mget_non_string_returns_none() {
        let store = Store::new();
        store.set(Bytes::from("str"), Bytes::from("v"), None, false, false);
        // Create a hash key
        store.hset(Bytes::from("hash"), vec![(Bytes::from("f"), Bytes::from("v"))]);
        let result = store.mget(&[Bytes::from("str"), Bytes::from("hash")]);
        assert_eq!(result, vec![Some(Bytes::from("v")), None]);
    }

    // ── xpending_summary() tests ────────────────────────────────────

    #[test]
    fn test_xpending_summary_no_pending() {
        let store = Store::new();
        // Create stream and group
        let mut fields = HashMap::new();
        fields.insert(Bytes::from("f"), Bytes::from("v"));
        store.xadd(Bytes::from("s"), fields, None).unwrap();
        store.xgroup_create(&Bytes::from("s"), Bytes::from("g"), (0, 0), false).unwrap();

        let (total, min, max, consumers) = store.xpending_summary(&Bytes::from("s"), &Bytes::from("g")).unwrap();
        assert_eq!(total, 0);
        assert_eq!(min, None);
        assert_eq!(max, None);
        assert!(consumers.is_empty());
    }

    #[test]
    fn test_xpending_summary_with_pending() {
        let store = Store::new();
        // Create stream entries
        let mut fields1 = HashMap::new();
        fields1.insert(Bytes::from("f"), Bytes::from("v1"));
        let mut fields2 = HashMap::new();
        fields2.insert(Bytes::from("f"), Bytes::from("v2"));
        store.xadd(Bytes::from("s"), fields1, None).unwrap();
        store.xadd(Bytes::from("s"), fields2, None).unwrap();
        store.xgroup_create(&Bytes::from("s"), Bytes::from("g"), (0, 0), false).unwrap();

        // Read messages (creates pending entries)
        store.xreadgroup(
            &Bytes::from("g"),
            &Bytes::from("consumer1"),
            &[Bytes::from("s")],
            &[">".to_string()],
            Some(10),
        ).unwrap();

        let (total, min, max, consumers) = store.xpending_summary(&Bytes::from("s"), &Bytes::from("g")).unwrap();
        assert_eq!(total, 2);
        assert!(min.is_some());
        assert!(max.is_some());
        assert_eq!(consumers.len(), 1);
        assert_eq!(consumers[0].0, Bytes::from("consumer1"));
        assert_eq!(consumers[0].1, 2);
    }
}
