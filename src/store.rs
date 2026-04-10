use bytes::Bytes;
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// Represents the different data types a Redis key can hold.
#[derive(Clone, Debug)]
pub enum ValueData {
    /// Redis string value (raw bytes).
    String(Bytes),
    /// Redis hash value (field -> value mapping).
    Hash(HashMap<Bytes, Bytes>),
    /// Redis set value (unordered collection of unique members).
    Set(HashSet<Bytes>),
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
}
