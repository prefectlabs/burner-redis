---
phase: quick-260425-r3r
plan: 01
subsystem: python-bindings
tags: [redis-py-compat, async, blocking-commands, lists]
requires: []
provides:
  - BurnerRedis.blpop returns a coroutine (was eager asyncio.Future)
  - BurnerRedis.brpop returns a coroutine (was eager asyncio.Future)
  - BurnerRedis.blmove returns a coroutine (was eager asyncio.Future)
affects:
  - python/burner_redis/__init__.py
  - tests/test_lists.py
tech-stack:
  added: []
  patterns:
    - "Python async-def wrapper around Rust binding to defer pyo3_async_runtimes::tokio::future_into_py invocation until coroutine await"
key-files:
  created: []
  modified:
    - python/burner_redis/__init__.py
    - tests/test_lists.py
decisions:
  - "Apply pure Python-layer fix via async-def wrappers; do not modify Rust pyo3_async_runtimes usage."
  - "Do not pre-fill BLMOVE src/dest defaults at the wrapper layer beyond what redis-py exposes; Rust remains single source of truth for default handling."
  - "Leave existing test_blpop_cancellation_is_clean's _blpop_forever shim untouched; it is now redundant but still correct."
  - "xread/xreadgroup blocking paths (src/lib.rs lines 1025, 1328) have the same eager-Future bug class but are out of scope for this quick task; track as follow-up."
metrics:
  duration_seconds: 179
  completed_date: 2026-04-26
  tasks_completed: 2
  tests_added: 10
  files_modified: 2
---

# Phase quick-260425-r3r Plan 01: Wrap blocking list methods (blpop/brpop/blmove) as coroutines Summary

P2 redis.asyncio compatibility fix: convert `BurnerRedis.blpop`, `.brpop`, `.blmove` from eager-Future-returning bindings into proper Python coroutines via `async def` wrappers, so they accept `asyncio.create_task(...)` and defer the blocking pop until awaited.

## What was fixed

PyO3's `pyo3_async_runtimes::tokio::future_into_py(...)` schedules its future onto Tokio at call time and returns an `asyncio.Future`. That broke two `redis.asyncio.Redis` drop-in expectations:

1. **`asyncio.create_task(r.blpop(...))` raised `TypeError`** because `create_task` requires a coroutine, not a Future.
2. **The blocking pop began the moment `r.blpop(keys)` was called**, not when the result was awaited — defeating the "work begins on await" semantic that callers rely on for cancellation/composition.

The fix introduces three Python `async def` wrappers in `python/burner_redis/__init__.py` that capture the original Rust binding and `await` it inside the coroutine body. Because `async def` evaluation produces a coroutine object without executing the body, the call to `_original_blpop` (and therefore `future_into_py`) only fires when the coroutine is awaited or scheduled. This restores both compatibility properties.

## Pattern applied

The wrappers mirror the existing list-command coercion-wrapper pattern already established in `python/burner_redis/__init__.py` for `LPUSH`, `RPUSH`, `LSET`, `LREM`, `LINSERT`, `SET`, and `SETEX`:

```python
_original_blpop = BurnerRedis.blpop


async def _async_blpop(self, keys, timeout=None):
    return await _original_blpop(self, keys, timeout=timeout)


BurnerRedis.blpop = _async_blpop
```

The blocking-command wrappers differ from the LPUSH-style coercion wrappers in one important way: **they do no value coercion**. There are no scalar values passed to BLPOP/BRPOP/BLMOVE — only keys (normalized by Rust `normalize_key_list`), a numeric timeout, and the LEFT/RIGHT direction tokens (which Rust accepts as either str or bytes per quick task 260425-ftl). The wrappers exist purely to convert the eager Future into a coroutine.

A new sub-section header and explanatory comment block was added above the wrappers documenting the rationale for future readers:

```
# ---- Blocking List Commands: coroutine wrappers (redis-py parity) ----
```

## Audit observation (out of scope, follow-up)

