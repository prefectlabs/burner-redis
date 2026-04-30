# T01: Extend the Store engine to support sorted set data type with dual-index pattern and all 6 sorted set operations.

**Slice:** S03 — **Milestone:** M001

## Description

Extend the Store engine to support sorted set data type with dual-index pattern and all 6 sorted set operations.

Purpose: The in-memory store needs a SortedSet value type using BTreeMap<(OrderedFloat<f64>, Bytes), ()> for score-ordered range queries plus HashMap<Bytes, f64> for O(1) member-to-score lookup. This enables ZADD (with NX/XX/GT/LT/CH flags), ZREM, ZRANGE, ZRANGEBYSCORE, ZRANGESTORE, and ZREMRANGEBYSCORE at the Rust engine level.

Output: Extended Store with SortedSet ValueData variant, ordered-float dependency, 6 Store methods with full flag support, and comprehensive Rust unit tests.

## Legacy Source

---
phase: 03-sorted-set-commands
plan: 01
type: execute
wave: 1
depends_on: []
files_modified:
  - Cargo.toml
  - src/store.rs
  - src/commands/mod.rs
  - src/commands/sorted_sets.rs
autonomous: true
requirements: [ZSET-01, ZSET-02, ZSET-03, ZSET-04, ZSET-05, ZSET-06]

must_haves:
  truths:
    - "Store can hold SortedSet values alongside String, Hash, and Set values"
    - "ZADD inserts members with scores using dual-index BTreeMap+HashMap pattern"
    - "ZADD respects NX/XX/GT/LT/CH flags with correct return value semantics"
    - "ZREM removes members and cleans up both indexes"
    - "ZRANGE returns members in score order by index range"
    - "ZRANGEBYSCORE returns members within a score range including -inf/+inf"
    - "ZRANGESTORE copies a score-range result into a destination key"
    - "ZREMRANGEBYSCORE removes all members within a score range"
    - "Operations on wrong-type keys return WRONGTYPE error"
  artifacts:
    - path: "src/store.rs"
      provides: "SortedSet variant in ValueData enum, 6 sorted set Store methods"
      contains: "SortedSet"
    - path: "src/commands/sorted_sets.rs"
      provides: "Sorted set command module with documentation"
      min_lines: 10
    - path: "Cargo.toml"
      provides: "ordered-float dependency for BTreeMap key ordering"
      contains: "ordered-float"
  key_links:
    - from: "src/store.rs"
      to: "Cargo.toml"
      via: "ordered-float dependency for OrderedFloat<f64>"
      pattern: "OrderedFloat"
    - from: "src/store.rs"
      to: "src/commands/sorted_sets.rs"
      via: "Store methods for sorted set operations"
      pattern: "pub fn z(add|rem|range)"
---

<objective>
Extend the Store engine to support sorted set data type with dual-index pattern and all 6 sorted set operations.

Purpose: The in-memory store needs a SortedSet value type using BTreeMap<(OrderedFloat<f64>, Bytes), ()> for score-ordered range queries plus HashMap<Bytes, f64> for O(1) member-to-score lookup. This enables ZADD (with NX/XX/GT/LT/CH flags), ZREM, ZRANGE, ZRANGEBYSCORE, ZRANGESTORE, and ZREMRANGEBYSCORE at the Rust engine level.

Output: Extended Store with SortedSet ValueData variant, ordered-float dependency, 6 Store methods with full flag support, and comprehensive Rust unit tests.
</objective>

<execution_context>
@$HOME/.claude/get-shit-done/workflows/execute-plan.md
@$HOME/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@.planning/PROJECT.md
@.planning/ROADMAP.md
@.planning/STATE.md
@.planning/phases/02-hash-and-set-commands/02-01-SUMMARY.md

<interfaces>
<!-- Key types and contracts from the existing codebase that the executor needs. -->

From src/store.rs:
```rust
use bytes::Bytes;
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub enum ValueData {
    String(Bytes),
    Hash(HashMap<Bytes, Bytes>),
    Set(HashSet<Bytes>),
}

#[derive(Clone, Debug)]
pub struct ValueEntry {
    pub data: ValueData,
    pub expires_at: Option<Instant>,
}

impl ValueEntry {
    pub fn new(data: Bytes, ttl: Option<Duration>) -> Self;
    pub fn new_hash() -> Self;
    pub fn new_set() -> Self;
    pub fn is_expired(&self) -> bool;
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("WRONGTYPE Operation against a key holding the wrong kind of value")]
    WrongType,
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
    // Hash methods: hset, hget, hdel, hvals
    // Set methods: sadd, smembers, sismember, srem
}
```

