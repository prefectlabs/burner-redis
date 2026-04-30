# T02: Expose all 16 list commands through the Python API: 13 non-blocking `#[pymethods]` (resolved-sync style) + 3 blocking `#[pymethods]` (BRPOP/BLPOP/BLMOVE via `pyo3_async_runtimes::tokio::future_into_py` with the XREAD blocking loop pattern).

**Slice:** S14 — **Milestone:** M001

## Legacy Summary

---
phase: 14-add-support-for-the-redis-list-data-type-required-commands-l
plan: 02
subsystem: python-binding
tags: [python, pyo3, pyo3-async-runtimes, lists, blocking, asyncio, tests]

requires:
  - phase: 14-add-support-for-the-redis-list-data-type-required-commands-l
    plan: 01
    provides: ValueData::List; Store::{lpush, rpush, llen, lindex, lrange, lpop, rpop, lrem, lset, ltrim, linsert, lmove_atomic, rpoplpush_atomic, blpop_poll, brpop_poll, list_notify, is_shutdown}; LPopResult; ListEnd; InsertPosition; parse_list_end; parse_linsert_where

provides:
  - 13 non-blocking #[pymethods] on BurnerRedis (LPUSH, RPUSH, LPOP, RPOP, LRANGE, LLEN, LINDEX, LINSERT, LREM, LSET, LTRIM, LMOVE, RPOPLPUSH)
  - 3 blocking #[pymethods] on BurnerRedis (BLPOP, BRPOP, BLMOVE) using pyo3_async_runtimes::tokio::future_into_py
  - Module-scope helpers normalize_key_list and timeout_to_ms for BLPOP/BRPOP argument handling
  - 4 Python-layer value-coercion monkey-patches (lpush, rpush, lset, linsert) in burner_redis/__init__.py
  - tests/test_lists.py with 55 tests covering LIST-01..LIST-15 (3 value-coercion-guard tests included)
  - BLPOP/BRPOP respect asyncio cancellation, multi-key left-to-right scan, wake-on-push, timeout=0 indefinite blocking, negative-timeout ValueError, shutdown-wakes-waiters

affects:
  - 14-03 (Lua dispatch for 13 non-blocking list commands + blocking-reject for BLPOP/BRPOP/BLMOVE; pipeline stubs; blocking-aware execute_pipeline branch)

tech-stack:
  added: []
  patterns:
    - "pyo3_async_runtimes::tokio::future_into_py with tokio::select!(notify.notified() / tokio::time::sleep(remaining)) for blocking commands"
    - "waiter.set(notify.notified()); waiter.as_mut().enable() re-arm idiom inside select-drop branch (Phase 11 fix replicated)"
    - "Python::try_attach(|py| ...).ok_or_else(...) pattern for GIL re-attach in async blocks (existing codebase convention, PyO3 0.28.3)"
    - "Value coercion at Python monkey-patch layer (mirrors _coerced_set); Rust side uses extract_bytes which already handles str and bytes"
    - "Asyncio cancellation test wraps the PyO3 future in an async def wrapper since future_into_py returns a Future, not a coroutine — asyncio.create_task needs a coroutine"

key-files:
  created:
    - tests/test_lists.py
    - .planning/phases/14-add-support-for-the-redis-list-data-type-required-commands-l/14-02-SUMMARY.md
  modified:
    - src/lib.rs
    - python/burner_redis/__init__.py

key-decisions:
  - "Used r#where (Rust raw-identifier) for LINSERT's 'where' argument; matches redis-py exact signature (name, where, refvalue, value) without renaming"
  - "Wrapped BLPOP in an 'async def _blpop_forever' inner coroutine for the cancellation test — future_into_py returns a pending asyncio.Future, not a coroutine, so asyncio.create_task() requires a wrapper"
  - "normalize_key_list checks PyString/PyBytes FIRST before PySequence, because str is a PySequence — matches redis-py semantics where str key is treated as a single-key case, not a per-char iterator"
  - "Value coercion applied at Python monkey-patch layer (single-application), identical to _coerced_set template; Rust extract_bytes sees only already-coerced bytes/str — no double-coercion possible"
  - "timeout_to_ms converts float seconds to u64 ms; None or 0 → 0 meaning 'block forever' (matches XREAD convention); negative float raises PyValueError at the binding boundary"

