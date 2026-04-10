use bytes::Bytes;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct ValueEntry {
    pub data: Bytes,
    pub expires_at: Option<Instant>,
}

impl ValueEntry {
    pub fn new(data: Bytes, ttl: Option<Duration>) -> Self {
        let expires_at = ttl.map(|d| Instant::now() + d);
        ValueEntry { data, expires_at }
    }

    pub fn is_expired(&self) -> bool {
        self.expires_at
            .map(|exp| Instant::now() >= exp)
            .unwrap_or(false)
    }
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

    /// GET: Returns the value for a key, or None if missing/expired.
    /// Passive expiration: removes expired keys on access.
    pub fn get(&self, key: &Bytes) -> Option<Bytes> {
        // First check with a read lock
        {
            let data = self.data.read();
            match data.get(key) {
                None => return None,
                Some(entry) if !entry.is_expired() => return Some(entry.data.clone()),
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
        store.set(key.clone(), Bytes::from("v"), Some(Duration::from_millis(0)), false, false);
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
}
