---
phase: 14-add-support-for-the-redis-list-data-type-required-commands-l
type: code-review
status: findings
reviewed: 2026-04-24
depth: standard
tally:
  critical: 0
  high: 1
  medium: 5
  low: 4
  info: 3
---

# Phase 14 Code Review: Redis List Data Type

**Files reviewed:**
- `src/store.rs`
- `src/commands/lists.rs`
- `src/commands/mod.rs`
- `src/lib.rs`
- `src/scripting.rs`
- `python/burner_redis/__init__.py`
- `python/burner_redis/pipeline.py`
- `tests/test_lists.py`

**Verdict:** No critical correctness bugs in notify/blocking machinery. Tokio `Arc<Notify>` wake-up protocol, empty-list deletion invariants (D-03), type-check-before-mutation orderings, and `count=0` pitfall handling are all implemented correctly. Main findings center on (1) pipeline coercion gap, (2) Lua blocking-reject error wording divergence, (3) spurious `had_list_mutation` flag cases, and (4) test-coverage honesty.

---

## HIGH

### H-01: Pipeline list commands bypass Python-layer value coercion

**Files:** `python/burner_redis/pipeline.py:166-172, 194-204`, `src/lib.rs:3558-3578, 3675-3723`, `python/burner_redis/__init__.py:85-128`

The monkey-patched `_coerced_lpush`/`_coerced_rpush`/`_coerced_lset`/`_coerced_linsert` in `__init__.py` apply `_coerce_value` (int/float → bytes, bool → TypeError, None → TypeError) before the Rust call. The pipeline stubs buffer raw Python values and the Rust `dispatch_pipeline_command` calls `extract_bytes()` directly, which only accepts `str`/`bytes`. Result: `pipe.lpush("k", 42).execute()` raises `TypeError: expected str or bytes`, while `r.lpush("k", 42)` works.

Silent drop-in-compatibility break. No pipeline coercion tests exist (`tests/test_lists.py` pipeline tests use only string values).

**Fix:** Apply `_coerce_value` at the pipeline stubs in `python/burner_redis/pipeline.py` for `lpush`, `rpush`, `lset`, `linsert` (and mirror for `set`). Add pipeline coercion tests.

---

## MEDIUM

### M-01: Lua blocking-reject error wording includes command name — real Redis does not

**File:** `src/scripting.rs:2582-2585`

Emits: `"ERR This Redis command is not allowed from scripts: BLPOP"`. Real Redis: `"This Redis command is not allowed from script"` (no colon, no cmd name). Existing tests match `"not allowed from scripts"` so don't catch the divergence.

### M-02: Blocking-command tests hit the non-blocking fast path

**File:** `tests/test_lists.py:244-259, 282-286`

`test_blpop_returns_tuple_on_success` and `test_brpop_pops_from_tail` push synchronously then pop — they exercise the first non-blocking poll, not the `tokio::select!` wake-up path. Only 3 tests actually exercise the slow-path wake. Add an explicit slow-path test asserting a lower bound on elapsed time.

### M-03: Eager `notify_waiters()` in `lpush`/`rpush` could be optimized to empty→non-empty transitions

**File:** `src/store.rs:3296-3298, and lpush/rpush`

Correctness is fine; `notify_waiters()` with 0 waiters is cheap. But under high push throughput with concurrent BLPOP subscribers, transitioning only on empty→non-empty (via `was_empty = list.is_empty()` before push) would avoid unnecessary waker churn. Defer to perf profiling.

### M-04: `had_list_mutation` fires on non-mutating success cases

**File:** `src/scripting.rs:285-300, 2181-2225`

`dispatch_command` treats any non-`RedisValue::Error` as success, so:
- LINSERT returning -1 (pivot not found) or 0 (key missing) triggers `had_list_mutation=true`
- LMOVE/RPOPLPUSH returning `Nil` (empty src) triggers `had_list_mutation=true`

Spurious but safe wakes. Refine by matching on return value per-command (LPUSH/RPUSH always true on Integer; LINSERT true only on `n > 0`; LMOVE/RPOPLPUSH true only on BulkString).

### M-05: Timing-based tests flaky on loaded CI

**File:** `tests/test_lists.py:317-322, 236-241`

