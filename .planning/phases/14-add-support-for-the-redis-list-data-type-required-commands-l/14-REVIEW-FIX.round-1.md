---
phase: 14-add-support-for-the-redis-list-data-type-required-commands-l
fixed_at: 2026-04-24
review_path: .planning/phases/14-add-support-for-the-redis-list-data-type-required-commands-l/14-REVIEW.md
iteration: 1
findings_in_scope: 6
fixed: 5
skipped: 1
status: partial
---

# Phase 14: Code Review Fix Report

**Fixed at:** 2026-04-24
**Source review:** `.planning/phases/14-add-support-for-the-redis-list-data-type-required-commands-l/14-REVIEW.md`
**Iteration:** 1

**Summary:**
- Findings in scope: 6 (H-01 + M-01..M-05)
- Fixed: 5
- Skipped: 1 (M-03, deferred to perf profiling per the review's own guidance)

After applying fixes, the full Python test suite passes: **470 passed**, with 90 tests in `tests/test_lists.py` (up from 80 — 10 new regression tests added across H-01, M-01, and M-02).

## Fixed Issues

### H-01: Pipeline list commands bypass Python-layer value coercion

**Files modified:** `python/burner_redis/pipeline.py`, `tests/test_lists.py`
**Commit:** `bf88fb5`
**Applied fix:**

- Added a local `_coerce_value(value)` helper at the top of `pipeline.py` that mirrors the one in `burner_redis/__init__.py`. Duplicated rather than imported to avoid a circular import (`burner_redis/__init__.py` imports `Pipeline`).
- Coerce values at buffer time in the pipeline stubs:
  - `Pipeline.set` — coerce `value`
  - `Pipeline.lpush` / `Pipeline.rpush` — coerce each variadic value
  - `Pipeline.lset` — coerce `value` (not `index`)
  - `Pipeline.linsert` — coerce `value` only; `refvalue` is a lookup pivot and is left untouched, matching `_coerced_linsert` in `__init__.py`.
- Added 7 new regression tests (`test_pipeline_lpush_int_coerced`, `test_pipeline_rpush_float_coerced`, `test_pipeline_lpush_bool_raises`, `test_pipeline_lset_int_coerced`, `test_pipeline_linsert_int_coerced`, `test_pipeline_set_int_coerced`, `test_pipeline_set_bool_raises`) verifying parity with the monkey-patched client methods.
- Note: coercion happens at `pipe.lpush(...)` time (synchronously), so `TypeError` raises on the buffering call rather than on `.execute()`. This matches the spirit of redis-py's `pipe.lpush(...)` — bool/None rejection is a client-side validation and not deferred.

### M-01: Lua blocking-reject error wording diverged from real Redis

**Files modified:** `src/scripting.rs`, `tests/test_lists.py`
**Commits:** `123ab8f` (initial fix), `a0dd54a` (regression-test tightening)
**Applied fix:**

- Changed the BLPOP/BRPOP/BLMOVE rejection error from `"ERR This Redis command is not allowed from scripts: BLPOP"` to `"ERR This Redis command is not allowed from script"` (singular `script`, no colon, no command name) — matches real Redis exactly.
- Updated the three existing `test_lua_*_rejected` tests' regex from `"not allowed from scripts"` to `"not allowed from script"`.
- Added a new regression test (`test_lua_blocking_error_does_not_include_command_name`) that asserts the first line of the error message contains neither `"BLPOP"` nor the old `"from scripts"` plural. The first-line check is needed because mlua appends a Lua stack traceback (which legitimately contains colons and source paths) — that traceback is not part of the wording we control.

### M-02: Add explicit slow-path BLPOP/BRPOP wake test asserting elapsed-time lower bound

**Files modified:** `tests/test_lists.py`
**Commit:** `567dae8`
**Applied fix:**

Added two new tests that pin a meaningful elapsed-time lower bound, distinguishing the `tokio::select!` wake-up path from a fast-path race-win:

- `test_blpop_slow_path_wake_elapsed_lower_bound` — uses `SLEEP = 0.15`, asserts `elapsed >= SLEEP * 0.8 (≈0.12s)` and `elapsed < 2.0s`.
- `test_brpop_slow_path_wake_elapsed_lower_bound` — same pattern, mirrored for BRPOP / RPUSH.

The 0.8× tolerance accounts for monotonic-clock jitter on heavily loaded CI; the assertion still rules out a "first poll succeeded" code path which would return in <1 ms.

### M-04: `had_list_mutation` fires on non-mutating success cases (requires human verification)

**Files modified:** `src/scripting.rs`
**Commit:** `ea05cc9`
**Applied fix:**

Replaced the blanket `is_list_write && success` predicate in `dispatch_command` with per-command return-value matching:

- `LPUSH` / `RPUSH` → `matches!(result, RedisValue::Integer(_))` (always grow on success).
- `LINSERT` → `matches!(result, RedisValue::Integer(n) if n > 0)` (skips `-1` pivot-not-found and `0` key-missing — neither mutates the list).
- `LMOVE` / `RPOPLPUSH` → `matches!(result, RedisValue::BulkString(_))` (skips `Nil` empty-source returns).
- All other commands → `false`.

Added an extended doc comment on `dispatch_command` explaining the refined semantics.

**Status:** `fixed: requires human verification` — this is a logic refinement, and Tier 1/Tier 2 verification (re-read + `cargo check`) only confirms syntax. The full Python test suite (`470 passed`) including the existing `test_brpop_wakes_on_lua_lpush` and `test_blpop_wakes_on_lua_rpush` regression guards passes, which exercises the LPUSH/RPUSH paths. The LINSERT / LMOVE / RPOPLPUSH spurious-wake cases are not directly asserted (they were "spurious but safe" per the review). Recommend: a developer should manually confirm the per-command match arms are exhaustive and the new behavior is intended before this is considered final.

### M-05: Widen timing-based test upper bounds

**Files modified:** `tests/test_lists.py`
**Commit:** `567dae8` (combined with M-02)
**Applied fix:**

- `test_blpop_timeout_returns_none` — upper bound widened from `< 0.5` to `< 2.0`.
- `test_blmove_timeout_returns_none` — same, `< 0.5` → `< 2.0`.
- Lower bound (`> 0.05`) preserved — that is the meaningful assertion (we did wait at least the requested timeout).
- Added inline comments explaining the rationale.

## Skipped Issues

### M-03: Eager `notify_waiters()` in `lpush`/`rpush` could be optimized to empty→non-empty transitions

**File:** `src/store.rs:3296-3298, and lpush/rpush`
**Reason:** Skipped per explicit user direction and the review's own guidance ("Defer to perf profiling"). The review classifies this as a perf-only optimization with no correctness implication — `notify_waiters()` with 0 waiters is cheap, and acting on it now without profile evidence would be premature.
**Original issue:** Under high push throughput with concurrent BLPOP subscribers, transitioning the wake only on `was_empty → non-empty` (via a `was_empty = list.is_empty()` snapshot before push) would avoid unnecessary waker churn. Defer to perf profiling.

---

## Verification Summary

| Layer | Tool | Result |
|---|---|---|
| Tier 1 (re-read) | manual | All edits verified present and surrounding code intact |
| Tier 2 (syntax) | `python3 -c "import ast; ast.parse(...)"` | `pipeline.py`, `test_lists.py` parse OK |
| Tier 2 (syntax) | `cargo check` | `scripting.rs` builds; only pre-existing deprecation warnings |
| Build | `maturin develop --release` | Wheel built and installed cleanly |
| Test suite | `pytest -x -q` | **470 passed**, 38 deselected — no regressions |
| List tests specifically | `pytest tests/test_lists.py -x -q` | **90 passed** (was 80 — 10 new tests added) |

---

_Fixed: 2026-04-24_
_Fixer: Claude (gsd-code-fixer)_
_Iteration: 1_
