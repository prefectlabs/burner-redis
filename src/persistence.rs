use std::fs;
use std::io::Write;
use std::path::Path;

use crate::store::{PersistableStore, Store};

/// Errors that can occur during persistence operations.
#[derive(Debug, thiserror::Error)]
pub enum PersistenceError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Serialize(#[from] rmp_serde::encode::Error),
    #[error("Deserialization error: {0}")]
    Deserialize(#[from] rmp_serde::decode::Error),
}

/// Save the store's data to a file using crash-safe write pattern:
/// 1. Serialize data to MessagePack binary format
/// 2. Write to a temporary file ({path}.tmp)
/// 3. fsync the temporary file to ensure data is on disk
/// 4. Rename temp file to target path (atomic on POSIX)
///
/// Expired keys are excluded during serialization.
/// Script cache is included in the serialized output.
pub fn save_to_path(store: &Store, path: &str) -> Result<(), PersistenceError> {
    let snapshot = PersistableStore::from_store(store);
    let serialized = rmp_serde::to_vec(&snapshot)?;

    let tmp_path = format!("{}.tmp", path);

    // Write to temp file
    let write_result = (|| -> Result<(), PersistenceError> {
        let mut file = fs::File::create(&tmp_path)?;
        file.write_all(&serialized)?;
        file.sync_all()?; // fsync to ensure data is on disk
        Ok(())
    })();

    if let Err(e) = write_result {
        // Clean up temp file on write failure
        let _ = fs::remove_file(&tmp_path);
        return Err(e);
    }

    // Atomic rename: temp -> target
    if let Err(e) = fs::rename(&tmp_path, path) {
        // Clean up temp file on rename failure
        let _ = fs::remove_file(&tmp_path);
        return Err(PersistenceError::Io(e));
    }

    Ok(())
}

/// Load store data from a persistence file.
///
/// Returns:
/// - Ok(None) if the file does not exist (normal: fresh start)
/// - Ok(Some(snapshot)) if the file was successfully read and deserialized
/// - Err(...) if the file exists but is corrupt or unreadable
pub fn load_from_path(path: &str) -> Result<Option<PersistableStore>, PersistenceError> {
    let file_path = Path::new(path);

    if !file_path.exists() {
        return Ok(None);
    }

    let data = fs::read(file_path)?;
    let snapshot: PersistableStore = rmp_serde::from_slice(&data)?;

    Ok(Some(snapshot))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use std::collections::HashMap;
    use std::time::Duration;

    /// Test round-trip: create store with data, save, load into new store, verify data matches.
    #[test]
    fn test_round_trip_all_types() {
        let store = Store::new();

        // Add a string
        store.set(
            Bytes::from("str_key"),
            Bytes::from("str_value"),
            None,
            false,
            false,
        );

        // Add a string with TTL (5 seconds -- should survive round-trip)
        store.set(
            Bytes::from("str_ttl"),
            Bytes::from("expires_later"),
            Some(Duration::from_secs(5)),
            false,
            false,
        );

        // Add a hash
        store
            .hset(
                Bytes::from("hash_key"),
                vec![
                    (Bytes::from("field1"), Bytes::from("val1")),
                    (Bytes::from("field2"), Bytes::from("val2")),
                ],
            )
            .unwrap();

        // Add a set
        store
            .sadd(
                Bytes::from("set_key"),
                vec![Bytes::from("a"), Bytes::from("b"), Bytes::from("c")],
            )
            .unwrap();

        // Add a sorted set
        store
            .zadd(
                Bytes::from("zset_key"),
                vec![
                    (1.5, Bytes::from("alice")),
                    (2.5, Bytes::from("bob")),
                    (3.5, Bytes::from("charlie")),
                ],
                false,
                false,
                false,
                false,
                false,
            )
            .unwrap();

        // Add a stream
        {
            let mut fields = HashMap::new();
            fields.insert(Bytes::from("field"), Bytes::from("value1"));
            store
                .xadd(Bytes::from("stream_key"), fields, None)
                .unwrap();
        }

        // Add a list (LIST-01..16 round-trip coverage; closes ISSUE-3 from v0.1.6 audit)
        store
            .rpush(
                Bytes::from("list_key"),
                vec![
                    Bytes::from("alpha"),
                    Bytes::from("bravo"),
                    Bytes::from("charlie"),
                ],
            )
            .unwrap();

        // Load a script into the cache
        let sha = store.script_load("return 1");

        // Save to temp file
        let tmp_dir = std::env::temp_dir();
        let path = tmp_dir.join("burner_redis_test_roundtrip.dat");
        let path_str = path.to_str().unwrap();

        save_to_path(&store, path_str).unwrap();

        // Verify temp file does not exist after successful save
        assert!(
            !Path::new(&format!("{}.tmp", path_str)).exists(),
            "Temp file should not exist after successful save"
        );

        // Verify target file exists
        assert!(path.exists(), "Persistence file should exist");

        // Load into new store
        let new_store = Store::new();
        let loaded = load_from_path(path_str).unwrap();
        assert!(loaded.is_some(), "Should load data from file");

        let snapshot = loaded.unwrap();
        let (data_map, scripts_map) = snapshot.into_runtime();

        // Replace store contents
        {
            let mut data = new_store.data_write();
            *data = data_map;
        }
        {
            let mut scripts = new_store.scripts_write();
            *scripts = scripts_map;
        }

        // Verify string
        assert_eq!(
            new_store.get(&Bytes::from("str_key")),
            Ok(Some(Bytes::from("str_value")))
        );

        // Verify string with TTL still exists (not expired)
        assert_eq!(
            new_store.get(&Bytes::from("str_ttl")),
            Ok(Some(Bytes::from("expires_later")))
        );

        // Verify hash
        assert_eq!(
            new_store
                .hget(&Bytes::from("hash_key"), &Bytes::from("field1"))
                .unwrap(),
            Some(Bytes::from("val1"))
        );
        assert_eq!(
            new_store
                .hget(&Bytes::from("hash_key"), &Bytes::from("field2"))
                .unwrap(),
            Some(Bytes::from("val2"))
        );

        // Verify set
        let members = new_store.smembers(&Bytes::from("set_key")).unwrap();
        assert_eq!(members.len(), 3);

        // Verify sorted set
        let zrange = new_store
            .zrange(&Bytes::from("zset_key"), 0, -1, false)
            .unwrap();
        assert_eq!(zrange.len(), 3);
        assert_eq!(zrange[0].0, Bytes::from("alice"));
        assert_eq!(zrange[1].0, Bytes::from("bob"));
        assert_eq!(zrange[2].0, Bytes::from("charlie"));

        // Verify stream
        let xlen = new_store.xlen(&Bytes::from("stream_key")).unwrap();
        assert_eq!(xlen, 1);

        // Verify list round-tripped with order preserved
        let lrange = new_store
            .lrange(&Bytes::from("list_key"), 0, -1)
            .unwrap();
        assert_eq!(lrange.len(), 3);
        assert_eq!(lrange[0], Bytes::from("alpha"));
        assert_eq!(lrange[1], Bytes::from("bravo"));
        assert_eq!(lrange[2], Bytes::from("charlie"));

        // Verify script cache
        let exists = new_store.script_exists(&[sha]);
        assert_eq!(exists, vec![true]);

        // Cleanup
        let _ = fs::remove_file(path);
    }

    /// Test that loading a missing file returns None (not an error).
    #[test]
    fn test_load_missing_file_returns_none() {
        let result = load_from_path("/tmp/burner_redis_nonexistent_file.dat").unwrap();
        assert!(result.is_none());
    }

    /// Test that loading a corrupt file returns an error.
    #[test]
    fn test_load_corrupt_file_returns_error() {
        let tmp_dir = std::env::temp_dir();
        let path = tmp_dir.join("burner_redis_test_corrupt.dat");
        let path_str = path.to_str().unwrap();

        // Write garbage data
        fs::write(&path, b"this is not valid msgpack data").unwrap();

        let result = load_from_path(path_str);
        assert!(result.is_err(), "Should return error for corrupt file");

        // Cleanup
        let _ = fs::remove_file(path);
    }

    /// Test that saving an empty store works correctly.
    #[test]
    fn test_save_and_load_empty_store() {
        let store = Store::new();
        let tmp_dir = std::env::temp_dir();
        let path = tmp_dir.join("burner_redis_test_empty.dat");
        let path_str = path.to_str().unwrap();

        save_to_path(&store, path_str).unwrap();

        let loaded = load_from_path(path_str).unwrap();
        assert!(loaded.is_some());

        let snapshot = loaded.unwrap();
        assert!(snapshot.entries.is_empty());
        assert!(snapshot.scripts.is_empty());

        // Cleanup
        let _ = fs::remove_file(path);
    }

    /// Test that expired keys are excluded from the save.
    #[test]
    fn test_expired_keys_excluded_from_save() {
        let store = Store::new();

        // Add a key that expires immediately
        store.set(
            Bytes::from("expired_key"),
            Bytes::from("gone"),
            Some(Duration::from_millis(1)),
            false,
            false,
        );

        // Add a key that persists
        store.set(
            Bytes::from("live_key"),
            Bytes::from("here"),
            None,
            false,
            false,
        );

        // Wait for expiration
        std::thread::sleep(Duration::from_millis(10));

        let tmp_dir = std::env::temp_dir();
        let path = tmp_dir.join("burner_redis_test_expired.dat");
        let path_str = path.to_str().unwrap();

        save_to_path(&store, path_str).unwrap();

        let loaded = load_from_path(path_str).unwrap().unwrap();
        let (data_map, _) = loaded.into_runtime();

        // Only the live key should be in the snapshot
        assert_eq!(data_map.len(), 1);
        assert!(data_map.contains_key(&Bytes::from("live_key")));
        assert!(!data_map.contains_key(&Bytes::from("expired_key")));

        // Cleanup
        let _ = fs::remove_file(path);
    }
}