From src/commands/mod.rs:
```rust
pub mod strings;
pub mod hashes;
pub mod sets;
```

From Cargo.toml:
```toml
[dependencies]
pyo3 = { version = "0.28.3", features = ["extension-module", "abi3-py39"] }
pyo3-async-runtimes = { version = "0.28.0", features = ["tokio-runtime"] }
tokio = { version = "1.51", features = ["rt", "time", "sync"] }
parking_lot = "0.12.5"
bytes = "1.11"
thiserror = "2.0"
```
</interfaces>
</context>

<tasks>

<task type="auto">
  <name>Task 1: Add SortedSet variant to ValueData and implement all 6 sorted set Store methods</name>
  <files>Cargo.toml, src/store.rs</files>
  <read_first>Cargo.toml, src/store.rs</read_first>
  <action>
1. Add the `ordered-float` crate to `Cargo.toml` dependencies:
   ```toml
   ordered-float = "5"
   ```
   This provides `OrderedFloat<f64>` which implements `Ord` for use as a BTreeMap key.

2. In `src/store.rs`, add new imports at the top:
   ```rust
   use std::collections::BTreeMap;
   use ordered_float::OrderedFloat;
   ```

3. Define a `SortedSet` struct (inside store.rs, above `ValueData`):
   ```rust
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
               self.by_score.remove(&(OrderedFloat(old_score), member.clone()));
               self.by_score.insert((OrderedFloat(score), member.clone()), ());
               self.by_member.insert(member, score);
               false // not new
           } else {
               self.by_score.insert((OrderedFloat(score), member.clone()), ());
               self.by_member.insert(member, score);
               true // new member
           }
       }

       /// Remove a member. Returns true if member existed.
       pub fn remove(&mut self, member: &Bytes) -> bool {
           if let Some(score) = self.by_member.remove(member) {
               self.by_score.remove(&(OrderedFloat(score), member.clone()));
               true
           } else {
               false
           }
       }
   }
   ```

4. Add `SortedSet` variant to `ValueData` enum:
   ```rust
   pub enum ValueData {
       String(Bytes),
       Hash(HashMap<Bytes, Bytes>),
       Set(HashSet<Bytes>),
       SortedSet(SortedSet),
   }
   ```

5. Add `ValueEntry::new_sorted_set` constructor:
   ```rust
   pub fn new_sorted_set() -> Self {
       ValueEntry {
           data: ValueData::SortedSet(SortedSet::new()),
           expires_at: None,
       }
   }
   ```

6. Update `Store::get` to return None for SortedSet type (add `ValueData::SortedSet(_)` to the match arm that returns None for non-string types).

7. Implement `Store::zadd` method:
   ```rust
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
   ) -> Result<i64, StoreError>
   ```
   Implementation:
   - Acquire write lock. Passive expiration check (remove key if expired).
   - Use `entry().or_insert_with(ValueEntry::new_sorted_set)` to get/create the sorted set.
   - Match on `ValueData::SortedSet(ref mut zset)`, return `Err(StoreError::WrongType)` for other types.
   - For each `(score, member)`:
     - Check if member exists in `zset.by_member`:
       - If member exists AND `nx` is true: skip (NX = only add new)
       - If member does NOT exist AND `xx` is true: skip (XX = only update existing)
       - If member exists: check GT/LT constraints:
         - If `gt` and `score <= old_score`: skip
         - If `lt` and `score >= old_score`: skip
         - Otherwise: update score via `zset.insert(member, score)`, increment `changed` counter
       - If member is new: `zset.insert(member, score)`, increment both `added` and `changed` counters
   - Return `added` if `ch` is false, `changed` if `ch` is true.

8. Implement `Store::zrem` method:
   ```rust
   /// ZREM: Removes members from a sorted set. Returns count of members removed.
   pub fn zrem(&self, key: &Bytes, members: &[Bytes]) -> Result<i64, StoreError>
   ```
   - Write lock, passive expiration, match on SortedSet.
   - For each member, call `zset.remove(member)`, count removals.
   - Missing key returns Ok(0). Wrong type returns Err.

