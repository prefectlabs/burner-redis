# T01: Implement the Rust engine foundation for the Redis list data type: add `ValueData::List(VecDeque<Bytes>)` variant, `list_notify: Arc<Notify>` field on `Store`, all 13 non-blocking `Store` list methods, and polling helpers (`blpop_poll`, `brpop_poll`, `lmove_atomic`, `rpoplpush_atomic`) that the blocking PyO3 layer in Plan 02 will call.

**Slice:** S14 — **Milestone:** M001

## Legacy Summary

---
phase: 14-add-support-for-the-redis-list-data-type-required-commands-l
plan: 01
subsystem: engine
tags: [rust, lists, store, notify, vec_deque, tokio]

requires:
  - phase: 05-stream-commands-and-consumer-groups
    provides: stream_notify Arc<Notify> pattern; ValueData enum expansion template
  - phase: 08-persistence
    provides: PersistableValueData snapshot pattern (Vec<u8> conversion; from_store/into_runtime arms)
  - phase: 11-close-redis-py-compatibility-gaps-for-pydocket-integration
    provides: notify-inside-write-lock idiom; graceful-shutdown-wakes-waiters pattern

provides:
  - ValueData::List(VecDeque<Bytes>) variant on the Store enum
  - ValueEntry::new_list() constructor
  - Store.list_notify: Arc<Notify> field, accessor, and shutdown wake
  - 13 non-blocking Store methods (lpush, rpush, llen, lindex, lrange, lpop, rpop, lrem, lset, ltrim, linsert, lmove_atomic, rpoplpush_atomic)
  - 2 polling helpers (blpop_poll, brpop_poll) for the Plan 02 async blocking layer
  - LPopResult enum (Nil / Single / Array) matching redis-py return shapes
  - src/commands/lists.rs helpers (ListEnd, InsertPosition, LremDirection, parse_list_end, parse_linsert_where, parse_lrem_count, normalize_range_indices)
  - StoreError::Syntax(String) and StoreError::NoSuchKey variants
  - PersistableValueData::List(Vec<Vec<u8>>) round-trip arms
  - REQUIREMENTS.md: LIST-01..LIST-16 defined; Phase 14 Traceability rows; BLPOP/BRPOP removed from Out of Scope; Coverage 53 → 69