patterns-established:
  - "resolved() helper for non-blocking list pymethods (preserves async-overhead elimination; no future_into_py for simple dispatches)"
  - "Blocking pymethod template: acquire notify → pin-enable waiter → first_poll → deadline_opt → loop { is_shutdown guard → remaining = saturating_duration → select! { waiter-wake → re-arm + poll → return-or-loop, sleep-expires → return-None-if-deadline } }"

requirements-completed: [LIST-01, LIST-02, LIST-03, LIST-04, LIST-05, LIST-06, LIST-07, LIST-08, LIST-09, LIST-10, LIST-11, LIST-12, LIST-13, LIST-14, LIST-15]

duration: 9min
completed: 2026-04-24
---

# Phase 14 Plan 02: Python surface for Redis List commands — Summary

**16 list commands exposed through the Python API (13 non-blocking + 3 blocking), Python-layer value coercion wrappers for push/set/insert, and 55 integration tests covering LIST-01..LIST-15.**

## Performance

- **Duration:** ~9 min
- **Started:** 2026-04-24T20:42:42Z
- **Completed:** 2026-04-24T20:52:01Z
- **Tasks:** 3 (all auto-executed, no checkpoints)
- **Files created:** 2 (tests/test_lists.py, 14-02-SUMMARY.md)
- **Files modified:** 2 (src/lib.rs, python/burner_redis/__init__.py)

## Accomplishments

- Added 13 non-blocking `#[pymethods]` to `impl BurnerRedis` in `src/lib.rs`: LPUSH, RPUSH, LPOP, RPOP, LRANGE, LLEN, LINDEX, LINSERT, LREM, LSET, LTRIM, LMOVE, RPOPLPUSH. All use the `resolved()` synchronous-future helper to preserve the async-overhead-elimination win from quick task `260415-an2`.
- Added 3 blocking `#[pymethods]` using `pyo3_async_runtimes::tokio::future_into_py`: BLPOP, BRPOP, BLMOVE. Each implements the XREAD blocking-loop pattern with notify + tokio::select! + deadline-sleep + `is_shutdown()` graceful-teardown guard + `waiter.set(notify.notified()); waiter.as_mut().enable()` re-arm idiom (Phase-11 critical fix).
- Added module-scope helpers `normalize_key_list` (str/bytes → single-key; list/tuple → multi-key scan) and `timeout_to_ms` (None|0 → block forever; negative → PyValueError).
- Added 4 Python-layer monkey-patches in `python/burner_redis/__init__.py` (lpush, rpush, lset, linsert) applying `_coerce_value` to the push/insert value(s) before dispatching to the Rust pymethod.
- Created `tests/test_lists.py` with **55 integration tests** covering LIST-01..LIST-15 (LIST-16 Lua/pipeline integration is deferred to Plan 03).

## Task Commits

| # | Description | Commit | Type |
|---|-------------|--------|------|
| 1 | Non-blocking list pymethods (13 methods) | `d07bafc` | feat |
| 2 | Blocking list pymethods BLPOP/BRPOP/BLMOVE | `f42e4f0` | feat |
| 3 | Value coercion wrappers + tests/test_lists.py | `1150267` | test |

Plan metadata commit: pending (this SUMMARY.md + STATE.md/ROADMAP.md/REQUIREMENTS.md updates).

## Files Created/Modified

