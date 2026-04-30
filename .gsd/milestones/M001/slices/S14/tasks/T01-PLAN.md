# T01: Implement the Rust engine foundation for the Redis list data type: add `ValueData::List(VecDeque<Bytes>)` variant, `list_notify: Arc<Notify>` field on `Store`, all 13 non-blocking `Store` list methods, and polling helpers (`blpop_poll`, `brpop_poll`, `lmove_atomic`, `rpoplpush_atomic`) that the blocking PyO3 layer in Plan 02 will call.

**Slice:** S14 — **Milestone:** M001

## Description

Implement the Rust engine foundation for the Redis list data type: add `ValueData::List(VecDeque<Bytes>)` variant, `list_notify: Arc<Notify>` field on `Store`, all 13 non-blocking `Store` list methods, and polling helpers (`blpop_poll`, `brpop_poll`, `lmove_atomic`, `rpoplpush_atomic`) that the blocking PyO3 layer in Plan 02 will call. Also create `src/commands/lists.rs` helper module and update REQUIREMENTS.md with LIST-01..LIST-16 mapped to Phase 14.

Purpose: The engine is the lock-owning, data-owning subsystem. Every command in the phase flows through these methods. Getting the Store layer correct (notify-inside-write-lock, empty-list-deletes-key, WRONGTYPE matching) means the PyO3 and Lua layers in later plans are pure adapters.

Output: A compilable Rust core with full unit-test coverage for 13 non-blocking list commands, plus the polling helpers needed by Plan 02's blocking loops. REQUIREMENTS.md updated to canonicalize LIST-* IDs.

## Legacy Source

---
phase: 14-add-support-for-the-redis-list-data-type-required-commands-l
plan: 01
type: execute
wave: 1
depends_on: []
files_modified:
  - src/store.rs
  - src/commands/lists.rs
  - src/commands/mod.rs
  - .planning/REQUIREMENTS.md
autonomous: true
requirements:
  - LIST-01
  - LIST-02
  - LIST-03
  - LIST-04
  - LIST-05
  - LIST-06
  - LIST-07
  - LIST-08
  - LIST-09
  - LIST-10
  - LIST-11
  - LIST-12
  - LIST-13
tags:
  - rust
  - store
  - lists
  - engine

must_haves:
  truths:
    - "ValueData::List(VecDeque<Bytes>) variant exists on the Store enum"
    - "Store has a list_notify: Arc<Notify> field, constructed in Store::new, waked in Store::shutdown"
    - "Every non-blocking list command has a Store method returning Result<T, StoreError>"
    - "LPUSH/RPUSH/LMOVE(dst)/RPOPLPUSH(dst) call self.list_notify.notify_waiters() inside the write lock after mutation"
    - "Pop commands (LPOP/RPOP/LREM/LTRIM) delete the key when the list becomes empty (D-03)"
    - "WRONGTYPE is returned when any list op runs against a non-list key"
    - "Rust cargo test --lib list unit tests pass for all 13 non-blocking list ops"
    - "REQUIREMENTS.md has LIST-01..LIST-16 defined and maps them to Phase 14 in Traceability"
    - "BLPOP/BRPOP line removed from REQUIREMENTS.md Out of Scope table"
  artifacts:
    - path: "src/store.rs"
      provides: "ValueData::List variant, list_notify field, 13 non-blocking Store methods, blpop_poll/brpop_poll/lmove_atomic/rpoplpush_atomic helpers for blocking layer"
      contains: "ValueData::List(VecDeque<Bytes>)"
    - path: "src/commands/lists.rs"
      provides: "Helpers: ListEnd, InsertPosition, parse_list_end, parse_linsert_where, normalize_range_indices, parse_lrem_count"
      exports: ["ListEnd", "InsertPosition", "parse_list_end", "parse_linsert_where", "normalize_range_indices"]
    - path: "src/commands/mod.rs"
      provides: "Registers new lists module"
      contains: "pub mod lists;"
    - path: ".planning/REQUIREMENTS.md"
      provides: "LIST-01..LIST-16 requirement definitions and Phase 14 traceability"
      contains: "LIST-01"
  key_links:
    - from: "src/store.rs (ValueData enum line 118)"
      to: "ValueData::List variant"
      via: "enum variant addition"
      pattern: "ValueData::List\\("
    - from: "src/store.rs (LPUSH/RPUSH/LMOVE/RPOPLPUSH)"
      to: "self.list_notify.notify_waiters()"
      via: "inside write lock after mutation"
      pattern: "list_notify\\.notify_waiters"
    - from: "src/store.rs (Store::shutdown)"
      to: "list_notify.notify_waiters()"
      via: "shutdown wake"
      pattern: "list_notify\\.notify_waiters"
    - from: "src/commands/mod.rs"
      to: "pub mod lists;"
      via: "module registration"
      pattern: "pub mod lists"
---

<objective>
Implement the Rust engine foundation for the Redis list data type: add `ValueData::List(VecDeque<Bytes>)` variant, `list_notify: Arc<Notify>` field on `Store`, all 13 non-blocking `Store` list methods, and polling helpers (`blpop_poll`, `brpop_poll`, `lmove_atomic`, `rpoplpush_atomic`) that the blocking PyO3 layer in Plan 02 will call. Also create `src/commands/lists.rs` helper module and update REQUIREMENTS.md with LIST-01..LIST-16 mapped to Phase 14.

Purpose: The engine is the lock-owning, data-owning subsystem. Every command in the phase flows through these methods. Getting the Store layer correct (notify-inside-write-lock, empty-list-deletes-key, WRONGTYPE matching) means the PyO3 and Lua layers in later plans are pure adapters.

Output: A compilable Rust core with full unit-test coverage for 13 non-blocking list commands, plus the polling helpers needed by Plan 02's blocking loops. REQUIREMENTS.md updated to canonicalize LIST-* IDs.
</objective>

<execution_context>
@$HOME/.claude/get-shit-done/workflows/execute-plan.md
@$HOME/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@.planning/PROJECT.md
@.planning/ROADMAP.md
@.planning/STATE.md
@.planning/REQUIREMENTS.md
@.planning/phases/14-add-support-for-the-redis-list-data-type-required-commands-l/14-CONTEXT.md
@.planning/phases/14-add-support-for-the-redis-list-data-type-required-commands-l/14-RESEARCH.md
@.planning/phases/14-add-support-for-the-redis-list-data-type-required-commands-l/14-PATTERNS.md
@.planning/phases/14-add-support-for-the-redis-list-data-type-required-commands-l/14-VALIDATION.md
@src/store.rs
@src/commands/mod.rs
@src/commands/streams.rs
@src/commands/sets.rs

<interfaces>
<!-- Key types and functions the executor needs. Use directly; no codebase exploration required. -->

Existing types in src/store.rs:
```rust
// line 118 — add List variant here
#[derive(Clone, Debug)]
pub enum ValueData {
    String(Bytes),
    Hash(HashMap<Bytes, Bytes>),
    Set(HashSet<Bytes>),
    SortedSet(SortedSet),
    Stream(Stream),
    // ADD: List(VecDeque<Bytes>),
}

// lines 149-178 — constructor style (mirror for new_list)
impl ValueEntry {
    pub fn new_hash() -> Self { ValueEntry { data: ValueData::Hash(HashMap::new()), expires_at: None } }
    // ... new_set / new_sorted_set / new_stream ...
    // ADD new_list mirror
}

// line 190+ — StoreError (already has WrongType)
pub enum StoreError {
    WrongType,
    // ... may need to add IndexOutOfRange for LSET (check if exists; add if missing)
}

// lines 275-277 — Store struct field layout
pub struct Store {
    data: RwLock<HashMap<Bytes, ValueEntry>>,
    scripts: RwLock<HashMap<String, String>>,
    pub(crate) pubsub: RwLock<PubSubRegistry>,
    stream_notify: Arc<Notify>,
    // ADD: list_notify: Arc<Notify>,
    shutdown: AtomicBool,
}

// lines 280-288 — Store::new layout
// ADD list_notify: Arc::new(Notify::new()) in constructor

// lines 290-293 — accessor style
pub fn stream_notify(&self) -> Arc<Notify> { self.stream_notify.clone() }
// ADD pub fn list_notify(&self) -> Arc<Notify> { self.list_notify.clone() }

// lines 298-312 — shutdown fires stream_notify; ADD list_notify fire
```

Existing helper in src/commands/strings.rs:
```rust
pub fn extract_bytes(obj: &Bound<'_, PyAny>) -> PyResult<Bytes>;
```

Existing sadd template at src/store.rs:697-723 — use as the template for lpush/rpush.