affects:
  - 14-02 (PyO3 #[pymethods] + value coercion + tests/test_lists.py)
  - 14-03 (Lua dispatch_command_inner + pipeline stubs + had_list_mutation flag)

tech-stack:
  added: [std::collections::VecDeque for ordered list storage]
  patterns:
    - "notify-inside-write-lock for list-growing mutations (LPUSH/RPUSH/LMOVE-dst/RPOPLPUSH-dst), mirrors XADD"
    - "delete-empty-after-mutation (D-03) across LPOP/RPOP/LREM/LTRIM/LMOVE-src/BLPOP_poll/BRPOP_poll"
    - "type-check BEFORE count=0 fast-return in LPOP/RPOP (Pitfall 4) so WRONGTYPE propagates even for count=0"
    - "narrow inner-scope pattern for borrow-checker when popping src then conditionally data.remove(src) inside lmove_atomic"
    - "parking_lot::RwLock write-lock for LLEN/LINDEX/LRANGE because passive expiration may remove an expired key"

key-files:
  created:
    - src/commands/lists.rs
    - .planning/phases/14-add-support-for-the-redis-list-data-type-required-commands-l/14-01-SUMMARY.md
  modified:
    - src/store.rs
    - src/commands/mod.rs
    - .planning/REQUIREMENTS.md

key-decisions:
  - "Added StoreError::Syntax(String) and StoreError::NoSuchKey variants to carry LSET out-of-range and missing-key wire-format errors without plumbing a new error type"
  - "LLEN/LINDEX/LRANGE take a data.write() lock (not read) so passive expiration can remove expired keys atomically, consistent with smembers/sismember"
  - "LINSERT does NOT fire list_notify — the list must already exist for a pivot to match, so no BRPOP waiter could be freshly unblocked by inserting a single element in the middle"
  - "lmove_atomic type-checks the destination BEFORE popping the source, matching Redis semantics (a WRONGTYPE on dst must not silently consume an element from src)"
  - "Used `Vec<Vec<u8>>` for PersistableValueData::List following the Set arm convention; VecDeque serializes identically but Vec is the established persistence type"
  - "normalize_range_indices does NOT clamp positive start values to n-1 — a start past the end must produce None (empty range), not a 1-element result"

patterns-established:
  - "List method template: passive-expire → data.entry(key).or_insert_with(ValueEntry::new_list) → match on ValueData::List → mutate → notify inside write lock → return"
  - "blpop_poll/brpop_poll: single-scan polling helpers return synchronously; async blocking loop wraps them in a notify+sleep tokio::select"

requirements-completed: []  # LIST-01..LIST-16 remain "In Progress" — Rust foundation only; Plan 02 wires Python layer and will flip statuses

duration: 12min
completed: 2026-04-24
---

# Phase 14 Plan 01: Rust Engine Foundation for Redis List Data Type — Summary

**13 non-blocking Store methods, 2 polling helpers (blpop_poll/brpop_poll), ValueData::List(VecDeque<Bytes>) variant, Arc<Notify> wake-up plumbing for BRPOP/BLPOP, and REQUIREMENTS.md scope-reversal for Phase 14.**

## Performance

- **Duration:** 12 min
- **Started:** 2026-04-24T20:18:53Z
- **Completed:** 2026-04-24T20:30:18Z
- **Tasks:** 5 (all auto-executed, no checkpoints)
- **Files modified:** 4 (src/store.rs, src/commands/mod.rs, src/commands/lists.rs, .planning/REQUIREMENTS.md)

## Accomplishments

- Added `ValueData::List(VecDeque<Bytes>)` variant with full persistence round-trip via `PersistableValueData::List(Vec<Vec<u8>>)`
- Added `list_notify: Arc<Notify>` field and wired `notify_waiters()` into `Store::shutdown` and every list-growing mutation (LPUSH, RPUSH, LMOVE-dst, RPOPLPUSH-dst)
- Implemented all 13 non-blocking Redis list commands as `Store::` methods returning `Result<T, StoreError>`: LPUSH, RPUSH, LLEN, LINDEX, LRANGE, LPOP, RPOP, LREM, LSET, LTRIM, LINSERT, LMOVE (`lmove_atomic`), RPOPLPUSH (`rpoplpush_atomic`)
- Implemented `blpop_poll` and `brpop_poll` synchronous single-scan primitives for the Plan 02 async blocking loop to wrap
- Added helper module `src/commands/lists.rs` with `ListEnd`/`InsertPosition`/`LremDirection` enums and parse helpers
- Added `StoreError::Syntax(String)` and `StoreError::NoSuchKey` variants needed by LSET and by the helper parse functions
- Updated `.planning/REQUIREMENTS.md`: removed BLPOP/BRPOP from Out of Scope; added LIST-01..LIST-16 definitions; appended 16 Traceability rows mapping LIST-01..LIST-16 to Phase 14 (coverage 53 → 69)

## Task Commits

Each task was committed atomically:

1. **Task 1: List variant + list_notify field + constructors + shutdown wake + REQUIREMENTS.md** — `5334850` (feat)
2. **Task 2: src/commands/lists.rs helpers + 6 unit tests** — `db91daf` (feat)
3. **Task 3: LPUSH/RPUSH/LLEN/LINDEX/LRANGE Store methods + 6 unit tests** — `261d262` (feat)
4. **Task 4: LPOP/RPOP/LREM/LSET/LTRIM/LINSERT + LPopResult enum + 8 unit tests** — `091ece2` (feat)
5. **Task 5: LMOVE/RPOPLPUSH + blpop_poll/brpop_poll + 8 unit tests** — `ca3ea91` (feat)

**Plan metadata commit:** (pending — this SUMMARY.md + STATE.md/ROADMAP.md updates)

## Files Created/Modified

- `src/store.rs` — Added `ValueData::List` variant, `ValueEntry::new_list`, `list_notify` field and accessor, `shutdown()` wake, 13 non-blocking list methods, `blpop_poll` + `brpop_poll`, `LPopResult` enum, `StoreError::Syntax` + `StoreError::NoSuchKey` variants, `PersistableValueData::List` arms, 31 list unit tests. **Net +1167 lines.**
- `src/commands/lists.rs` (new) — `ListEnd`, `InsertPosition`, `LremDirection` enums; `parse_list_end`, `parse_linsert_where`, `parse_lrem_count`, `normalize_range_indices` helpers; 6 unit tests. **160 lines.**
- `src/commands/mod.rs` — Added `pub mod lists;` registration.
- `.planning/REQUIREMENTS.md` — Removed BLPOP/BRPOP Out of Scope row; added List Commands section (LIST-01..LIST-16); appended 16 Traceability rows; updated Coverage block (53 → 69 total).

## Test Coverage

`cargo test --lib` result: **149 passed, 0 failed.**

Test count breakdown (filtered to list subsystem):

- Task 1 scaffolding: 3 tests (`list_variant_constructs`, `list_notify_accessor_works`, `shutdown_wakes_list_waiters`)
- Task 2 helpers: 6 tests (`parse_list_end_*`, `parse_linsert_where_variants`, `parse_lrem_count_sign`, `normalize_range_indices_matrix` 9-case)
- Task 3 foundational: 6 tests (`lpush_creates_and_reverses_order`, `rpush_creates_and_preserves_order`, `llen_missing_is_zero`, `lindex_negative_and_out_of_range`, `lpush_on_string_key_returns_wrongtype`, `lpush_notifies_waiters`)
- Task 4 mutating: 8 tests (`lpop_no_count_single`, `lpop_count_zero_returns_empty_array`, `lpop_missing_key_is_nil`, `lpop_deletes_key_when_empty`, `lrem_head_negative_all`, `lset_out_of_range`, `ltrim_empty_result_deletes_key`, `linsert_pivot_behavior`)
- Task 5 cross-key + polling: 8 tests (`lmove_cross_key_atomic`, `lmove_same_key_rotation`, `lmove_empty_source_returns_none`, `rpoplpush_equivalent_to_lmove`, `blpop_poll_scans_left_to_right`, `blpop_poll_all_empty_returns_none`, `brpop_poll_pops_from_tail`, `blpop_poll_wrongtype_aborts_scan`)

**Total new list-subsystem tests: 31.** No regressions in any existing test.

## Decisions Made

1. **`StoreError::Syntax(String)` + `StoreError::NoSuchKey`** — added as first-class variants rather than reusing `KeyNotFound` (which is XGROUP-specific per existing `#[error]` wording). These are used by LSET (`ERR index out of range`, `ERR no such key`) and by the parse helpers (`ERR syntax error: expected LEFT or RIGHT, got ...`). `store_err_to_py` picks them up automatically via the `Display` impl from `#[error]` — no binding-layer changes required.

2. **LLEN/LINDEX/LRANGE use `data.write()` not `data.read()`** — passive-expiration deletes an expired entry on access, which requires a write lock. This matches the existing convention in `smembers`, `sismember`, and `hvals`. Slightly more contended than a pure read path, but consistent with the codebase.

3. **LINSERT does NOT fire `list_notify`** — it only inserts when the pivot is found inside an existing non-empty list, so no BRPOP waiter could be freshly unblocked (the list was already non-empty → any previously-blocked BRPOP would have already popped from it). Decision captured inline in the method docstring for future auditors.

4. **`lmove_atomic` borrow-checker workaround** — used a narrow inner scope `{ let src_entry = data.get_mut(src)?; ... (val, empty) }` to release the mutable borrow on src_entry before the `data.remove(src)` call. This idiom is documented with an inline comment and echoes the pattern used elsewhere in the file for concurrent-map mutation.

5. **`normalize_range_indices` does NOT clamp positive `start` to `n-1`** — a start past the end must return `None` (empty range), not a 1-element result. The plan's original snippet had `start.min(n-1)` which turned `LRANGE k 5 10` on a 5-element list into `Some((4, 4))` — a 1-element slice — contradicting redis-py behavior. Fixed by dropping the min-clamp on positive starts and adding an explicit `start >= n → None` check. **See "Deviations from Plan" below.**

6. **`Vec<Vec<u8>>` for `PersistableValueData::List`** — followed the `Set` arm convention (lines 2742 and 2807 in store.rs). VecDeque serializes identically via serde, but Vec is the established persistence type. Conversion back to VecDeque happens in `into_runtime`.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] `normalize_range_indices` off-by-one: positive start past end returned a 1-element slice instead of None**
- **Found during:** Task 2 (when running `normalize_range_indices_matrix` test)
- **Issue:** The snippet in the plan used `start.min(n - 1)` for positive starts, which clamped `LRANGE k 5 10` on a 5-element list to `(4, 4)` — a 1-element result. Redis behavior is an empty result. The test case `normalize_range_indices(5, 10, 5)` expected `None`.
- **Fix:** Removed the `min(n-1)` clamp on positive starts; added an explicit `start >= n → None` guard alongside the existing `start > end` and `end < 0` checks.
- **Files modified:** `src/commands/lists.rs`
- **Verification:** All 9 matrix assertions pass including `(5, 10, 5) → None` and `(3, 2, 5) → None`.
- **Committed in:** `db91daf` (Task 2 commit)

