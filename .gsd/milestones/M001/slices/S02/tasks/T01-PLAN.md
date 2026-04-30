# T01: Extend the Store engine to support Hash and Set data types with WRONGTYPE error handling.

**Slice:** S02 — **Milestone:** M001

## Description

Extend the Store engine to support Hash and Set data types with WRONGTYPE error handling.

Purpose: The in-memory store currently only holds string (Bytes) values. This plan adds HashMap<Bytes, Bytes> and HashSet<Bytes> as value types with proper type discrimination, enabling hash and set commands in Plan 02.

Output: Extended Store with multi-type ValueEntry enum, WRONGTYPE error variant, and hash/set operation methods with Rust unit tests.

## Legacy Source

---
phase: 02-hash-and-set-commands
plan: 01
type: execute
wave: 1
depends_on: []
files_modified:
  - src/store.rs
  - src/commands/mod.rs
  - src/commands/hashes.rs
  - src/commands/sets.rs
autonomous: true
requirements: [HASH-01, HASH-02, HASH-03, HASH-04, SET-01, SET-02, SET-03, SET-04]

must_haves:
  truths:
    - "Store can hold Hash values (HashMap<Bytes, Bytes>) alongside String values"
    - "Store can hold Set values (HashSet<Bytes>) alongside String and Hash values"
    - "Operations on wrong-type keys return a WRONGTYPE error instead of corrupting data"
    - "Hash operations (hset/hget/hdel/hvals) work correctly at the engine level"
    - "Set operations (sadd/smembers/sismember/srem) work correctly at the engine level"
  artifacts:
    - path: "src/store.rs"
      provides: "Multi-type ValueEntry enum with Hash and Set variants, type-checking methods"
      contains: "ValueType::Hash"
    - path: "src/commands/hashes.rs"
      provides: "Hash command helper functions or Store methods for hset/hget/hdel/hvals"
      min_lines: 30
    - path: "src/commands/sets.rs"
      provides: "Set command helper functions or Store methods for sadd/smembers/sismember/srem"
      min_lines: 30
  key_links:
    - from: "src/store.rs"
      to: "src/commands/hashes.rs"
      via: "Store methods for hash operations"
      pattern: "pub fn h(set|get|del|vals)"
    - from: "src/store.rs"
      to: "src/commands/sets.rs"
      via: "Store methods for set operations"
      pattern: "pub fn s(add|members|ismember|rem)"
---

<objective>
Extend the Store engine to support Hash and Set data types with WRONGTYPE error handling.

Purpose: The in-memory store currently only holds string (Bytes) values. This plan adds HashMap<Bytes, Bytes> and HashSet<Bytes> as value types with proper type discrimination, enabling hash and set commands in Plan 02.

Output: Extended Store with multi-type ValueEntry enum, WRONGTYPE error variant, and hash/set operation methods with Rust unit tests.
</objective>

<execution_context>
@$HOME/.claude/get-shit-done/workflows/execute-plan.md
@$HOME/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@.planning/PROJECT.md
@.planning/ROADMAP.md
@.planning/STATE.md
@.planning/phases/01-foundation-and-string-commands/01-01-SUMMARY.md
@.planning/phases/01-foundation-and-string-commands/01-02-SUMMARY.md

<interfaces>
<!-- Key types and contracts from the existing codebase that the executor needs. -->

From src/store.rs:
```rust
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
    pub fn new(data: Bytes, ttl: Option<Duration>) -> Self;
    pub fn is_expired(&self) -> bool;
}

pub struct Store {
    data: RwLock<HashMap<Bytes, ValueEntry>>,
}

impl Store {
    pub fn new() -> Self;
    pub fn get(&self, key: &Bytes) -> Option<Bytes>;
    pub fn set(&self, key: Bytes, value: Bytes, ttl: Option<Duration>, nx: bool, xx: bool) -> bool;
    pub fn delete(&self, keys: &[Bytes]) -> i64;
    pub fn exists(&self, keys: &[Bytes]) -> i64;
}
```

From src/commands/strings.rs:
```rust
pub fn extract_bytes(obj: &Bound<'_, PyAny>) -> PyResult<Bytes>;
pub fn extract_expiry(obj: &Bound<'_, PyAny>, unit_millis: bool) -> PyResult<Duration>;
```

From src/commands/mod.rs:
```rust
pub mod strings;
```

From Cargo.toml dependencies:
```toml
pyo3 = { version = "0.28.3", features = ["extension-module", "abi3-py39"] }
tokio = { version = "1.51", features = ["rt", "time", "sync"] }
parking_lot = "0.12.5"
bytes = "1.11"
thiserror = "2.0"
```
</interfaces>
</context>

<tasks>