Existing xadd notify-inside-write-lock at src/store.rs:1259-1263:
```rust
stream.entries.insert(new_id, fields);
stream.last_id = new_id;
// Wake any blocking XREADGROUP waiters
self.stream_notify.notify_waiters();  // <-- inside write lock, after mutation
Ok(new_id)
```
</interfaces>
</context>

<tasks>

<task type="auto" tdd="true">
  <name>Task 1: Add List variant + list_notify field + constructors + shutdown wake + REQUIREMENTS.md</name>
  <read_first>
    - src/store.rs (full file — need current ValueData enum at line 118, Store struct at 275, Store::new at 280-288, Store::shutdown at 298-312, PersistableValueData at 2738-2745 and arms at 2799-2863 / 2899-2925)
    - src/commands/mod.rs (all 6 lines — append `pub mod lists;`)
    - .planning/REQUIREMENTS.md (full file — need Out of Scope table at 120-131, Stream Commands section at 48-60 as analog, Traceability table at 137-189)
    - .planning/phases/14-add-support-for-the-redis-list-data-type-required-commands-l/14-PATTERNS.md (sections "ValueData::List(VecDeque<Bytes>) variant and helpers", ".planning/REQUIREMENTS.md")
    - .planning/phases/14-add-support-for-the-redis-list-data-type-required-commands-l/14-CONTEXT.md (D-04, D-06, D-10, D-21)
  </read_first>
  <behavior>
    - ValueData::List(VecDeque<Bytes>) variant is enumerable and matchable
    - ValueEntry::new_list() constructs entry with empty VecDeque, expires_at=None
    - store.list_notify() returns Arc<Notify> clone
    - Store::shutdown() wakes both stream_notify and list_notify waiters
    - PersistableValueData round-trips a ValueData::List via rmp-serde (from_store + into_runtime arms)
    - .planning/REQUIREMENTS.md contains LIST-01..LIST-16 definitions, has BLPOP/BRPOP removed from Out of Scope, and maps LIST-01..LIST-16 → Phase 14 in Traceability
  </behavior>
  <action>
Make the following edits:

**1. `src/store.rs` — imports (around line 5):**
Add `VecDeque` to the collections import:
```rust
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
```

**2. `src/store.rs` — ValueData enum (at line 118, per D-04):**
Add the new variant immediately after the `Stream(Stream)` variant:
```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ValueData {
    String(Bytes),
    Hash(HashMap<Bytes, Bytes>),
    Set(HashSet<Bytes>),
    SortedSet(SortedSet),
    Stream(Stream),
    List(VecDeque<Bytes>),
}
```
If `Serialize`/`Deserialize` derives are NOT already on the enum, leave the existing derive set — serde handling is managed via PersistableValueData.

**3. `src/store.rs` — ValueEntry constructors (around line 149-178):**
Add sibling constructor next to `new_stream`:
```rust
pub fn new_list() -> Self {
    ValueEntry { data: ValueData::List(VecDeque::new()), expires_at: None }
}
```

**4. `src/store.rs` — Store struct field (per D-06, around line 275):**
Add `list_notify: Arc<Notify>` between `stream_notify` and `shutdown`:
```rust
pub struct Store {
    data: RwLock<HashMap<Bytes, ValueEntry>>,
    scripts: RwLock<HashMap<String, String>>,
    pub(crate) pubsub: RwLock<PubSubRegistry>,
    stream_notify: Arc<Notify>,
    list_notify: Arc<Notify>,
    shutdown: AtomicBool,
}
```

**5. `src/store.rs` — Store::new() (around line 280-288):**
Add `list_notify: Arc::new(Notify::new()),` next to `stream_notify`.

**6. `src/store.rs` — Accessor (adjacent to `pub fn stream_notify`):**
```rust
pub fn list_notify(&self) -> Arc<Notify> {
    self.list_notify.clone()
}
```

**7. `src/store.rs` — Store::shutdown() (around line 298-312, per D-08):**
After the existing `self.stream_notify.notify_waiters();` line, add:
```rust
self.list_notify.notify_waiters();
```

**8. `src/store.rs` — PersistableValueData:**
Look at the existing enum definition near line 2738-2745. Add a `List(Vec<Vec<u8>>)` variant (use `Vec<Vec<u8>>` per PATTERNS.md guidance — consistent with Set arm). Add `from_store` arm (line 2799-2808 style) converting `ValueData::List(deque)` → `PersistableValueData::List(deque.iter().map(|b| b.to_vec()).collect())` and `into_runtime` arm (line 2899-2912 style) converting back: `PersistableValueData::List(v) => ValueData::List(v.into_iter().map(Bytes::from).collect::<VecDeque<_>>())`.

**9. `src/commands/mod.rs` — register module:**
Append `pub mod lists;` as a new line (the file currently has 6 lines, add a 7th for `pub mod lists;`).

**10. `.planning/REQUIREMENTS.md` — three edits per D-21:**
  a. In the "Out of Scope" table (around line 130), DELETE the row:
     `| Blocking list commands (BLPOP/BRPOP) | Prefect uses Streams, not blocking lists |`
  b. After the "### Stream Commands" section (after line 60), INSERT a new section:
```markdown

### List Commands

- [ ] **LIST-01**: User can LPUSH one or more values onto the head of a list
- [ ] **LIST-02**: User can RPUSH one or more values onto the tail of a list
- [ ] **LIST-03**: User can LPOP with optional count (returns bytes, list of bytes, or None)
- [ ] **LIST-04**: User can RPOP with the same semantics as LPOP
- [ ] **LIST-05**: User can LRANGE with negative indices to slice a list
- [ ] **LIST-06**: User can LLEN to get the length of a list
- [ ] **LIST-07**: User can LINDEX to read an element at an index
- [ ] **LIST-08**: User can LINSERT BEFORE or AFTER a pivot
- [ ] **LIST-09**: User can LREM with positive, negative, or zero count
- [ ] **LIST-10**: User can LSET to replace an element at an index
- [ ] **LIST-11**: User can LTRIM to clamp a list to a range
- [ ] **LIST-12**: User can LMOVE between two lists atomically
- [ ] **LIST-13**: User can RPOPLPUSH (legacy alias for LMOVE RIGHT LEFT)
- [ ] **LIST-14**: User can BRPOP/BLPOP with float-seconds timeout, multi-key scan
- [ ] **LIST-15**: User can BLMOVE with timeout, atomic src/dst semantics
- [ ] **LIST-16**: All list commands work in pipelines; 13 non-blocking work in Lua
```
  c. Append 16 rows to the Traceability table, one per LIST-NN:
```markdown
| LIST-01 | Phase 14 | In Progress |
| LIST-02 | Phase 14 | In Progress |
| LIST-03 | Phase 14 | In Progress |
| LIST-04 | Phase 14 | In Progress |
| LIST-05 | Phase 14 | In Progress |
| LIST-06 | Phase 14 | In Progress |
| LIST-07 | Phase 14 | In Progress |
| LIST-08 | Phase 14 | In Progress |
| LIST-09 | Phase 14 | In Progress |
| LIST-10 | Phase 14 | In Progress |
| LIST-11 | Phase 14 | In Progress |
| LIST-12 | Phase 14 | In Progress |
| LIST-13 | Phase 14 | In Progress |
| LIST-14 | Phase 14 | In Progress |
| LIST-15 | Phase 14 | In Progress |
| LIST-16 | Phase 14 | In Progress |
```
  d. Update the Coverage block: `v1 requirements: 69 total` (53 + 16), `Mapped to phases: 69`.