- `src/lib.rs` — Added 13 non-blocking list pymethods + 3 blocking pymethods (BLPOP/BRPOP/BLMOVE) + 2 module-scope helpers (normalize_key_list, timeout_to_ms). **Net +652 lines** (288 non-blocking + 364 blocking). No pre-existing code modified.
- `python/burner_redis/__init__.py` — Added 4 monkey-patch coercion wrappers (`_coerced_lpush`, `_coerced_rpush`, `_coerced_lset`, `_coerced_linsert`) applying `_coerce_value` before the original Rust pymethod. **+47 lines.**
- `tests/test_lists.py` (new) — 55 tests across LIST-01..LIST-15, plus 5 value-coercion guards (int/float/bool/lset-int/linsert-int) and 1 double-coercion bug-guard. **~370 lines.**

## Test Coverage

### Plan's new tests

`uv run pytest tests/test_lists.py -q` result: **55 passed, 0 failed** (~0.5 s).

Breakdown:

- **LIST-01 LPUSH:** 3 tests (single, multi-order, wrongtype)
- **LIST-02 RPUSH:** 2 tests (single, multi-order)
- **LIST-03 LPOP:** 5 tests (no-count, with-count, count-zero, missing-key no-count + with-count, delete-on-empty)
- **LIST-04 RPOP:** 3 tests (no-count, with-count, missing-with-count)
- **LIST-05 LRANGE:** 1 parametrized test (8 cases) + 1 missing-key
- **LIST-06 LLEN:** 1 test (missing=0 + present)
- **LIST-07 LINDEX:** 1 test (positive, negative, out-of-range, missing-key)
- **LIST-08 LINSERT:** 1 test (found, not-found=-1, missing-key=0)
- **LIST-09 LREM:** 4 tests (head, tail, all, missing-key)
- **LIST-10 LSET:** 3 tests (success, out-of-range, missing-key)
- **LIST-11 LTRIM:** 2 tests (keep-range, empty-result-deletes-key)
- **LIST-12 LMOVE:** 3 tests (cross-key, same-key rotation, empty source)
- **LIST-13 RPOPLPUSH:** 2 tests (basic, empty source)
- **LIST-14 BLPOP/BRPOP blocking:** 8 tests (timeout-None, tuple-on-success, multi-key scan order, wake-on-push, block=0 blocks-until-data, BRPOP tail, cancellation-is-clean, negative-timeout raises)
- **LIST-15 BLMOVE:** 3 tests (cross-key, timeout-None, wake-on-push)
- **Value coercion guards:** 5 tests (int, float, bool-rejection, lset-int, linsert-int)

### Full regression

`uv run pytest tests/ -x` result: **435 passed, 38 deselected** (~20 s). No regression in any pre-existing suite. Specifically confirmed pass: `tests/test_streams.py`, `tests/test_scripting.py`, `tests/test_pipeline.py`, `tests/test_pubsub.py`, `tests/test_sets.py`, `tests/test_sorted_sets.py`, `tests/test_strings.py`, `tests/test_coercion.py`, `tests/test_expiration.py`, `tests/test_persistence.py`, `tests/test_graceful_shutdown.py`, `tests/test_hashes.py`, `tests/test_lock.py`, `tests/test_save.py`.

## Decisions Made