<task type="auto">
  <name>Task 1: Extend ValueEntry to support Hash and Set types with WRONGTYPE errors</name>
  <files>src/store.rs</files>
  <read_first>src/store.rs, Cargo.toml</read_first>
  <action>
Refactor `ValueEntry` in src/store.rs to support multiple data types:

1. Create a `ValueData` enum with three variants:
   - `String(Bytes)` — existing string values
   - `Hash(HashMap<Bytes, Bytes>)` — hash field-value maps
   - `Set(HashSet<Bytes>)` — set members

2. Change `ValueEntry` to use the new enum:
   ```rust
   #[derive(Clone, Debug)]
   pub struct ValueEntry {
       pub data: ValueData,
       pub expires_at: Option<Instant>,
   }
   ```

3. Add a `StoreError` enum using thiserror:
   ```rust
   #[derive(Debug, thiserror::Error)]
   pub enum StoreError {
       #[error("WRONGTYPE Operation against a key holding the wrong kind of value")]
       WrongType,
   }
   ```

4. Update `ValueEntry::new` to create String entries (backward compatible):
   ```rust
   pub fn new(data: Bytes, ttl: Option<Duration>) -> Self {
       ValueEntry { data: ValueData::String(data), expires_at: ttl.map(|d| Instant::now() + d) }
   }
   ```

5. Add `ValueEntry::new_hash` and `ValueEntry::new_set` constructors that create Hash/Set entries with no expiration (TTL is applied later via Phase 4):
   ```rust
   pub fn new_hash() -> Self {
       ValueEntry { data: ValueData::Hash(HashMap::new()), expires_at: None }
   }
   pub fn new_set() -> Self {
       ValueEntry { data: ValueData::Set(HashSet::new()), expires_at: None }
   }
   ```

6. Update the existing `Store::get` method to only return values for String-type entries (return None for Hash/Set — matches Redis behavior where GET on a non-string key returns a WRONGTYPE error, but we handle that at the Python layer).

7. Update the existing `Store::set` method to overwrite any existing key regardless of type (SET always overwrites in Redis).

8. Add hash operation methods to Store returning `Result<_, StoreError>`:
   - `pub fn hset(&self, key: Bytes, fields: Vec<(Bytes, Bytes)>) -> Result<i64, StoreError>` — inserts field-value pairs, creates key if missing, returns count of NEW fields. If key exists and is not a Hash, returns Err(StoreError::WrongType).
   - `pub fn hget(&self, key: &Bytes, field: &Bytes) -> Result<Option<Bytes>, StoreError>` — returns field value or None. If key is wrong type, returns Err.
   - `pub fn hdel(&self, key: &Bytes, fields: &[Bytes]) -> Result<i64, StoreError>` — removes fields, returns count deleted. If key is wrong type, returns Err.
   - `pub fn hvals(&self, key: &Bytes) -> Result<Vec<Bytes>, StoreError>` — returns all values. If key is wrong type, returns Err. Missing key returns Ok(empty vec).

9. Add set operation methods to Store returning `Result<_, StoreError>`:
   - `pub fn sadd(&self, key: Bytes, members: Vec<Bytes>) -> Result<i64, StoreError>` — adds members, creates key if missing, returns count of NEW members. If key is wrong type, returns Err.
   - `pub fn smembers(&self, key: &Bytes) -> Result<Vec<Bytes>, StoreError>` — returns all members. Missing key returns Ok(empty vec). Wrong type returns Err.
   - `pub fn sismember(&self, key: &Bytes, member: &Bytes) -> Result<bool, StoreError>` — returns true if member exists. Missing key returns Ok(false). Wrong type returns Err.
   - `pub fn srem(&self, key: &Bytes, members: &[Bytes]) -> Result<i64, StoreError>` — removes members, returns count removed. Wrong type returns Err.

10. For all hash/set methods: check passive expiration first (if key exists but is expired, remove it and treat as missing key).

11. Update existing tests to work with the new ValueData::String variant. Add new Rust unit tests:
    - test_hset_new_key, test_hset_existing_fields, test_hset_wrongtype
    - test_hget_existing, test_hget_missing_field, test_hget_missing_key, test_hget_wrongtype
    - test_hdel_existing_fields, test_hdel_missing_key, test_hdel_wrongtype
    - test_hvals_existing, test_hvals_empty_key
    - test_sadd_new_key, test_sadd_existing_members, test_sadd_wrongtype
    - test_smembers_existing, test_smembers_missing_key
    - test_sismember_true, test_sismember_false, test_sismember_missing_key, test_sismember_wrongtype
    - test_srem_existing, test_srem_missing_key, test_srem_wrongtype