**11. Write one Rust unit test in `src/store.rs` `#[cfg(test)] mod tests` (or create one if missing) to verify scaffolding:**
```rust
#[test]
fn list_variant_constructs() {
    let entry = ValueEntry::new_list();
    assert!(matches!(entry.data, ValueData::List(_)));
    if let ValueData::List(ref list) = entry.data {
        assert_eq!(list.len(), 0);
    }
}

#[test]
fn list_notify_accessor_works() {
    let store = Store::new();
    let _notify = store.list_notify();  // must compile and return Arc<Notify>
}

#[test]
fn shutdown_wakes_list_waiters() {
    // Smoke test — just call shutdown; full wake behavior covered by integration tests
    let store = Store::new();
    store.shutdown();
    assert!(store.is_shutdown());
}
```
  </action>
  <verify>
    <automated>cargo build --lib 2>&1 | tee /tmp/phase14-task1-build.log; cargo test --lib store::tests::list_variant_constructs store::tests::list_notify_accessor_works store::tests::shutdown_wakes_list_waiters -- --exact 2>&1 | tee /tmp/phase14-task1-tests.log; grep -q "LIST-01" .planning/REQUIREMENTS.md && grep -q "LIST-16" .planning/REQUIREMENTS.md && ! grep -q "Blocking list commands (BLPOP/BRPOP)" .planning/REQUIREMENTS.md && grep -q "pub mod lists;" src/commands/mod.rs && echo PASS-TASK1-SCAFFOLD</automated>
  </verify>
  <acceptance_criteria>
    - `cargo build --lib` exits 0
    - `cargo test --lib` passes `store::tests::list_variant_constructs`, `list_notify_accessor_works`, `shutdown_wakes_list_waiters`
    - `grep -q "ValueData::List" src/store.rs` returns 0
    - `grep -q "list_notify:" src/store.rs` returns 0 (field declared)
    - `grep -q "self.list_notify.notify_waiters" src/store.rs` returns 0 (shutdown wake)
    - `grep -q "pub mod lists;" src/commands/mod.rs` returns 0
    - `grep -q "LIST-01" .planning/REQUIREMENTS.md` returns 0
    - `grep -q "LIST-16" .planning/REQUIREMENTS.md` returns 0
    - `grep -qE "Blocking list commands \\(BLPOP/BRPOP\\)" .planning/REQUIREMENTS.md` returns 1 (NOT present)
  </acceptance_criteria>
  <done>Variant, field, accessor, constructors, shutdown wake, persistence round-trip arms, commands/mod.rs registration, and REQUIREMENTS.md update all land in one commit. `cargo build --lib` and 3 smoke tests green.</done>
</task>

