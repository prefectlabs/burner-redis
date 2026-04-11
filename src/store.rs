use bytes::Bytes;
use ordered_float::OrderedFloat;
use parking_lot::RwLock;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Bound;
use std::time::{Duration, Instant};

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
}

pub struct Store {
    data: RwLock<HashMap<Bytes, ValueEntry>>,
}

impl Store {
    pub fn new() -> Self {
        Store {
            data: RwLock::new(HashMap::new()),
        }
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
}