The plan included a directed audit of other `pyo3_async_runtimes::tokio::future_into_py` call sites in `src/lib.rs`. Findings:

| Site | Line | Bug class | Status |
|------|------|-----------|--------|
| `xread` blocking path | 1025 | Same eager-Future bug | **Out of scope.** File follow-up if a user hits it. |
| `xreadgroup` blocking path | 1328 | Same eager-Future bug | **Out of scope.** File follow-up if a user hits it. |
| `_subscribe_listener` | 1970 | Internal helper, never user-composed | No bug. |
| `_stop_subscriber_listener` | 2061 | Internal helper, called from `PubSub.aclose` | No bug. |

Recommendation: open a follow-up quick task for `xread` / `xreadgroup` if/when it bites a real user. The `problem_statement` for this fix explicitly scoped to `blpop`/`brpop`/`blmove`, so bundling those would expand the change surface unnecessarily.

## Test counts

**+10 new tests in `tests/test_lists.py`:**

| Category | Count | Tests |
|----------|-------|-------|
| `iscoroutinefunction` assertion | 1 | `test_blpop_is_coroutine_function` (asserts all three at once) |
| `returns_coroutine` | 3 | `test_{blpop,brpop,blmove}_returns_coroutine` |
| `create_task` acceptance | 3 | `test_{blpop,brpop,blmove}_create_task_accepts_coroutine` |
| Deferred execution | 3 | `test_{blpop_deferred_execution_does_not_pop,brpop_deferred_execution_does_not_pop,blmove_deferred_execution_does_not_move}_before_await` |

All tests run clean under `pytest -W error::RuntimeWarning` — no leaked un-awaited coroutines.

## Verification

Per the plan's verification block, all gates passed:

- `cargo check` — clean (no Rust changes).
- `maturin develop --release` — built and installed editable wheel.
- `pytest tests/test_lists.py -x -W error::RuntimeWarning` — **143 passed** (was 133 pre-fix; +10 new).
- `pytest tests/ -x` — **523 passed, 38 deselected** (no cross-module regression).
- Manual sanity check (4-property one-liner): `OK`.

## Deviations from Plan

None — plan executed exactly as written.

## TDD Gate Compliance

The plan author marked both tasks `tdd="true"` but ordered Task 1 as the implementation (the `async def` wrappers) and Task 2 as the tests. As executed, the commit sequence was `feat(...)` followed by `test(...)`. This inverts the canonical RED → GREEN gate sequence.

This is acceptable for this particular fix because:

1. The change is a pure Python-layer wrapping refactor — `r.blpop` already existed and worked for direct-await callers; the bug surface is a redis-py compat gap, not a missing feature.
2. The new tests (`returns_coroutine`, `create_task_accepts_coroutine`, `deferred_execution_*`) explicitly verify the property the wrapper introduces. Without the wrapper they would fail with `TypeError`/eager-execution observation, so retroactively running them against the pre-fix tree would still produce a RED → GREEN signal.
3. The plan was authored with this ordering and explicitly designated the two tasks as a `feat` followed by a `test` commit, which aligns with the typical convention for small contained Python-binding wrappers in this codebase.

If strict gate-sequence compliance is required, the test commit (`62c2b36`) could be re-ordered before the implementation commit (`0556b80`) via interactive rebase — but that would also require splitting the wrappers into a "stub that fails" stage, which is not meaningful for an `async def` wrapper this small.

## Self-Check: PASSED

**Files claimed created:** none.

**Files claimed modified:**

- `python/burner_redis/__init__.py` — FOUND (modified in commit `0556b80`).
- `tests/test_lists.py` — FOUND (modified in commit `62c2b36`).

**Commits claimed:**

- `0556b80` (`feat(quick-260425-r3r): wrap blocking list methods as coroutines`) — FOUND.
- `62c2b36` (`test(quick-260425-r3r): cover create_task + deferred-exec for blpop/brpop/blmove`) — FOUND.

**Tests claimed:** 143 passed in `tests/test_lists.py`, 523 passed in full unit suite. Reproduced.

**Manual sanity check:** `OK`.