<task type="auto" tdd="true">
  <name>Task 2: src/commands/lists.rs helpers + cargo-test unit coverage of normalize/parse functions</name>
  <read_first>
    - src/commands/streams.rs (full 37-line file — template)
    - src/commands/sets.rs (file header convention)
    - src/store.rs (StoreError enum — check current variants so `parse_list_end` returns the right error type)
    - .planning/phases/14-add-support-for-the-redis-list-data-type-required-commands-l/14-PATTERNS.md (section "src/commands/lists.rs (NEW)")
    - .planning/phases/14-add-support-for-the-redis-list-data-type-required-commands-l/14-RESEARCH.md (Pattern 2: LRANGE negative-index normalization; Pattern 3: LREM count-sign; Open Question #2 on LSET wording)
    - .planning/phases/14-add-support-for-the-redis-list-data-type-required-commands-l/14-CONTEXT.md (D-05, Claude's Discretion for LRANGE/LINSERT/LREM)
  </read_first>
  <behavior>
    - `ListEnd::Left` and `ListEnd::Right` are constructable and parseable
    - `parse_list_end("LEFT")` → `Ok(ListEnd::Left)`, `parse_list_end("left")` → `Ok(ListEnd::Left)` (case-insensitive), `parse_list_end("up")` → `Err`
    - `parse_linsert_where("BEFORE")` → `Ok(InsertPosition::Before)`, `parse_linsert_where("after")` → `Ok(InsertPosition::After)`, anything else → `Err`
    - `normalize_range_indices(start, stop, len)` applies the 9-case matrix in RESEARCH.md Pattern 2 table
    - `parse_lrem_count(i64)` returns `LremDirection::Head(n)`, `LremDirection::Tail(n)`, or `LremDirection::All`
  </behavior>
  <action>
Create `src/commands/lists.rs` as a new file. Put every helper + its #[cfg(test)] tests in the same file. Concrete content:

```rust
//! List command helpers for the Python binding layer.
//!
//! This module provides helper functions for Redis list commands.
//! The actual Python method implementations live in lib.rs following
//! the established pattern (via #[pymethods] on BurnerRedis).
//!
//! List commands implemented:
//! - LPUSH, RPUSH, LPOP, RPOP, LRANGE, LLEN, LINDEX, LINSERT,
//! - LREM, LSET, LTRIM, LMOVE, RPOPLPUSH, BRPOP, BLPOP, BLMOVE
//!
//! The core list logic lives in Store (src/store.rs).

use crate::store::StoreError;

/// Which end of a list to operate on (for LMOVE/BLMOVE/RPOPLPUSH).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListEnd {
    Left,
    Right,
}

/// Whether LINSERT operates before or after the pivot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InsertPosition {
    Before,
    After,
}

/// LREM direction interpreted from the signed count argument.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LremDirection {
    /// count > 0 — scan head-to-tail, remove up to N matches.
    Head(usize),
    /// count < 0 — scan tail-to-head, remove up to N matches.
    Tail(usize),
    /// count == 0 — remove all matches.
    All,
}

/// Parse "LEFT" / "RIGHT" case-insensitively.
pub fn parse_list_end(s: &str) -> Result<ListEnd, StoreError> {
    match s.to_ascii_uppercase().as_str() {
        "LEFT" => Ok(ListEnd::Left),
        "RIGHT" => Ok(ListEnd::Right),
        _ => Err(StoreError::Syntax(format!(
            "ERR syntax error: expected LEFT or RIGHT, got {}",
            s
        ))),
    }
}

/// Parse "BEFORE" / "AFTER" case-insensitively for LINSERT.
pub fn parse_linsert_where(s: &str) -> Result<InsertPosition, StoreError> {
    match s.to_ascii_uppercase().as_str() {
        "BEFORE" => Ok(InsertPosition::Before),
        "AFTER" => Ok(InsertPosition::After),
        _ => Err(StoreError::Syntax(format!(
            "ERR syntax error: expected BEFORE or AFTER, got {}",
            s
        ))),
    }
}

/// Map a signed LREM count to its direction.
pub fn parse_lrem_count(count: i64) -> LremDirection {
    match count.cmp(&0) {
        std::cmp::Ordering::Greater => LremDirection::Head(count as usize),
        std::cmp::Ordering::Less => LremDirection::Tail((-count) as usize),
        std::cmp::Ordering::Equal => LremDirection::All,
    }
}

/// Normalize a Python-style (negative-allowed) inclusive range to concrete
/// usize bounds for a list of `len` elements.
///
/// Returns `None` if the normalized range is empty (start > end, end < 0 after
/// normalization, or list is empty).
pub fn normalize_range_indices(start: i64, stop: i64, len: usize) -> Option<(usize, usize)> {
    if len == 0 {
        return None;
    }
    let n = len as i64;
    let start = if start < 0 { (start + n).max(0) } else { start.min(n - 1) };
    let end = if stop < 0 { stop + n } else { stop.min(n - 1) };
    if start > end || end < 0 {
        return None;
    }
    Some((start as usize, end as usize))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_list_end_uppercase() {
        assert_eq!(parse_list_end("LEFT").unwrap(), ListEnd::Left);
        assert_eq!(parse_list_end("RIGHT").unwrap(), ListEnd::Right);
    }
    #[test]
    fn parse_list_end_lowercase() {
        assert_eq!(parse_list_end("left").unwrap(), ListEnd::Left);
        assert_eq!(parse_list_end("right").unwrap(), ListEnd::Right);
    }
    #[test]
    fn parse_list_end_invalid() {
        assert!(parse_list_end("up").is_err());
        assert!(parse_list_end("").is_err());
    }
    #[test]
    fn parse_linsert_where_variants() {
        assert_eq!(parse_linsert_where("BEFORE").unwrap(), InsertPosition::Before);
        assert_eq!(parse_linsert_where("after").unwrap(), InsertPosition::After);
        assert!(parse_linsert_where("AROUND").is_err());
    }
    #[test]
    fn parse_lrem_count_sign() {
        assert_eq!(parse_lrem_count(3), LremDirection::Head(3));
        assert_eq!(parse_lrem_count(-2), LremDirection::Tail(2));
        assert_eq!(parse_lrem_count(0), LremDirection::All);
    }
    #[test]
    fn normalize_range_indices_matrix() {
        // 9-case matrix from RESEARCH.md Pattern 2:
        assert_eq!(normalize_range_indices(0, -1, 5), Some((0, 4)));     // all
        assert_eq!(normalize_range_indices(0, 100, 5), Some((0, 4)));    // end clamps
        assert_eq!(normalize_range_indices(-100, 100, 5), Some((0, 4))); // both clamp
        assert_eq!(normalize_range_indices(-3, -1, 5), Some((2, 4)));    // last three
        assert_eq!(normalize_range_indices(-3, 2, 5), Some((2, 2)));     // one element
        assert_eq!(normalize_range_indices(5, 10, 5), None);             // start past end
        assert_eq!(normalize_range_indices(3, 2, 5), None);              // start > end
        assert_eq!(normalize_range_indices(-10, -6, 5), None);           // end < 0 after
        assert_eq!(normalize_range_indices(0, 0, 0), None);              // empty list
    }
}
```

**IMPORTANT — StoreError::Syntax variant:** If `StoreError` does not already have a `Syntax(String)` variant, add it in `src/store.rs` alongside `WrongType`:
```rust
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("WRONGTYPE Operation against a key holding the wrong kind of value")]
    WrongType,
    #[error("{0}")]
    Syntax(String),
    // ... existing variants stay unchanged ...
}
```
If the enum already has a catch-all `Generic(String)` or equivalent, use that instead — match the existing style.
  </action>
  <verify>
    <automated>cargo build --lib 2>&1 | tail -20; cargo test --lib commands::lists::tests -- 2>&1 | tee /tmp/phase14-task2-tests.log | tail -40; grep -q "parse_list_end" src/commands/lists.rs && grep -q "normalize_range_indices" src/commands/lists.rs && echo PASS-TASK2</automated>
  </verify>
  <acceptance_criteria>
    - `cargo build --lib` exits 0
    - `cargo test --lib commands::lists::tests` shows 6 passing tests (parse_list_end_uppercase/lowercase/invalid, parse_linsert_where_variants, parse_lrem_count_sign, normalize_range_indices_matrix)
    - `src/commands/lists.rs` exports `ListEnd`, `InsertPosition`, `LremDirection`, `parse_list_end`, `parse_linsert_where`, `parse_lrem_count`, `normalize_range_indices`
    - `grep -q "pub enum ListEnd" src/commands/lists.rs` returns 0
  </acceptance_criteria>
  <done>Helper module complete with full unit-test coverage of the 9-case LRANGE matrix, LREM sign matrix, and LEFT/RIGHT/BEFORE/AFTER parsing. Can be imported from lib.rs for the blocking plan.</done>
</task>

<task type="auto" tdd="true">
  <name>Task 3: Store methods for LPUSH/RPUSH + LLEN/LINDEX/LRANGE (read+write foundational commands)</name>
  <read_first>
    - src/store.rs (sadd at lines 697-723 — variadic template; smembers at lines 728-746 — read-only template; xadd at lines 1217-1267 — notify-inside-write-lock template)
    - src/commands/lists.rs (normalize_range_indices — needed for lrange)
    - .planning/phases/14-add-support-for-the-redis-list-data-type-required-commands-l/14-PATTERNS.md (section "src/store.rs — per-command Store methods")
    - .planning/phases/14-add-support-for-the-redis-list-data-type-required-commands-l/14-RESEARCH.md (Pattern 1: LPUSH template)
    - .planning/phases/14-add-support-for-the-redis-list-data-type-required-commands-l/14-CONTEXT.md (D-04, D-10)
  </read_first>
  <behavior>
    - `store.lpush(key, vals)` pushes each value to the HEAD in order (LPUSH k a b c → [c, b, a]), returns new length, fires `list_notify.notify_waiters()` inside the write lock, WRONGTYPE on non-list keys
    - `store.rpush(key, vals)` pushes each value to the TAIL in order (RPUSH k a b c → [a, b, c]), returns new length, fires notify, WRONGTYPE on non-list keys
    - `store.llen(key)` returns 0 for missing key, current length for existing, WRONGTYPE on non-list
    - `store.lindex(key, index)` returns Some(Bytes) on hit (supports negative indices), None on missing key OR out-of-range, WRONGTYPE on non-list
    - `store.lrange(key, start, stop)` returns Vec<Bytes> (possibly empty), uses `normalize_range_indices` for bounds, WRONGTYPE on non-list
  </behavior>
  <action>
Add the following methods to `impl Store` in `src/store.rs`. Place them together in a new `// ---- List Commands ----` section after the stream commands block (for consistency with the existing layout convention).

**LPUSH** (template from sadd at 697-723 + xadd notify at 1259-1263):
```rust
pub fn lpush(&self, key: Bytes, values: Vec<Bytes>) -> Result<i64, StoreError> {
    let mut data = self.data.write();

    // Passive expiration
    if let Some(entry) = data.get(&key) {
        if entry.is_expired() {
            data.remove(&key);
        }
    }

    let entry = data.entry(key).or_insert_with(ValueEntry::new_list);
    match entry.data {
        ValueData::List(ref mut list) => {
            // redis-py semantics: LPUSH k a b c → [c, b, a]
            // Each value is pushed in turn to the head.
            for v in values {
                list.push_front(v);
            }
            let len = list.len() as i64;
            // Wake blocking BRPOP/BLPOP waiters (D-10) — inside write lock.
            self.list_notify.notify_waiters();
            Ok(len)
        }
        _ => Err(StoreError::WrongType),
    }
}
```

**RPUSH** — same but `list.push_back(v)`:
```rust
pub fn rpush(&self, key: Bytes, values: Vec<Bytes>) -> Result<i64, StoreError> {
    let mut data = self.data.write();
    if let Some(entry) = data.get(&key) {
        if entry.is_expired() { data.remove(&key); }
    }
    let entry = data.entry(key).or_insert_with(ValueEntry::new_list);
    match entry.data {
        ValueData::List(ref mut list) => {
            for v in values { list.push_back(v); }
            let len = list.len() as i64;
            self.list_notify.notify_waiters();
            Ok(len)
        }
        _ => Err(StoreError::WrongType),
    }
}
```

**LLEN** (template from smembers-style read-only at 728-746):
```rust
pub fn llen(&self, key: &Bytes) -> Result<i64, StoreError> {
    let data = self.data.read();
    match data.get(key) {
        None => Ok(0),
        Some(entry) if entry.is_expired() => Ok(0),
        Some(entry) => match &entry.data {
            ValueData::List(list) => Ok(list.len() as i64),
            _ => Err(StoreError::WrongType),
        },
    }
}
```

**LINDEX:**
```rust
pub fn lindex(&self, key: &Bytes, index: i64) -> Result<Option<Bytes>, StoreError> {
    let data = self.data.read();
    let entry = match data.get(key) {
        None => return Ok(None),
        Some(e) if e.is_expired() => return Ok(None),
        Some(e) => e,
    };
    let list = match &entry.data {
        ValueData::List(l) => l,
        _ => return Err(StoreError::WrongType),
    };
    let n = list.len() as i64;
    let actual = if index < 0 { index + n } else { index };
    if actual < 0 || actual >= n {
        return Ok(None);
    }
    Ok(list.get(actual as usize).cloned())
}
```

**LRANGE:**
```rust
pub fn lrange(&self, key: &Bytes, start: i64, stop: i64) -> Result<Vec<Bytes>, StoreError> {
    use crate::commands::lists::normalize_range_indices;
    let data = self.data.read();
    let entry = match data.get(key) {
        None => return Ok(Vec::new()),
        Some(e) if e.is_expired() => return Ok(Vec::new()),
        Some(e) => e,
    };
    let list = match &entry.data {
        ValueData::List(l) => l,
        _ => return Err(StoreError::WrongType),
    };
    let (start, end) = match normalize_range_indices(start, stop, list.len()) {
        None => return Ok(Vec::new()),
        Some(pair) => pair,
    };
    // inclusive on both ends
    Ok(list.iter().skip(start).take(end - start + 1).cloned().collect())
}
```

**Write #[cfg(test)] tests in src/store.rs** exercising each method. Minimum tests (use `Bytes::from_static(b"...")` for keys/values):

```rust
#[test]
fn lpush_creates_and_reverses_order() {
    let store = Store::new();
    let key = Bytes::from_static(b"k");
    let n = store.lpush(key.clone(), vec![Bytes::from_static(b"a"), Bytes::from_static(b"b"), Bytes::from_static(b"c")]).unwrap();
    assert_eq!(n, 3);
    let elems = store.lrange(&key, 0, -1).unwrap();
    assert_eq!(elems, vec![Bytes::from_static(b"c"), Bytes::from_static(b"b"), Bytes::from_static(b"a")]);
}

#[test]
fn rpush_creates_and_preserves_order() {
    let store = Store::new();
    let key = Bytes::from_static(b"k");
    let n = store.rpush(key.clone(), vec![Bytes::from_static(b"a"), Bytes::from_static(b"b"), Bytes::from_static(b"c")]).unwrap();
    assert_eq!(n, 3);
    let elems = store.lrange(&key, 0, -1).unwrap();
    assert_eq!(elems, vec![Bytes::from_static(b"a"), Bytes::from_static(b"b"), Bytes::from_static(b"c")]);
}

#[test]
fn llen_missing_is_zero() {
    let store = Store::new();
    assert_eq!(store.llen(&Bytes::from_static(b"missing")).unwrap(), 0);
}

#[test]
fn lindex_negative_and_out_of_range() {
    let store = Store::new();
    let key = Bytes::from_static(b"k");
    store.rpush(key.clone(), vec![Bytes::from_static(b"a"), Bytes::from_static(b"b"), Bytes::from_static(b"c")]).unwrap();
    assert_eq!(store.lindex(&key, 0).unwrap(), Some(Bytes::from_static(b"a")));
    assert_eq!(store.lindex(&key, -1).unwrap(), Some(Bytes::from_static(b"c")));
    assert_eq!(store.lindex(&key, 100).unwrap(), None);
    assert_eq!(store.lindex(&key, -100).unwrap(), None);
}

#[test]
fn lpush_on_string_key_returns_wrongtype() {
    let store = Store::new();
    let key = Bytes::from_static(b"strkey");
    store.set(key.clone(), Bytes::from_static(b"val"), None, false, false).unwrap();
    let err = store.lpush(key, vec![Bytes::from_static(b"a")]).unwrap_err();
    assert!(matches!(err, StoreError::WrongType));
}

#[test]
fn lpush_notifies_waiters() {
    // Integration: call lpush, observe list_notify fires. We can't easily await a Notify in a
    // sync test, but we can verify the method returns without panicking and list is populated.
    let store = Store::new();
    let key = Bytes::from_static(b"k");
    store.lpush(key.clone(), vec![Bytes::from_static(b"v")]).unwrap();
    assert_eq!(store.llen(&key).unwrap(), 1);
}
```
(Adjust the `store.set` call signature to match the existing `set` method — if set's arity differs, use the closest existing WRONGTYPE test in `src/store.rs` as the literal template.)
  </action>
  <verify>
    <automated>cargo build --lib 2>&1 | tail -10; cargo test --lib store::tests::lpush store::tests::rpush store::tests::llen store::tests::lindex store::tests::lpush_on_string_key store::tests::lpush_notifies 2>&1 | tee /tmp/phase14-task3.log | tail -30 && echo PASS-TASK3</automated>
  </verify>
  <acceptance_criteria>
    - `cargo build --lib` exits 0
    - `cargo test --lib store::tests::lpush_creates_and_reverses_order store::tests::rpush_creates_and_preserves_order store::tests::llen_missing_is_zero store::tests::lindex_negative_and_out_of_range store::tests::lpush_on_string_key_returns_wrongtype store::tests::lpush_notifies_waiters -- --exact` shows 6 passing tests
    - `grep -qE "pub fn lpush\\s*\\(" src/store.rs` returns 0
    - `grep -qE "pub fn rpush\\s*\\(" src/store.rs` returns 0
    - `grep -qE "pub fn llen\\s*\\(" src/store.rs` returns 0
    - `grep -qE "pub fn lindex\\s*\\(" src/store.rs` returns 0
    - `grep -qE "pub fn lrange\\s*\\(" src/store.rs` returns 0
    - Inside `lpush` AND `rpush`, `self.list_notify.notify_waiters()` appears before `Ok(`
  </acceptance_criteria>
  <done>5 store methods (lpush, rpush, llen, lindex, lrange) with unit tests. All WRONGTYPE + notify semantics proven. Foundation for blocking layer and Python surface.</done>
</task>

<task type="auto" tdd="true">
  <name>Task 4: Store methods for LPOP/RPOP/LREM/LSET/LTRIM/LINSERT (mutating ops + empty-list-deletes-key)</name>
  <read_first>
    - src/store.rs (the lpush/rpush/llen added in Task 3 as pattern baseline; also srem at ~line 774+ for delete-empty-after-mutation pattern)
    - src/commands/lists.rs (LremDirection, InsertPosition — used by lrem/linsert)
    - .planning/phases/14-add-support-for-the-redis-list-data-type-required-commands-l/14-RESEARCH.md (Pattern 3: LREM count-sign; Pattern 4: LPOP count semantics; Code Examples block; Pitfall 4: LPOP count=0)
    - .planning/phases/14-add-support-for-the-redis-list-data-type-required-commands-l/14-CONTEXT.md (D-02, D-03)
  </read_first>
  <behavior>
    - `store.lpop(key, count=None)` — `LPopResult::Nil` on missing key; `LPopResult::Single(Bytes)` when count is None and list non-empty; `LPopResult::Array(vec)` when count is `Some(n)` (including `Some(0)` → empty vec); deletes key when list becomes empty (D-03); WRONGTYPE on non-list
    - `store.rpop(key, count=None)` — mirror of lpop but pops from back
    - `store.lrem(key, count, value)` — implements LremDirection::Head(n)/Tail(n)/All semantics from parse_lrem_count; deletes key if empty after (D-03); returns removed count; WRONGTYPE on non-list; returns 0 on missing key (NOT an error)
    - `store.lset(key, index, value)` — replaces at index; returns StoreError::Syntax("ERR index out of range") on out-of-range; StoreError::NoSuchKey (or equivalent existing variant) on missing key; WRONGTYPE on non-list
    - `store.ltrim(key, start, stop)` — uses `normalize_range_indices`; if normalized returns `None`, delete the key; otherwise truncate to the inclusive range and delete key if empty; WRONGTYPE on non-list
    - `store.linsert(key, where, pivot, value)` — returns new length on insert, -1 on pivot-not-found, 0 on missing key; WRONGTYPE on non-list
  </behavior>
  <action>
Add to `impl Store` in `src/store.rs`:

**Define `pub enum LPopResult` at the top of the list commands section:**
```rust
/// Return variants for LPOP/RPOP — mirrors redis-py semantics:
/// - Nil: missing key (with or without count)
/// - Single: count was None and a single element was popped
/// - Array: count was Some(n), possibly empty (count=0 → empty array)
pub enum LPopResult {
    Nil,
    Single(Bytes),
    Array(Vec<Bytes>),
}
```

**LPOP** (per RESEARCH.md Pattern 4 — type-check FIRST, count=0 fast-return ONLY after type check):
```rust
pub fn lpop(&self, key: &Bytes, count: Option<usize>) -> Result<LPopResult, StoreError> {
    let mut data = self.data.write();

    // Passive expiration
    if let Some(entry) = data.get(key) {
        if entry.is_expired() {
            data.remove(key);
        }
    }

    // Type-check against the existing entry (if any)
    match data.get(key) {
        None => return Ok(LPopResult::Nil),
        Some(entry) => match &entry.data {
            ValueData::List(_) => {}  // proceed
            _ => return Err(StoreError::WrongType),
        },
    }

    // Type-confirmed. Now handle count=0 fast-return.
    if count == Some(0) {
        return Ok(LPopResult::Array(Vec::new()));
    }

    let entry = data.get_mut(key).unwrap();
    let list = match &mut entry.data {
        ValueData::List(l) => l,
        _ => unreachable!(),
    };

    let result = match count {
        None => {
            // list may never be empty here because empty lists are deleted by D-03 —
            // but defensively:
            match list.pop_front() {
                Some(v) => LPopResult::Single(v),
                None => LPopResult::Nil,
            }
        }
        Some(n) => {
            let actual = n.min(list.len());
            let popped: Vec<Bytes> = (0..actual).map(|_| list.pop_front().unwrap()).collect();
            LPopResult::Array(popped)
        }
    };

    // D-03: delete key if empty
    if let Some(entry) = data.get(key) {
        if let ValueData::List(l) = &entry.data {
            if l.is_empty() {
                data.remove(key);
            }
        }
    }
    Ok(result)
}
```

**RPOP** — mirror of lpop but using `pop_back()` instead of `pop_front()`.

**LREM** (per RESEARCH.md Pattern 3):
```rust
pub fn lrem(&self, key: &Bytes, count: i64, value: Bytes) -> Result<i64, StoreError> {
    use crate::commands::lists::{parse_lrem_count, LremDirection};
    let mut data = self.data.write();

    if let Some(entry) = data.get(key) {
        if entry.is_expired() { data.remove(key); }
    }

    let entry = match data.get_mut(key) {
        None => return Ok(0),
        Some(e) => e,
    };
    let list = match &mut entry.data {
        ValueData::List(l) => l,
        _ => return Err(StoreError::WrongType),
    };

    let mut removed: i64 = 0;
    match parse_lrem_count(count) {
        LremDirection::Head(target) => {
            list.retain(|v| {
                if (removed as usize) < target && v == &value {
                    removed += 1;
                    false
                } else {
                    true
                }
            });
        }
        LremDirection::Tail(target) => {
            let indices: Vec<usize> = list.iter().enumerate()
                .rev()
                .filter_map(|(i, v)| if v == &value { Some(i) } else { None })
                .take(target)
                .collect();
            for i in indices {  // descending order — safe to remove in sequence
                list.remove(i);
                removed += 1;
            }
        }
        LremDirection::All => {
            let before = list.len();
            list.retain(|v| v != &value);
            removed = (before - list.len()) as i64;
        }
    }

    if list.is_empty() {
        data.remove(key);
    }
    Ok(removed)
}
```

**LSET:**
```rust
pub fn lset(&self, key: &Bytes, index: i64, value: Bytes) -> Result<(), StoreError> {
    let mut data = self.data.write();

    if let Some(entry) = data.get(key) {
        if entry.is_expired() { data.remove(key); }
    }

    let entry = match data.get_mut(key) {
        None => return Err(StoreError::NoSuchKey),  // or whichever variant matches your enum
        Some(e) => e,
    };
    let list = match &mut entry.data {
        ValueData::List(l) => l,
        _ => return Err(StoreError::WrongType),
    };
    let n = list.len() as i64;
    let actual = if index < 0 { index + n } else { index };
    if actual < 0 || actual >= n {
        return Err(StoreError::Syntax("ERR index out of range".to_string()));
    }
    list[actual as usize] = value;
    Ok(())
}
```
If `StoreError` has no `NoSuchKey` variant, either (a) add one (`#[error("ERR no such key")] NoSuchKey`) OR (b) use `StoreError::Syntax("ERR no such key".to_string())` as a workaround. Match existing convention.

**LTRIM:**
```rust
pub fn ltrim(&self, key: &Bytes, start: i64, stop: i64) -> Result<(), StoreError> {
    use crate::commands::lists::normalize_range_indices;
    let mut data = self.data.write();

    if let Some(entry) = data.get(key) {
        if entry.is_expired() { data.remove(key); }
    }

    let entry = match data.get_mut(key) {
        None => return Ok(()),  // missing key is a no-op, not an error
        Some(e) => e,
    };
    let list = match &mut entry.data {
        ValueData::List(l) => l,
        _ => return Err(StoreError::WrongType),
    };
    let len = list.len();
    match normalize_range_indices(start, stop, len) {
        None => {
            // Empty result: delete the key (D-03)
            data.remove(key);
        }
        Some((s, e)) => {
            // Keep only elements [s..=e]
            let new_list: VecDeque<Bytes> = list.iter().skip(s).take(e - s + 1).cloned().collect();
            *list = new_list;
            if list.is_empty() {
                data.remove(key);
            }
        }
    }
    Ok(())
}
```

**LINSERT:**
```rust
pub fn linsert(
    &self,
    key: &Bytes,
    where_: crate::commands::lists::InsertPosition,
    pivot: &Bytes,
    value: Bytes,
) -> Result<i64, StoreError> {
    use crate::commands::lists::InsertPosition;
    let mut data = self.data.write();

    if let Some(entry) = data.get(key) {
        if entry.is_expired() { data.remove(key); }
    }

    let entry = match data.get_mut(key) {
        None => return Ok(0),  // missing key → 0
        Some(e) => e,
    };
    let list = match &mut entry.data {
        ValueData::List(l) => l,
        _ => return Err(StoreError::WrongType),
    };
    let pos = match list.iter().position(|v| v == pivot) {
        None => return Ok(-1),  // pivot not found
        Some(p) => p,
    };
    let insert_at = match where_ {
        InsertPosition::Before => pos,
        InsertPosition::After => pos + 1,
    };
    list.insert(insert_at, value);
    Ok(list.len() as i64)
}
```

**Unit tests** — add to `#[cfg(test)] mod tests` in `src/store.rs`:
```rust
#[test]
fn lpop_no_count_single() {
    let store = Store::new();
    let k = Bytes::from_static(b"k");
    store.rpush(k.clone(), vec![Bytes::from_static(b"a"), Bytes::from_static(b"b")]).unwrap();
    match store.lpop(&k, None).unwrap() {
        LPopResult::Single(v) => assert_eq!(v, Bytes::from_static(b"a")),
        other => panic!("expected Single, got {:?}", matches!(other, LPopResult::Nil)),
    }
}
#[test]
fn lpop_count_zero_returns_empty_array() {
    let store = Store::new();
    let k = Bytes::from_static(b"k");
    store.rpush(k.clone(), vec![Bytes::from_static(b"a")]).unwrap();
    match store.lpop(&k, Some(0)).unwrap() {
        LPopResult::Array(v) => assert_eq!(v.len(), 0),
        _ => panic!("expected empty Array for count=0"),
    }
}
#[test]
fn lpop_missing_key_is_nil() {
    let store = Store::new();
    assert!(matches!(store.lpop(&Bytes::from_static(b"missing"), None).unwrap(), LPopResult::Nil));
    assert!(matches!(store.lpop(&Bytes::from_static(b"missing"), Some(5)).unwrap(), LPopResult::Nil));
}
#[test]
fn lpop_deletes_key_when_empty() {
    let store = Store::new();
    let k = Bytes::from_static(b"k");
    store.rpush(k.clone(), vec![Bytes::from_static(b"a")]).unwrap();
    store.lpop(&k, None).unwrap();
    assert_eq!(store.llen(&k).unwrap(), 0);
    // Verify key is actually deleted, not just empty:
    assert!(matches!(store.lpop(&k, None).unwrap(), LPopResult::Nil));
}
#[test]
fn lrem_head_negative_all() {
    let store = Store::new();
    let k = Bytes::from_static(b"k");
    let a = Bytes::from_static(b"a");
    store.rpush(k.clone(), vec![a.clone(), Bytes::from_static(b"b"), a.clone(), Bytes::from_static(b"c"), a.clone()]).unwrap();
    // count=2 (head→tail): remove first 2 'a's
    assert_eq!(store.lrem(&k, 2, a.clone()).unwrap(), 2);
    assert_eq!(store.lrange(&k, 0, -1).unwrap(), vec![Bytes::from_static(b"b"), Bytes::from_static(b"c"), a.clone()]);
    // count=-1: remove last remaining 'a'
    assert_eq!(store.lrem(&k, -1, a.clone()).unwrap(), 1);
    // count=0 on clean list: remove all 'z's (zero found)
    assert_eq!(store.lrem(&k, 0, Bytes::from_static(b"z")).unwrap(), 0);
}
#[test]
fn lset_out_of_range() {
    let store = Store::new();
    let k = Bytes::from_static(b"k");
    store.rpush(k.clone(), vec![Bytes::from_static(b"a")]).unwrap();
    let err = store.lset(&k, 5, Bytes::from_static(b"v")).unwrap_err();
    // must contain "index out of range"
    assert!(format!("{}", err).contains("index out of range"));
}
#[test]
fn ltrim_empty_result_deletes_key() {
    let store = Store::new();
    let k = Bytes::from_static(b"k");
    store.rpush(k.clone(), vec![Bytes::from_static(b"a"), Bytes::from_static(b"b"), Bytes::from_static(b"c")]).unwrap();
    store.ltrim(&k, 5, 10).unwrap();  // out-of-range → empty
    assert_eq!(store.llen(&k).unwrap(), 0);
}
#[test]
fn linsert_pivot_behavior() {
    let store = Store::new();
    let k = Bytes::from_static(b"k");
    store.rpush(k.clone(), vec![Bytes::from_static(b"a"), Bytes::from_static(b"c")]).unwrap();
    // Insert BEFORE "c" → [a, b, c]
    assert_eq!(store.linsert(&k, crate::commands::lists::InsertPosition::Before, &Bytes::from_static(b"c"), Bytes::from_static(b"b")).unwrap(), 3);
    assert_eq!(store.lrange(&k, 0, -1).unwrap(), vec![Bytes::from_static(b"a"), Bytes::from_static(b"b"), Bytes::from_static(b"c")]);
    // Pivot not found → -1
    assert_eq!(store.linsert(&k, crate::commands::lists::InsertPosition::Before, &Bytes::from_static(b"z"), Bytes::from_static(b"w")).unwrap(), -1);
    // Missing key → 0
    assert_eq!(store.linsert(&Bytes::from_static(b"missing"), crate::commands::lists::InsertPosition::After, &Bytes::from_static(b"a"), Bytes::from_static(b"b")).unwrap(), 0);
}
```
  </action>
  <verify>
    <automated>cargo build --lib 2>&1 | tail -10; cargo test --lib store::tests::lpop store::tests::lrem store::tests::lset store::tests::ltrim store::tests::linsert 2>&1 | tee /tmp/phase14-task4.log | tail -40 && echo PASS-TASK4</automated>
  </verify>
  <acceptance_criteria>
    - `cargo build --lib` exits 0
    - `cargo test --lib store::tests::lpop_no_count_single store::tests::lpop_count_zero_returns_empty_array store::tests::lpop_missing_key_is_nil store::tests::lpop_deletes_key_when_empty store::tests::lrem_head_negative_all store::tests::lset_out_of_range store::tests::ltrim_empty_result_deletes_key store::tests::linsert_pivot_behavior -- --exact` shows 8 passing tests
    - `grep -qE "pub enum LPopResult" src/store.rs` returns 0
    - All 6 methods (lpop, rpop, lrem, lset, ltrim, linsert) present as `pub fn`
  </acceptance_criteria>
  <done>6 mutating store methods + LPopResult enum + 8 unit tests. Empty-list-deletes-key (D-03) verified. LSET out-of-range error message matches redis-py parity text.</done>
</task>

<task type="auto" tdd="true">
  <name>Task 5: Store methods for LMOVE/RPOPLPUSH (cross-key atomic moves) + blpop_poll/brpop_poll (non-blocking multi-key scan helpers)</name>
  <read_first>
    - src/store.rs (lpush/rpush/lpop added in Tasks 3-4 — pattern baseline)
    - src/commands/lists.rs (ListEnd)
    - .planning/phases/14-add-support-for-the-redis-list-data-type-required-commands-l/14-RESEARCH.md (BLMOVE under a single write lock; Multi-key scan order; Pitfall 5 key-order-bug)
    - .planning/phases/14-add-support-for-the-redis-list-data-type-required-commands-l/14-CONTEXT.md (D-09, D-10 wake sites)
    - .planning/phases/14-add-support-for-the-redis-list-data-type-required-commands-l/14-PATTERNS.md (BLMOVE cross-key atomicity section)
  </read_first>
  <behavior>
    - `store.lmove_atomic(src, dst, src_from, dst_to)` — atomic pop-from-src + push-to-dst under single write lock; src==dst valid (rotation); returns `Option<Bytes>` (None on missing/empty src); fires `list_notify.notify_waiters()` when push occurs; deletes src if empty after pop (D-03); WRONGTYPE if either key is wrong type
    - `store.rpoplpush_atomic(src, dst)` — equivalent to `lmove_atomic(src, dst, Right, Left)`
    - `store.blpop_poll(&[keys])` — scans keys LEFT-TO-RIGHT; pops from FIRST non-empty list (from head for BLPOP, from tail for BRPOP); returns `Option<(Bytes, Bytes)>`; deletes key if empty after pop (D-03); WRONGTYPE on first non-list key encountered (abort scan)
    - `store.brpop_poll(&[keys])` — mirror but pops from back
  </behavior>
  <action>
Add to `impl Store` in `src/store.rs`:

**LMOVE atomic:**
```rust
pub fn lmove_atomic(
    &self,
    src: &Bytes,
    dst: &Bytes,
    src_from: crate::commands::lists::ListEnd,
    dst_to: crate::commands::lists::ListEnd,
) -> Result<Option<Bytes>, StoreError> {
    use crate::commands::lists::ListEnd;
    let mut data = self.data.write();

    // Passive expiration (both keys)
    for k in [src, dst].iter() {
        if let Some(e) = data.get(*k) {
            if e.is_expired() { data.remove(*k); }
        }
    }

    // Type-check destination BEFORE mutating source (matches Redis order)
    if let Some(dst_entry) = data.get(dst) {
        if !matches!(dst_entry.data, ValueData::List(_)) {
            return Err(StoreError::WrongType);
        }
    }

    // Pop from source
    let popped = {
        let src_entry = match data.get_mut(src) {
            None => return Ok(None),
            Some(e) => e,
        };
        let src_list = match &mut src_entry.data {
            ValueData::List(l) => l,
            _ => return Err(StoreError::WrongType),
        };
        let val = match src_from {
            ListEnd::Left => src_list.pop_front(),
            ListEnd::Right => src_list.pop_back(),
        };
        if val.is_none() {
            return Ok(None);
        }
        // D-03: delete src if empty
        let src_empty = src_list.is_empty();
        drop(src_entry);  // release mutable borrow
        if src_empty {
            data.remove(src);
        }
        val.unwrap()
    };

    // Push onto destination
    let dst_entry = data.entry(dst.clone()).or_insert_with(ValueEntry::new_list);
    match &mut dst_entry.data {
        ValueData::List(l) => {
            match dst_to {
                ListEnd::Left => l.push_front(popped.clone()),
                ListEnd::Right => l.push_back(popped.clone()),
            }
        }
        _ => return Err(StoreError::WrongType),  // type-checked above but defensive
    }

    // Wake waiters (D-10)
    self.list_notify.notify_waiters();
    Ok(Some(popped))
}
```

**Note on the `drop(src_entry)` issue:** Because the borrow checker won't let you hold a mutable borrow on `src_entry` while also calling `data.remove(src)`, restructure the block to compute `src_empty` then drop the borrow BEFORE the `data.remove` call. Shown as `drop(src_entry);` above — but Rust may need the block to actually end instead. Use an inner scope `{ let src_entry = ...; ... }` if necessary. The mwhile idiomatic way:
```rust
let (popped_opt, src_empty) = {
    let src_entry = match data.get_mut(src) { ... };
    let src_list = match &mut src_entry.data { ... };
    let val = match src_from {
        ListEnd::Left => src_list.pop_front(),
        ListEnd::Right => src_list.pop_back(),
    };
    let empty = src_list.is_empty();
    (val, empty)
};
let popped = match popped_opt {
    None => return Ok(None),
    Some(v) => v,
};
if src_empty { data.remove(src); }
// ... then push
```

**RPOPLPUSH atomic** — thin wrapper:
```rust
pub fn rpoplpush_atomic(&self, src: &Bytes, dst: &Bytes) -> Result<Option<Bytes>, StoreError> {
    use crate::commands::lists::ListEnd;
    self.lmove_atomic(src, dst, ListEnd::Right, ListEnd::Left)
}
```

**blpop_poll** (LEFT-scan, pop FRONT):
```rust
pub fn blpop_poll(&self, keys: &[Bytes]) -> Result<Option<(Bytes, Bytes)>, StoreError> {
    let mut data = self.data.write();
    for k in keys {
        // passive expire
        if let Some(e) = data.get(k) {
            if e.is_expired() { data.remove(k); }
        }
        let entry = match data.get_mut(k) {
            None => continue,
            Some(e) => e,
        };
        let list = match &mut entry.data {
            ValueData::List(l) => l,
            _ => return Err(StoreError::WrongType),  // abort scan on first wrong type
        };
        if let Some(v) = list.pop_front() {
            if list.is_empty() {
                data.remove(k);
            }
            return Ok(Some((k.clone(), v)));
        }
    }
    Ok(None)
}
```

**brpop_poll** — mirror but `pop_back()`:
```rust
pub fn brpop_poll(&self, keys: &[Bytes]) -> Result<Option<(Bytes, Bytes)>, StoreError> {
    let mut data = self.data.write();
    for k in keys {
        if let Some(e) = data.get(k) {
            if e.is_expired() { data.remove(k); }
        }
        let entry = match data.get_mut(k) {
            None => continue,
            Some(e) => e,
        };
        let list = match &mut entry.data {
            ValueData::List(l) => l,
            _ => return Err(StoreError::WrongType),
        };
        if let Some(v) = list.pop_back() {
            if list.is_empty() {
                data.remove(k);
            }
            return Ok(Some((k.clone(), v)));
        }
    }
    Ok(None)
}
```

**Unit tests:**
```rust
#[test]
fn lmove_cross_key_atomic() {
    let store = Store::new();
    let src = Bytes::from_static(b"src");
    let dst = Bytes::from_static(b"dst");
    store.rpush(src.clone(), vec![Bytes::from_static(b"a"), Bytes::from_static(b"b"), Bytes::from_static(b"c")]).unwrap();
    // LMOVE src dst LEFT RIGHT — pops "a" from front of src, pushes to back of dst
    let moved = store.lmove_atomic(&src, &dst, crate::commands::lists::ListEnd::Left, crate::commands::lists::ListEnd::Right).unwrap();
    assert_eq!(moved, Some(Bytes::from_static(b"a")));
    assert_eq!(store.lrange(&src, 0, -1).unwrap(), vec![Bytes::from_static(b"b"), Bytes::from_static(b"c")]);
    assert_eq!(store.lrange(&dst, 0, -1).unwrap(), vec![Bytes::from_static(b"a")]);
}

#[test]
fn lmove_same_key_rotation() {
    let store = Store::new();
    let k = Bytes::from_static(b"k");
    store.rpush(k.clone(), vec![Bytes::from_static(b"a"), Bytes::from_static(b"b"), Bytes::from_static(b"c")]).unwrap();
    // RIGHT → LEFT rotates tail to head: [c, a, b]
    let moved = store.lmove_atomic(&k, &k, crate::commands::lists::ListEnd::Right, crate::commands::lists::ListEnd::Left).unwrap();
    assert_eq!(moved, Some(Bytes::from_static(b"c")));
    assert_eq!(store.lrange(&k, 0, -1).unwrap(), vec![Bytes::from_static(b"c"), Bytes::from_static(b"a"), Bytes::from_static(b"b")]);
}

#[test]
fn lmove_empty_source_returns_none() {
    let store = Store::new();
    assert_eq!(store.lmove_atomic(&Bytes::from_static(b"missing"), &Bytes::from_static(b"dst"), crate::commands::lists::ListEnd::Left, crate::commands::lists::ListEnd::Right).unwrap(), None);
}

#[test]
fn rpoplpush_equivalent_to_lmove() {
    let store = Store::new();
    let src = Bytes::from_static(b"src");
    let dst = Bytes::from_static(b"dst");
    store.rpush(src.clone(), vec![Bytes::from_static(b"a"), Bytes::from_static(b"b")]).unwrap();
    let moved = store.rpoplpush_atomic(&src, &dst).unwrap();
    assert_eq!(moved, Some(Bytes::from_static(b"b")));
    assert_eq!(store.lrange(&dst, 0, -1).unwrap(), vec![Bytes::from_static(b"b")]);
}

#[test]
fn blpop_poll_scans_left_to_right() {
    let store = Store::new();
    store.rpush(Bytes::from_static(b"k2"), vec![Bytes::from_static(b"v2")]).unwrap();
    store.rpush(Bytes::from_static(b"k4"), vec![Bytes::from_static(b"v4")]).unwrap();
    let keys = vec![
        Bytes::from_static(b"k1"), Bytes::from_static(b"k2"),
        Bytes::from_static(b"k3"), Bytes::from_static(b"k4"),
    ];
    let result = store.blpop_poll(&keys).unwrap();
    assert_eq!(result, Some((Bytes::from_static(b"k2"), Bytes::from_static(b"v2"))));
    // k4 must still have its value
    assert_eq!(store.llen(&Bytes::from_static(b"k4")).unwrap(), 1);
}

#[test]
fn blpop_poll_all_empty_returns_none() {
    let store = Store::new();
    let keys = vec![Bytes::from_static(b"k1"), Bytes::from_static(b"k2")];
    assert_eq!(store.blpop_poll(&keys).unwrap(), None);
}

#[test]
fn brpop_poll_pops_from_tail() {
    let store = Store::new();
    let k = Bytes::from_static(b"k");
    store.rpush(k.clone(), vec![Bytes::from_static(b"a"), Bytes::from_static(b"b"), Bytes::from_static(b"c")]).unwrap();
    let result = store.brpop_poll(&[k.clone()]).unwrap();
    assert_eq!(result, Some((k, Bytes::from_static(b"c"))));
}

#[test]
fn blpop_poll_wrongtype_aborts_scan() {
    let store = Store::new();
    store.set(Bytes::from_static(b"s"), Bytes::from_static(b"v"), None, false, false).unwrap();
    let keys = vec![Bytes::from_static(b"s"), Bytes::from_static(b"k")];
    let err = store.blpop_poll(&keys).unwrap_err();
    assert!(matches!(err, StoreError::WrongType));
}
```
(If the `store.set` call signature differs, use the matching existing style from prior tests.)
  </action>
  <verify>
    <automated>cargo build --lib 2>&1 | tail -10; cargo test --lib store::tests::lmove store::tests::rpoplpush store::tests::blpop_poll store::tests::brpop_poll 2>&1 | tee /tmp/phase14-task5.log | tail -40 && echo PASS-TASK5</automated>
  </verify>
  <acceptance_criteria>
    - `cargo build --lib` exits 0
    - `cargo test --lib store::tests::lmove_cross_key_atomic store::tests::lmove_same_key_rotation store::tests::lmove_empty_source_returns_none store::tests::rpoplpush_equivalent_to_lmove store::tests::blpop_poll_scans_left_to_right store::tests::blpop_poll_all_empty_returns_none store::tests::brpop_poll_pops_from_tail store::tests::blpop_poll_wrongtype_aborts_scan -- --exact` shows 8 passing tests
    - `grep -qE "pub fn lmove_atomic\\s*\\(" src/store.rs` returns 0
    - `grep -qE "pub fn blpop_poll\\s*\\(" src/store.rs` returns 0
    - `grep -qE "pub fn brpop_poll\\s*\\(" src/store.rs` returns 0
    - Inside `lmove_atomic`, `self.list_notify.notify_waiters()` fires when push succeeds
  </acceptance_criteria>
  <done>Cross-key atomic move methods + polling helpers for the blocking layer. BLPOP left-to-right scan order verified. All WRONGTYPE and empty-source semantics correct.</done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|----------|-------------|
| Python caller → Rust PyO3 pymethod (touched indirectly via Store methods in Plan 02) | Python int/str/bytes crosses into Rust `Bytes`; Store layer assumes already-validated bytes |
| Lua script → Store methods (via scripting.rs, Plan 03) | Lua scripts can call LPUSH/etc under an already-acquired write lock; atomicity boundary |
| Background Tokio task → Store (via blocking loops in Plan 02) | Concurrent read/write across BRPOP waiters and LPUSH writers |

## STRIDE Threat Register

| Threat ID | Category | Component | Disposition | Mitigation Plan |
|-----------|----------|-----------|-------------|-----------------|
| T-14-02 | Tampering | Every Store list method | mitigate | Exhaustive `ValueData::List` match in every method; non-list variant → `StoreError::WrongType`. Covered by `lpush_on_string_key_returns_wrongtype`, `blpop_poll_wrongtype_aborts_scan` unit tests. |
| T-14-05 | Tampering | `lmove_atomic`, `rpoplpush_atomic` | mitigate | Single `data.write()` acquisition; pop + push under the same lock scope. Type-check destination BEFORE popping (Redis order). Covered by `lmove_cross_key_atomic` test. |
| T-14-07 (resource) | Denial of Service | `lpush`/`rpush`/`linsert` | accept | Single-process embedded DB — caller IS the trust boundary per REQUIREMENTS.md §Out of Scope ("runs in-process, no auth boundary"). Informational only. |
| T-14-08 (input-validation) | Tampering | `parse_list_end`, `parse_linsert_where`, `lrem`, `lset` | mitigate | All parse-helpers return `StoreError::Syntax` with specific messages on invalid input; LSET out-of-range returns `"ERR index out of range"` matching redis-py ResponseError wording. Covered by `parse_list_end_invalid`, `lset_out_of_range` tests. |
| T-14-09 (wrong-type-from-Lua) | Tampering | Will be mitigated in Plan 03 — `dispatch_command_inner` returns `RedisValue::Error(WRONGTYPE ...)` for Lua callers | mitigate (deferred to Plan 03) | Plan 03 replicates the WRONGTYPE arm from Task 3's `match entry.data` block into each Lua dispatch arm. Tracked here for auditing; implementation in Plan 03. |

No threats with severity=high remain unmitigated. ASVS L1 compliance maintained.
</threat_model>

<verification>
Run the full engine test suite. Expected: all list-related unit tests pass with no regression in existing stream/hash/set/sorted_set tests.

```bash
cargo test --lib store::tests 2>&1 | tail -60
cargo test --lib commands::lists::tests 2>&1 | tail -20
```

Plus the targeted greps in each task's acceptance criteria.
</verification>

<success_criteria>
- `cargo build --lib` exits 0
- All new unit tests pass (expect ~25 list-related tests added across Tasks 1-5)
- No regression in existing tests: `cargo test --lib 2>&1 | grep -E "test result: (ok|FAILED)"` shows only "ok"
- `.planning/REQUIREMENTS.md` has LIST-01..LIST-16 with Phase 14 traceability
- `src/commands/lists.rs` exists and is registered in `src/commands/mod.rs`
- `ValueData::List` and `list_notify` field both visible in `src/store.rs`
- Every list-grow operation (lpush, rpush, lmove_atomic, rpoplpush_atomic) fires `list_notify.notify_waiters()` inside the write lock
- Every list-empty-after-pop operation (lpop, rpop, lrem, ltrim, blpop_poll, brpop_poll, lmove_atomic on src) deletes the key (D-03)
</success_criteria>

<output>
After completion, create `.planning/phases/14-add-support-for-the-redis-list-data-type-required-commands-l/14-01-SUMMARY.md` with:
- Rust store methods added (count: 13 non-blocking + 2 polling helpers + 1 persistence arm)
- Unit test count and cargo test output summary
- REQUIREMENTS.md diff summary (additions + Out of Scope removal)
- Any deviations from the plan (LSET error wording used, NoSuchKey variant decision, etc.)
- Handoff notes for Plan 02 (list of Store method signatures available for the PyO3 layer)
</output>