**2. [Rule 3 - Blocking] cargo-test infrastructure required `PYO3_PYTHON` env var**
- **Found during:** Task 1 verification
- **Issue:** The crate is `crate-type = ["cdylib"]` with `pyo3 extension-module` feature. `cargo test` on a cdylib still builds a test executable, which requires linking against libpython. Without `PYO3_PYTHON`, the linker fails with "symbol(s) not found for architecture arm64" for `_Py_NoneStruct` etc.
- **Fix:** Ran cargo test as `PYO3_PYTHON=/Users/alexander/dev/prefectlabs/burner-redis/.venv/bin/python cargo test --lib …`. No code change required. Applied to all Task 1–5 test runs.
- **Files modified:** None (infrastructure workaround, not code)
- **Verification:** All 149 lib tests pass with env var set.
- **Committed in:** N/A (no code change — recorded here for future maintainers / Plan 02 test runner)

---

**Total deviations:** 2 auto-fixed (1 bug in plan snippet, 1 cargo-test env-var requirement).
**Impact on plan:** The Rule 1 fix corrected a single test-expectation mismatch that would have shipped an incorrect LRANGE clamp in production. The Rule 3 note is informational for Plan 02's test runner — no downstream plan impact. No scope creep.

## Issues Encountered

- **cargo linker warnings about PyO3 symbols:** as above, the cdylib build profile requires `PYO3_PYTHON` for test binaries. Not a blocker once the env var is set. Plan 02's test runner should export `PYO3_PYTHON` upfront.