Add `use std::collections::HashSet;` at the top of the file.
  </action>
  <verify>
    <automated>cd /Users/desertaxle/dev/prefectlabs/burner-redis && cargo test 2>&1 | tail -20</automated>
  </verify>
  <acceptance_criteria>
    - grep -q "ValueData" src/store.rs (multi-type enum exists)
    - grep -q "WrongType" src/store.rs (WRONGTYPE error variant exists)
    - grep -q "pub fn hset" src/store.rs (hash set method exists)
    - grep -q "pub fn hget" src/store.rs (hash get method exists)
    - grep -q "pub fn hdel" src/store.rs (hash delete method exists)
    - grep -q "pub fn hvals" src/store.rs (hash values method exists)
    - grep -q "pub fn sadd" src/store.rs (set add method exists)
    - grep -q "pub fn smembers" src/store.rs (set members method exists)
    - grep -q "pub fn sismember" src/store.rs (set ismember method exists)
    - grep -q "pub fn srem" src/store.rs (set remove method exists)
    - grep -q "HashSet" src/store.rs (HashSet imported and used)
    - grep -q "test_hset" src/store.rs (hash tests exist)
    - grep -q "test_sadd" src/store.rs (set tests exist)
    - grep -q "wrongtype" src/store.rs (wrongtype tests exist)
    - cargo test passes with 0 failures
  </acceptance_criteria>
  <done>
    - ValueEntry uses ValueData enum with String, Hash, Set variants
    - StoreError::WrongType defined with thiserror
    - All 8 Store methods (hset/hget/hdel/hvals/sadd/smembers/sismember/srem) implemented with type checking and passive expiration
    - Existing string command tests still pass
    - New unit tests cover hash/set operations and WRONGTYPE errors
    - cargo test passes with all tests green
  </done>
</task>

<task type="auto">
  <name>Task 2: Add hash and set command modules</name>
  <files>src/commands/hashes.rs, src/commands/sets.rs, src/commands/mod.rs</files>
  <read_first>src/commands/strings.rs, src/commands/mod.rs, src/store.rs</read_first>
  <action>
1. Create `src/commands/hashes.rs` with a doc comment explaining it provides hash command helpers for the Python layer. For now it will be a module declaration placeholder (the actual Python method implementations live in lib.rs following the established pattern). Add a comment noting the hash commands implemented: HSET, HGET, HDEL, HVALS.

2. Create `src/commands/sets.rs` with a doc comment explaining it provides set command helpers for the Python layer. Add a comment noting the set commands implemented: SADD, SMEMBERS, SISMEMBER, SREM.

3. Update `src/commands/mod.rs` to declare the new submodules:
   ```rust
   pub mod strings;
   pub mod hashes;
   pub mod sets;
   ```

These modules exist for organizational clarity and to house any future helper functions specific to hash/set argument extraction (following the pattern where `strings.rs` houses `extract_bytes` and `extract_expiry`).
  </action>
  <verify>
    <automated>cd /Users/desertaxle/dev/prefectlabs/burner-redis && cargo check 2>&1 | tail -5</automated>
  </verify>
  <acceptance_criteria>
    - grep -q "pub mod hashes" src/commands/mod.rs
    - grep -q "pub mod sets" src/commands/mod.rs
    - test -f src/commands/hashes.rs (file exists)
    - test -f src/commands/sets.rs (file exists)
    - cargo check succeeds with no errors
  </acceptance_criteria>
  <done>
    - src/commands/hashes.rs exists with hash command documentation
    - src/commands/sets.rs exists with set command documentation
    - src/commands/mod.rs declares both new modules
    - Crate compiles cleanly
  </done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|----------|-------------|
| Python args -> Rust Store | Untrusted Python values cross into Rust via extract_bytes |

## STRIDE Threat Register

| Threat ID | Category | Component | Disposition | Mitigation Plan |
|-----------|----------|-----------|-------------|-----------------|
| T-02-01 | Tampering | Store type checking | mitigate | Every hash/set method checks ValueData variant before operating; returns StoreError::WrongType on mismatch |
| T-02-02 | Denial of Service | Hash/Set unbounded growth | accept | No per-key size limits needed — in-process library, user controls their own data. Phase 5 (streams) will add XTRIM for bounded collections |
</threat_model>

<verification>
- `cargo test` passes with all existing + new tests green
- `cargo check` confirms crate compiles
- ValueData enum has exactly 3 variants: String, Hash, Set
- All hash/set Store methods return Result<_, StoreError> for type safety
</verification>

<success_criteria>
- Store engine supports three value types (String, Hash, Set) with type discrimination
- WRONGTYPE errors prevent cross-type operations
- Hash operations (hset/hget/hdel/hvals) work with correct Redis semantics
- Set operations (sadd/smembers/sismember/srem) work with correct Redis semantics
- All Rust tests pass including existing string tests (no regressions)
</success_criteria>

<output>
After completion, create `.planning/phases/02-hash-and-set-commands/02-01-SUMMARY.md`
</output>