9. Implement `Store::zrange` method:
   ```rust
   /// ZRANGE: Returns members by index range (0-based, supports negative indices).
   /// When withscores=true, returns (member, score) pairs.
   pub fn zrange(
       &self,
       key: &Bytes,
       start: i64,
       stop: i64,
       withscores: bool,
   ) -> Result<Vec<(Bytes, Option<f64>)>, StoreError>
   ```
   - Read lock (upgrade to write for expiration if needed), passive expiration.
   - Match on SortedSet.
   - Collect all entries from `zset.by_score` into a sorted vec (BTreeMap is already sorted).
   - Convert negative indices: if start < 0, start = max(0, len + start). Same for stop.
   - Clamp stop to len - 1.
   - If start > stop or start >= len, return empty vec.
   - Slice the range and return members. If `withscores`, include `Some(score)`, else `None` for score.
   - Missing key returns Ok(empty vec).

10. Implement `Store::zrangebyscore` method:
    ```rust
    /// ZRANGEBYSCORE: Returns members with scores in [min, max] range.
    /// min and max are f64 where -inf = f64::NEG_INFINITY and +inf = f64::INFINITY.
    pub fn zrangebyscore(
        &self,
        key: &Bytes,
        min: f64,
        max: f64,
        withscores: bool,
    ) -> Result<Vec<(Bytes, Option<f64>)>, StoreError>
    ```
    - Write lock for expiration, passive expiration.
    - Match on SortedSet.
    - Use BTreeMap range query: iterate from `(OrderedFloat(min), Bytes::new())` to the end, taking while score <= max.
    - For efficient range, use `zset.by_score.range((Included((OrderedFloat(min), Bytes::new())), Unbounded))` and filter `score <= max`. Note: since BTreeMap orders by (score, member) lexicographically, and we want all members with `min <= score <= max`, we need to iterate and check scores.
    - Actually, the clearest approach: use `zset.by_score.range(..)` with a bound. The lower bound is `Included(&(OrderedFloat(min), Bytes::new()))` and iterate while the score component `<= max`. Use `std::ops::Bound::Included` and `std::ops::Bound::Unbounded`.
    - Return members with optional scores.
    - Missing key returns Ok(empty vec).

11. Implement `Store::zrangestore` method:
    ```rust
    /// ZRANGESTORE: Stores the result of a ZRANGEBYSCORE into a destination key.
    /// Returns the count of elements stored.
    pub fn zrangestore(
        &self,
        dst: Bytes,
        src: &Bytes,
        min: f64,
        max: f64,
    ) -> Result<i64, StoreError>
    ```
    - Write lock for both reading src and writing dst.
    - Passive expiration on src.
    - Get the score range from the source sorted set (same logic as zrangebyscore).
    - Create a new SortedSet for the destination, insert all matching members.
    - If the result set is empty, do not create the destination key (or remove it if it existed).
    - If the result set is non-empty, insert/replace the destination key with the new SortedSet.
    - Return count of elements stored.
    - If src key missing, store empty set (returns 0).
    - If src key is wrong type, return Err(WrongType).

12. Implement `Store::zremrangebyscore` method:
    ```rust
    /// ZREMRANGEBYSCORE: Removes all members with scores in [min, max] range.
    /// Returns count of members removed.
    pub fn zremrangebyscore(
        &self,
        key: &Bytes,
        min: f64,
        max: f64,
    ) -> Result<i64, StoreError>
    ```
    - Write lock, passive expiration.
    - Match on SortedSet.
    - Collect members to remove (those with min <= score <= max) into a temporary vec.
    - Remove each from both by_score and by_member.
    - Return count removed.
    - Missing key returns Ok(0). Wrong type returns Err.