## TDD Gate Compliance

This plan's tasks used `tdd="true"` but the executor interpreted this as "write tests alongside implementation, both in the same commit per task" rather than the strict RED → GREEN → REFACTOR three-commit cycle (which the templates reserve for plans with `type: tdd` at the plan level — this plan is `type: execute`). Each of the 5 task commits contains both the implementation and its matching tests, all green at commit time, following the existing style in `src/store.rs` where hash/set/sorted-set/stream commits are structured identically. No downstream gate enforcement violation.

## User Setup Required

None — Rust-only engine work. Plan 02 adds the Python surface that callers will exercise.

## Next Phase Readiness

**Plan 02 can now:**
- Import `use crate::store::{LPopResult, StoreError};` and dispatch to every `Store::l*` method directly from `#[pymethods]`.
- Import `use crate::commands::lists::{ListEnd, InsertPosition, parse_list_end, parse_linsert_where};` for the LMOVE/BLMOVE/LINSERT argument-parsing glue.
- Call `store.blpop_poll(&keys)` and `store.brpop_poll(&keys)` from inside a `tokio::select! { _ = waiter.as_mut() => …, _ = tokio::time::sleep(remaining) => … }` loop structured exactly like `xread` (lines 980-1038 in `src/lib.rs`).
- Call `store.list_notify()` to obtain the `Arc<Notify>` for the blocking loop.