Multiple tests assert `0.05 < elapsed < 0.5` for a 100ms timeout. Upper bound may fail on slow CI. Widen to `< 2.0` or use `asyncio.wait_for` with a generous outer timeout; the lower bound is the meaningful assertion.

---

## LOW

### L-01: `normalize_key_list` rejects iterables that aren't `PySequence`

**File:** `src/lib.rs:292-311`

`frozenset` and custom iterables fail the PySequence downcast and fall through to `extract_bytes`, which rejects them. Rare usage but redis-py accepts these. Add a `try_iter()` fallback.

### L-02: List read ops (LLEN/LINDEX/LRANGE) take a write lock for passive expiration

**File:** `src/store.rs:2823-2898`

Consistent with existing `smembers`/`sismember`/`hvals` pattern. On read-dominated workloads, all reads serialize. Try-upgrade pattern would help; not worth implementing without profile evidence.

### L-03: Three blocking pymethods (BLPOP/BRPOP/BLMOVE) are ~100-line near-duplicates

**File:** `src/lib.rs:2521-2828`

Factor into a helper taking `FnMut() -> Result<Option<T>, StoreError>` poll closure and result formatter. XREAD loop at `src/lib.rs:980-1076` shares the same skeleton. Defer to a refactor phase.

### L-04: Scripting LMOVE/RPOPLPUSH duplicate `Store::lmove_atomic`/`rpoplpush_atomic` logic

**File:** `src/scripting.rs:2412-2577` vs `src/store.rs:3206-3310`

Legitimate architectural constraint (Lua path holds the write lock), but means parallel bug fixes. Extract inner-lock helpers that both paths call. Defer to refactor phase.

---

## INFO

### I-01: Commit lineage is clean

13 commits in `5334850..HEAD` map cleanly to Task 1-5 / Plan 01-03. Planning artifacts committed in `28074b0`.

### I-02: REQUIREMENTS.md matches delivered code

LIST-01..LIST-16 all `[x]` Complete. Traceability rows correct. BLPOP/BRPOP removed from Out of Scope.

### I-03: Test coverage is honest about count (80 tests) but conflates fast/slow paths (see M-02)

Regression guards `test_brpop_wakes_on_lua_lpush`, `test_blpop_wakes_on_lua_rpush`, and `test_pipeline_non_blocking_fast_path_timing` correctly protect the Phase 11 + pipeline-perf wins.

---

## Correctness sanity checks that PASSED

1. `had_list_mutation` 3-tuple propagation `dispatch_command_inner → dispatch_command → LuaEngine::execute → Store::eval/evalsha → list_notify.notify_waiters()`.
2. `notify_waiters()` inside write lock at every growth site.
3. `.enable()` before first poll; `waiter.set(notify.notified()); enable()` re-arm sequence.
4. Type-check before `count=0` fast-return — `LPOP strkey 0` correctly returns WRONGTYPE.
5. Type-check destination before popping source in LMOVE/RPOPLPUSH.
6. Empty-list deletion invariant across LPOP/RPOP/LREM/LTRIM/LMOVE-src/BLPOP/BRPOP.
7. LRANGE normalization matrix correct for all 9 edge cases.
8. BLPOP cancellation safety — Notify and sleep are cancel-safe; no cancel point mid-lock.
9. `ValueData::List(VecDeque<Bytes>)` persistence round-trip symmetric via `PersistableValueData::List(Vec<Vec<u8>>)`.
10. Pipeline blocking detection correctly routes non-blocking → Rust fast-path, any-blocking → Python slow-path.
11. `r#where` raw identifier produces exact redis-py signature for LINSERT.
12. `blpop_poll`/`brpop_poll` abort scan on WRONGTYPE (multi-key semantics).

---

## Recommendations (priority order)

1. **Fix H-01** (pipeline coercion) — one-afternoon fix, restores drop-in parity.
2. **Fix M-01** (Lua error wording) — one-line change.
3. **Fix M-04** (spurious `had_list_mutation`) — cheap cleanup.
4. **Harden M-05** timing bounds (or use `asyncio.wait_for` wrappers).
5. **Add M-02** slow-path wake test.
6. **L-03/L-04** refactoring — defer to later polish phase.

Advisory only — no code changes made. Phase 14 may ship as-is if H-01 is tracked as a follow-up.