13. Add comprehensive Rust unit tests in `#[cfg(test)] mod tests` at the bottom of store.rs:

    **ZADD tests:**
    - `test_zadd_new_members`: ZADD to new key, returns count of new members
    - `test_zadd_update_existing_score`: ZADD existing member with new score, returns 0 (not new)
    - `test_zadd_nx_flag`: NX only adds new members, skips existing
    - `test_zadd_xx_flag`: XX only updates existing, skips new
    - `test_zadd_gt_flag`: GT only updates if new score is greater
    - `test_zadd_lt_flag`: LT only updates if new score is less
    - `test_zadd_ch_flag`: CH returns count of changed (new + updated) instead of just new
    - `test_zadd_wrongtype`: ZADD on a string key returns Err(WrongType)

    **ZREM tests:**
    - `test_zrem_existing_members`: Removes members, returns count
    - `test_zrem_missing_members`: Returns 0 for members not in set
    - `test_zrem_missing_key`: Returns 0 for non-existent key
    - `test_zrem_wrongtype`: Returns Err(WrongType) on wrong type

    **ZRANGE tests:**
    - `test_zrange_full_range`: Returns all members in score order
    - `test_zrange_subset`: Returns correct slice by index
    - `test_zrange_negative_indices`: Negative indices from end
    - `test_zrange_withscores`: Returns (member, Some(score)) pairs
    - `test_zrange_empty_key`: Returns empty vec for missing key
    - `test_zrange_out_of_bounds`: Out-of-range indices return empty

    **ZRANGEBYSCORE tests:**
    - `test_zrangebyscore_range`: Returns members within score range
    - `test_zrangebyscore_inf`: -inf/+inf return all members
    - `test_zrangebyscore_withscores`: Returns scores when requested
    - `test_zrangebyscore_empty`: No matches returns empty vec
    - `test_zrangebyscore_missing_key`: Returns empty for missing key

    **ZRANGESTORE tests:**
    - `test_zrangestore_basic`: Copies range to new key, returns count
    - `test_zrangestore_empty_range`: Empty range returns 0, no dest key created
    - `test_zrangestore_missing_src`: Missing source returns 0

    **ZREMRANGEBYSCORE tests:**
    - `test_zremrangebyscore_basic`: Removes members in range, returns count
    - `test_zremrangebyscore_no_matches`: No matches returns 0
    - `test_zremrangebyscore_missing_key`: Missing key returns 0
    - `test_zremrangebyscore_wrongtype`: Wrong type returns Err
  </action>
  <verify>
    <automated>cd /Users/desertaxle/dev/prefectlabs/burner-redis && cargo test 2>&1 | tail -20</automated>
  </verify>
  <acceptance_criteria>
    - grep -q "ordered-float" Cargo.toml (dependency added)
    - grep -q "SortedSet" src/store.rs (SortedSet struct and ValueData variant exist)
    - grep -q "OrderedFloat" src/store.rs (ordered-float used for BTreeMap keys)
    - grep -q "BTreeMap" src/store.rs (BTreeMap imported and used)
    - grep -q "pub fn zadd" src/store.rs (zadd method exists)
    - grep -q "pub fn zrem" src/store.rs (zrem method exists)
    - grep -q "pub fn zrange" src/store.rs (zrange method exists)
    - grep -q "pub fn zrangebyscore" src/store.rs (zrangebyscore method exists)
    - grep -q "pub fn zrangestore" src/store.rs (zrangestore method exists)
    - grep -q "pub fn zremrangebyscore" src/store.rs (zremrangebyscore method exists)
    - grep -q "new_sorted_set" src/store.rs (sorted set constructor exists)
    - grep -q "test_zadd" src/store.rs (zadd tests exist)
    - grep -q "test_zrem" src/store.rs (zrem tests exist)
    - grep -q "test_zrange" src/store.rs (zrange tests exist)
    - grep -q "test_zrangebyscore" src/store.rs (zrangebyscore tests exist)
    - grep -q "test_zrangestore" src/store.rs (zrangestore tests exist)
    - grep -q "test_zremrangebyscore" src/store.rs (zremrangebyscore tests exist)
    - cargo test passes with 0 failures
  </acceptance_criteria>
  <done>
    - ordered-float dependency added to Cargo.toml
    - SortedSet struct with dual-index (BTreeMap + HashMap) pattern implemented
    - ValueData enum has SortedSet variant
    - ValueEntry::new_sorted_set constructor exists
    - All 6 Store methods implemented: zadd (with NX/XX/GT/LT/CH flags), zrem, zrange (with withscores), zrangebyscore (with -inf/+inf and withscores), zrangestore, zremrangebyscore
    - WRONGTYPE errors returned for wrong-type keys
    - Passive expiration checks in all methods
    - 25+ new Rust unit tests covering all operations, flags, edge cases, and error conditions
    - All existing tests still pass (no regressions)
    - cargo test passes with all tests green
  </done>
</task>

