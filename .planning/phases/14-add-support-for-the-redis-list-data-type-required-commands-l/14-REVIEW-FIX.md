---
phase: 14-add-support-for-the-redis-list-data-type-required-commands-l
type: code-review-fix
status: all_fixed
fixed_date: 2026-04-25
review_source: 14-REVIEW.md (P2 round, external review)
findings_in_scope: 7
fixed: 7
skipped: 0
iteration: 1
fix_scope: all
---

# Phase 14 Code Review Fix Report (P2 Round)

## Summary

All 7 P2 findings from the external review were applied as atomic per-finding
commits with regression tests added alongside each fix. Each test was written
to fail under the pre-fix code path and pass after the surgical change. The
full Python test suite (`491 passed`) and the full Rust unit suite
(`cargo test --lib` — `149 passed`) are green; `tests/test_lists.py` grew
from 90 to 111 tests (+21 new regression tests across the seven findings).

## Fixes Applied

### P2-01 — Continue executing blocking pipelines after errors
- **Commit:** `bb7403c`
- **Files:** `python/burner_redis/pipeline.py`, `tests/test_lists.py`
- **Tests added:**
  - `test_pipeline_blocking_continues_on_error_then_raises_first` — pipeline
    with `blpop → lset(missing) → set('after')` must execute all three; the
    third command must run despite the second's `ResponseError`; the first
    captured exception is raised after the loop completes.
  - `test_pipeline_blocking_no_raise_returns_exceptions_inline` — same
    shape with `raise_on_error=False`; exceptions appear inline at the
    failed position.
- **Verification:** PASS (2/2)
- **Change:** Slow path now mirrors the fast path — `try/except` appends
  exceptions to `results`; we walk `results` after the loop and re-raise
  the first `Exception` only when `raise_on_error=True`.

### P2-02 — LMOVE/RPOPLPUSH return nil for empty source before checking dst type
- **Commit:** `43aff10`
- **Files:** `src/store.rs`, `tests/test_lists.py`
- **Tests added:**
  - `test_rpoplpush_missing_src_with_string_dst_returns_none` — RPOPLPUSH
    with missing src and string dst returns `None` (was: WRONGTYPE).
  - `test_lmove_missing_src_with_string_dst_returns_none` — LMOVE mirror.
  - `test_lmove_nonempty_src_with_string_dst_still_wrongtype` — atomicity
    guard: when src DOES have an element, dst type-check still fires
    BEFORE pop.
- **Verification:** PASS (3/3)
- **Change:** In `Store::lmove_atomic`, validate src state (missing /
  empty / wrongtype) BEFORE the dst type-check. Empty/missing src returns
  `Ok(None)` immediately and never inspects dst. The dst type-check still
  fires before pop in the non-empty case to preserve atomicity (one write
  lock spans pop+push).

### P2-03 — Preserve finite sub-millisecond blocking timeouts
- **Commit:** `627af43`
- **Files:** `src/lib.rs`, `tests/test_lists.py`
- **Tests added:**
  - `test_blpop_sub_millisecond_timeout_expires` — `blpop(['empty'],
    timeout=0.0005)` must return `None` (was: hung forever).
  - `test_brpop_sub_millisecond_timeout_expires` — BRPOP mirror.
- **Verification:** PASS (2/2)
- **Change:** `timeout_to_ms`: positive timeouts now use
  `((t * 1000.0).ceil() as u64).max(1)` instead of `(t * 1000.0) as u64`.
  Sub-millisecond positives no longer truncate to 0 (which the loops
  interpret as "block forever").

### P2-04 — Reject empty key lists for BLPOP/BRPOP
- **Commit:** `514f317`
- **Files:** `src/lib.rs`, `tests/test_lists.py`
- **Tests added:**
  - `test_blpop_empty_keys_raises_wrong_arity` — `blpop([], timeout=0.1)`
    raises `ResponseError` matching real-Redis wording (was: hung / None).
  - `test_brpop_empty_keys_raises_wrong_arity` — BRPOP mirror.
  - `test_blpop_empty_tuple_raises_wrong_arity` — tuple form also rejected.
- **Verification:** PASS (3/3)
- **Change:** Each blocking-pop binding (`blpop`, `brpop`) now validates
  the normalized key list before scheduling the future and raises
  `ERR wrong number of arguments for '<cmd>' command` via
  `make_response_error` — matches real Redis exactly.

### P2-05 — Reject LPUSH/RPUSH calls with no values
- **Commit:** `e8548c4`
- **Files:** `src/store.rs`, `tests/test_lists.py`
- **Tests added:**
  - `test_lpush_no_values_raises_and_does_not_create_key` — `lpush('k')`
    raises and `r.exists('k') == 0` (was: empty list created, returns 0).
  - `test_rpush_no_values_raises_and_does_not_create_key` — RPUSH mirror.
  - `test_pipeline_lpush_no_values_raises_at_execute` — pipeline path
    inherits the guard via `Store::lpush`.
  - `test_lua_lpush_no_values_returns_error` — Lua dispatch arm already
    had `args.len() < 2` check; pinned by regression test.
- **Verification:** PASS (4/4)
- **Change:** `Store::lpush` and `Store::rpush` guard on
  `values.is_empty()` with `StoreError::Syntax(...)` BEFORE any mutation
  or notify, so no empty list is created and no waiters are spuriously
  woken. Pipeline arms route through the same Store methods.

### P2-06 — Coerce the LINSERT pivot value
- **Commit:** `a4d5418`
- **Files:** `python/burner_redis/__init__.py`, `python/burner_redis/pipeline.py`,
  `tests/test_lists.py`
- **Tests added:**
  - `test_linsert_int_pivot_matches_bytes_pivot` — int pivot resolves to
    matching bytes (was: TypeError).
  - `test_linsert_float_pivot_coerced` — float pivot coerced.
  - `test_pipeline_linsert_int_pivot_coerced` — pipeline mirror.
- **Verification:** PASS (3/3)
- **Change:** Apply `_coerce_value(refvalue)` in `_coerced_linsert` and in
  `Pipeline.linsert`. Insert-value coercion was already in place; the
  pivot now matches redis-py's full Encoder.encode() pass over every arg.

### P2-07 — Coerce LREM values before extracting bytes
- **Commit:** `c969121`
- **Files:** `python/burner_redis/__init__.py`, `python/burner_redis/pipeline.py`,
  `tests/test_lists.py`
- **Tests added:**
  - `test_lrem_int_value_coerced` — `r.lrem('k', 0, 42)` removes b'42'
    matches (was: TypeError).
  - `test_lrem_float_value_coerced` — float coerced.
  - `test_lrem_bool_value_raises` — bool still rejected (matches
    `_coerce_value` contract).
  - `test_pipeline_lrem_int_value_coerced` — pipeline mirror.
- **Verification:** PASS (4/4)
- **Change:** Added `_coerced_lrem` wrapper monkey-patched onto
  `BurnerRedis.lrem` (mirror of `_coerced_lpush`/`_coerced_lset`).
  Pipeline `lrem` stub now also calls `_coerce_value(value)`.

## Skipped

None.

## Verification

- Rust: `cargo test --lib` — **149 passed, 0 failed**
- Python (lists only): `pytest tests/test_lists.py -q` — **111 passed**
  (baseline: 90 — added 21 regression tests across the 7 findings)
- Full Python suite: `pytest -q` — **491 passed, 38 deselected**

All seven findings closed. No tests were skipped, no regressions introduced.

---

_Fixed: 2026-04-25_
_Fixer: Claude (gsd-code-fixer, Opus 4.7 1M)_
_Iteration: 1_