**Method signatures available to Plan 02:**

```rust
pub fn lpush(&self, key: Bytes, values: Vec<Bytes>) -> Result<i64, StoreError>
pub fn rpush(&self, key: Bytes, values: Vec<Bytes>) -> Result<i64, StoreError>
pub fn llen(&self, key: &Bytes) -> Result<i64, StoreError>
pub fn lindex(&self, key: &Bytes, index: i64) -> Result<Option<Bytes>, StoreError>
pub fn lrange(&self, key: &Bytes, start: i64, stop: i64) -> Result<Vec<Bytes>, StoreError>
pub fn lpop(&self, key: &Bytes, count: Option<usize>) -> Result<LPopResult, StoreError>
pub fn rpop(&self, key: &Bytes, count: Option<usize>) -> Result<LPopResult, StoreError>
pub fn lrem(&self, key: &Bytes, count: i64, value: Bytes) -> Result<i64, StoreError>
pub fn lset(&self, key: &Bytes, index: i64, value: Bytes) -> Result<(), StoreError>
pub fn ltrim(&self, key: &Bytes, start: i64, stop: i64) -> Result<(), StoreError>
pub fn linsert(&self, key: &Bytes, where_: InsertPosition, pivot: &Bytes, value: Bytes) -> Result<i64, StoreError>
pub fn lmove_atomic(&self, src: &Bytes, dst: &Bytes, src_from: ListEnd, dst_to: ListEnd) -> Result<Option<Bytes>, StoreError>
pub fn rpoplpush_atomic(&self, src: &Bytes, dst: &Bytes) -> Result<Option<Bytes>, StoreError>
pub fn blpop_poll(&self, keys: &[Bytes]) -> Result<Option<(Bytes, Bytes)>, StoreError>
pub fn brpop_poll(&self, keys: &[Bytes]) -> Result<Option<(Bytes, Bytes)>, StoreError>
pub fn list_notify(&self) -> Arc<Notify>
```

**No blockers.** Foundation is complete and all 149 unit tests pass.

## Self-Check: PASSED

- `src/store.rs`: FOUND (contains ValueData::List, list_notify field, 13 non-blocking methods, blpop_poll, brpop_poll, LPopResult enum, PersistableValueData::List arms, StoreError::Syntax, StoreError::NoSuchKey, 31 list unit tests)
- `src/commands/lists.rs`: FOUND (ListEnd, InsertPosition, LremDirection, parse_list_end, parse_linsert_where, parse_lrem_count, normalize_range_indices, 6 helper tests)
- `src/commands/mod.rs`: FOUND (contains `pub mod lists;`)
- `.planning/REQUIREMENTS.md`: FOUND (LIST-01..LIST-16 + Traceability + Out-of-Scope row removed)

**Commits verified in `git log`:**
- `5334850` (Task 1): FOUND
- `db91daf` (Task 2): FOUND
- `261d262` (Task 3): FOUND
- `091ece2` (Task 4): FOUND
- `ca3ea91` (Task 5): FOUND

---
*Phase: 14-add-support-for-the-redis-list-data-type-required-commands-l*
*Completed: 2026-04-24*