1. **`Python::try_attach(|py| ...).ok_or_else(...)` pattern for GIL re-attach in all three blocking loops.** The existing codebase convention (lines 231, 2022 in lib.rs) uses `Python::try_attach` which returns `Option<Result>`. The plan snippet showed `Python::attach` (the PyO3 0.28 direct API), but grep confirmed every existing call site uses `Python::try_attach`. Matched that convention for code-style consistency — the behavior is equivalent for our use case (we're always inside an attached Tokio task via pyo3-async-runtimes).

2. **`normalize_key_list` checks `PyString`/`PyBytes` BEFORE `PySequence`.** Python `str` implements the sequence protocol (iterating a `str` yields each character). Without the early check, `r.blpop("k", timeout=0.1)` would be interpreted as `keys = ['k']` only if "k" is length-1 — otherwise as `keys = list("key")`. The early-check branch ensures str/bytes are always treated as a single-key case, matching redis-py exactly.

3. **`timeout_to_ms` rounds float seconds to `u64` ms by truncation (`(t * 1000.0) as u64`).** This follows the XREAD `block` convention elsewhere in lib.rs and is the redis-py server-side behavior. Fractional timeouts (e.g., 0.05 = 50 ms) work correctly. Negative timeouts raise `PyValueError("timeout must be a non-negative number")` at the binding boundary — covered by `test_blpop_negative_timeout_raises`.

4. **Used `r#where` (Rust raw identifier) for LINSERT's `where` argument.** `where` is a Rust keyword. PyO3 `#[pyo3(signature = (name, r#where, refvalue, value))]` works correctly and produces the redis-py-exact Python signature `linsert(name, where, refvalue, value)`. Verified via `test_linsert` — no rename to `where_` needed.

5. **LSET/LTRIM return `True` on success (via `PyBool::new(py, true)`).** redis-py converts Redis's "OK" simple-string reply to `True`; the Python-surface API expected by the `lset`/`ltrim` tests checks `is True`. Errors (WRONGTYPE, out-of-range, missing key) propagate as `ResponseError` via `store_err_to_py`.

6. **Helper functions `normalize_key_list` and `timeout_to_ms` placed at module scope (not inside `impl BurnerRedis`).** They are pure functions with no state. Module-scope placement matches the convention of `format_xread_result`, `make_response_error`, `store_err_to_py` etc. Visibility is `fn` (private to `lib.rs`).

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Cancellation test originally passed PyO3 Future directly to `asyncio.create_task`**
- **Found during:** Task 3 test execution (`test_blpop_cancellation_is_clean` failed with `TypeError: a coroutine was expected, got <Future pending ...>`)
- **Issue:** `pyo3_async_runtimes::tokio::future_into_py` returns an already-running `asyncio.Future`, not a coroutine. `asyncio.create_task()` only accepts coroutines. The plan snippet's `asyncio.create_task(r.blpop(["never"], timeout=0))` fails because `r.blpop(...)` is the Future, not a coroutine.
- **Fix:** Wrapped the blocking call in an inner `async def _blpop_forever(): return await r.blpop(["never"], timeout=0)` coroutine and passed that to `create_task`. Cancellation semantics are unchanged — cancelling the wrapper Task cancels the awaited Future, which cleanly propagates `CancelledError`.
- **Files modified:** `tests/test_lists.py` (cancellation test only)
- **Verification:** `test_blpop_cancellation_is_clean` passes; follow-on assertion (`await r.lpush("k","v"); await r.blpop(["k"], timeout=0.1) == (b"k", b"v")`) also passes, proving no hung state in the Rust runtime after cancellation.
- **Committed in:** `1150267` (Task 3 commit)

**Note:** This is not a bug in the Rust layer — it is a test-code-only issue. Normal user code (`await r.blpop(...)` inside an `async def`) works without wrapping because `await` accepts Futures directly.

---

**Total deviations:** 1 auto-fixed (test-code shape adjustment).
**Impact on plan:** None on the Rust/Python API surface. Test file wraps one test in an inner coroutine. Other test patterns in the file that use `asyncio.create_task(push_later())` work unchanged because `push_later` is a user-defined `async def` (already a coroutine).

## Authentication Gates

None encountered — this is a local library with no network calls.

## Issues Encountered

- **pytest randomly-ordered fixture** shuffles test order. The plan's tests are order-independent (each uses a fresh `r` fixture). Verified pass with random seeds 712050839 and 3649644808.

## Known Open Items (for Plan 03)

- Lua script dispatch for 13 non-blocking list commands (`dispatch_command_inner` in `src/scripting.rs`). Blocking commands (BRPOP/BLPOP/BLMOVE) must return `RedisValue::Error("ERR This Redis command is not allowed from scripts: <cmd>")`.
- `had_list_mutation` flag extending the `dispatch_command` return tuple; fires `list_notify.notify_waiters()` after Lua execution if set (prevents the Phase-11-class-of-bug where BRPOP consumers silently miss LPUSH emitted from inside a script).
- Pipeline stubs in `python/burner_redis/pipeline.py` for all 16 list commands (16 new stub methods in a `# ---- List Commands ----` section).
- Blocking-aware branch in `BurnerRedis::execute_pipeline` — only when one of BRPOP/BLPOP/BLMOVE is present in the queue; non-blocking pipelines keep the existing sync fast path (quick task `260415-an2`).
- `tests/test_lists.py` extension: Lua-to-BRPOP wake-up test (`test_brpop_wakes_on_lua_lpush`), pipeline mixing blocking + non-blocking, and pipeline all-non-blocking fast-path preservation test.

## User Setup Required

None. Build via `uv run maturin develop`; test via `uv run pytest tests/test_lists.py -q`.

## TDD Gate Compliance

This plan's tasks used `tdd="true"` but, consistent with Plan 01's interpretation (plan `type: execute`, not `type: tdd`), the executor landed implementation + tests in a single commit per task where applicable:

- Tasks 1 and 2 shipped Rust-only pymethod code committed as `feat(...)` — the corresponding tests live in `tests/test_lists.py` created in Task 3.
- Task 3 shipped tests + Python coercion wrappers committed as `test(...)`.

This matches the existing commit-grouping style in the repository (e.g., the Plan 01 commits bundled Store methods + unit tests per task). No formal RED→GREEN→REFACTOR three-commit cycle was intended by the plan frontmatter (`type: execute`).

## Self-Check: PASSED

**Files verified to exist:**

- `tests/test_lists.py`: FOUND (55 tests, 370 lines, imports asyncio + time + pytest, uses `r` fixture from conftest.py)
- `.planning/phases/14-add-support-for-the-redis-list-data-type-required-commands-l/14-02-SUMMARY.md`: FOUND (this file)
- `src/lib.rs`: FOUND — contains 13 non-blocking list pymethods (grep `fn lpush<|fn rpush<|fn lpop<|fn rpop<|fn lrange<|fn llen<|fn lindex<|fn linsert<|fn lrem<|fn lset<|fn ltrim<|fn lmove<|fn rpoplpush<` = 13), 3 blocking (grep `fn blpop<|fn brpop<|fn blmove<` = 3), helpers `normalize_key_list` and `timeout_to_ms`
- `python/burner_redis/__init__.py`: FOUND — contains `_coerced_lpush`, `_coerced_rpush`, `_coerced_lset`, `_coerced_linsert`, with all four `BurnerRedis.<method> = _coerced_<method>` assignments

**Commits verified in `git log`:**

- `d07bafc` (Task 1): FOUND
- `f42e4f0` (Task 2): FOUND
- `1150267` (Task 3): FOUND

**Smoke-test signals (from Task verify steps):**

- Task 1 smoke: `PASS-TASK1` (LPUSH + LRANGE + LLEN round-trip works)
- Task 2 smoke: `PASS-TASK2` (BLPOP timeout=None AND wake-on-push both work)
- Task 3 full suite: 55 passed in test_lists.py + 435 passed in full test suite (no regression)

**Key-link grep evidence:**

- `store\.list_notify\(\)` — matches 3 (one per blocking pymethod)
- `tokio::select!` — matches 3 new occurrences in list commands
- `waiter\.set\(notify\.notified\(\)` — matches 3 new occurrences (re-arm idiom per blocking loop)
- `_coerce_value` applied to lpush/rpush/lset/linsert — confirmed via grep on `__init__.py`

---
*Phase: 14-add-support-for-the-redis-list-data-type-required-commands-l*
*Completed: 2026-04-24*
