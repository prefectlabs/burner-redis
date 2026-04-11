# Phase 8: Persistence - Context

**Gathered:** 2026-04-11
**Status:** Ready for planning

<domain>
## Phase Boundary

Implement flush-to-disk persistence using MessagePack (rmp-serde), automatic shutdown persistence via atexit, crash-safe writes with write-then-rename+fsync, and automatic restore on construction.

</domain>

<decisions>
## Implementation Decisions

### Serialization
- Use `rmp-serde` (MessagePack) as specified in CLAUDE.md for compact binary serialization.
- Serialize entire keyspace as a single file. Each ValueEntry serialized with its data type, TTL (as duration-from-now or None), and value.
- Script cache also persisted (HashMap<String, String> of SHA1→source).

### Flush API
- `await client.save(path=None)` — serializes all data to a single file. Default path: `./burner-redis.dat`. Optional path kwarg overrides.
- Synchronous Rust function `Store::save_to_path(path)` does the actual serialization+write.

### Shutdown Persistence
- Python `atexit` handler registered on BurnerRedis creation (only if persistence_path is set).
- Handler calls save synchronously via the Tokio runtime's `block_on`.
- If no persistence_path configured, no atexit handler registered (ephemeral mode).

### Crash-Safe Writes
- Write to temp file (`{path}.tmp`) → fsync the file → rename to target path (atomic on POSIX).
- If rename fails, temp file is cleaned up.
- On Windows, rename is not atomic but best-effort (acceptable for dev use case).

### Restore
- `BurnerRedis(persistence_path="./burner-redis.dat")` constructor kwarg.
- If `persistence_path` is set and file exists on construction, deserialize and load into Store.
- If file missing: start empty (no error).
- If file corrupt/invalid: start empty, log warning to stderr.

### Implementation Notes
- Add `serde` derives to Store's data structures (ValueData, ValueEntry, SortedSet, Stream, ConsumerGroup, etc.).
- Need custom Serialize/Deserialize for `Instant` (convert to Duration from epoch or duration-until-expiry).
- `BurnerRedis.__new__` gains optional `persistence_path: Option<String>` parameter.

### Claude's Discretion
- How to handle `Instant` serialization (relative duration vs absolute timestamp).
- Whether to skip TTL restoration for already-expired keys during load.

</decisions>

<code_context>
## Existing Code Insights

### Reusable Assets
- `src/store.rs` — Store with all data types (String, Hash, Set, SortedSet, Stream).
- `Cargo.toml` — already has `serde` dependency.
- `src/lib.rs` — BurnerRedis constructor, Arc<Store>.

### Integration Points
- `Cargo.toml` needs `rmp-serde` dependency.
- `src/store.rs` needs `#[derive(Serialize, Deserialize)]` on all data structures.
- New `src/persistence.rs` module for save/load logic.
- `src/lib.rs` BurnerRedis constructor needs `persistence_path` param and restore logic.
- New `tests/test_persistence.py` file.

</code_context>

<specifics>
## Specific Ideas

No specific requirements — follow CLAUDE.md's rmp-serde recommendation.

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope.

</deferred>