<task type="auto">
  <name>Task 2: Add sorted set command module declaration</name>
  <files>src/commands/sorted_sets.rs, src/commands/mod.rs</files>
  <read_first>src/commands/mod.rs, src/commands/hashes.rs, src/commands/sets.rs</read_first>
  <action>
1. Create `src/commands/sorted_sets.rs` with documentation matching the established pattern from hashes.rs and sets.rs:
   ```rust
   //! Sorted set command helpers for the Python binding layer.
   //!
   //! This module provides helper functions for Redis sorted set commands.
   //! The actual Python method implementations live in lib.rs following
   //! the established pattern (via #[pymethods] on BurnerRedis).
   //!
   //! Sorted set commands implemented:
   //! - ZADD: Add members with scores to a sorted set (with NX/XX/GT/LT/CH flags)
   //! - ZREM: Remove one or more members from a sorted set
   //! - ZRANGE: Get members by index range
   //! - ZRANGEBYSCORE: Get members by score range
   //! - ZRANGESTORE: Store a score-range result into a destination key
   //! - ZREMRANGEBYSCORE: Remove members by score range
   //!
   //! The core sorted set logic lives in Store (src/store.rs) using the dual-index
   //! pattern: BTreeMap<(OrderedFloat<f64>, Bytes), ()> for score-ordered range queries
   //! plus HashMap<Bytes, f64> for O(1) member-to-score lookup.
   ```

2. Update `src/commands/mod.rs` to declare the new submodule:
   ```rust
   pub mod strings;
   pub mod hashes;
   pub mod sets;
   pub mod sorted_sets;
   ```
  </action>
  <verify>
    <automated>cd /Users/desertaxle/dev/prefectlabs/burner-redis && cargo check 2>&1 | tail -5</automated>
  </verify>
  <acceptance_criteria>
    - test -f src/commands/sorted_sets.rs (file exists)
    - grep -q "pub mod sorted_sets" src/commands/mod.rs (module declared)
    - grep -q "ZADD" src/commands/sorted_sets.rs (zadd documented)
    - grep -q "ZRANGEBYSCORE" src/commands/sorted_sets.rs (zrangebyscore documented)
    - grep -q "dual-index" src/commands/sorted_sets.rs (architecture documented)
    - cargo check succeeds with no errors
  </acceptance_criteria>
  <done>
    - src/commands/sorted_sets.rs exists with comprehensive documentation of all 6 sorted set commands
    - src/commands/mod.rs declares sorted_sets module
    - Crate compiles cleanly
  </done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|----------|-------------|
| Python args -> Rust Store | Untrusted Python values (scores, members, ranges) cross into Rust via extraction |

## STRIDE Threat Register

| Threat ID | Category | Component | Disposition | Mitigation Plan |
|-----------|----------|-----------|-------------|-----------------|
| T-03-01 | Tampering | Store type checking | mitigate | Every sorted set method checks ValueData variant before operating; returns StoreError::WrongType on mismatch |
| T-03-02 | Denial of Service | SortedSet unbounded growth | accept | No per-key size limits needed -- in-process library, user controls their own data |
| T-03-03 | Tampering | NaN/Infinity scores | mitigate | OrderedFloat handles NaN ordering correctly (treats NaN as greater than all values); f64::INFINITY and f64::NEG_INFINITY are valid BTreeMap keys |
</threat_model>

<verification>
- `cargo test` passes with all existing + new tests green
- `cargo check` confirms crate compiles
- ValueData enum has exactly 4 variants: String, Hash, Set, SortedSet
- SortedSet struct has both by_score (BTreeMap) and by_member (HashMap) fields
- All 6 sorted set Store methods return Result<_, StoreError> for type safety
- ZADD correctly handles all flag combinations (NX/XX/GT/LT/CH)
</verification>

<success_criteria>
- Store engine supports SortedSet value type with dual-index pattern
- ZADD with all flags (NX, XX, GT, LT, CH) works correctly
- ZREM removes members from both indexes
- ZRANGE returns members in correct score order by index
- ZRANGEBYSCORE returns members within score range including -inf/+inf
- ZRANGESTORE copies range results to a new key
- ZREMRANGEBYSCORE removes all members in a score range
- WRONGTYPE errors prevent cross-type operations
- All Rust tests pass including existing string/hash/set tests (no regressions)
</success_criteria>

<output>
After completion, create `.planning/phases/03-sorted-set-commands/03-01-SUMMARY.md`
</output>
