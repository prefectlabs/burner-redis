---
phase: 12-close-remaining-redis-py-compatibility-gaps-for-drop-in-repl
fixed_at: 2026-04-14T12:15:00Z
review_path: .planning/phases/12-close-remaining-redis-py-compatibility-gaps-for-drop-in-repl/12-REVIEW.md
iteration: 1
findings_in_scope: 4
fixed: 4
skipped: 0
status: all_fixed
---

# Phase 12: Code Review Fix Report

**Fixed at:** 2026-04-14T12:15:00Z
**Source review:** .planning/phases/12-close-remaining-redis-py-compatibility-gaps-for-drop-in-repl/12-REVIEW.md
**Iteration:** 1

**Summary:**
- Findings in scope: 4
- Fixed: 4
- Skipped: 0

## Fixed Issues

### CR-01: Lock release is non-atomic (TOCTOU race condition)

**Files modified:** `python/burner_redis/lock.py`
**Commit:** 7f0ea53
**Applied fix:** Replaced the three-step GET + compare + DELETE release with an atomic Lua script (`RELEASE_SCRIPT`) that performs check-and-delete in a single EVAL call. Added `RELEASE_SCRIPT` module-level constant matching redis-py's implementation. The `release()` method now calls `self._client.eval(RELEASE_SCRIPT, 1, self.name, self.token)` and checks the return value to determine if the lock was successfully released.

### WR-01: _coerce_value accepts arbitrary types via str() fallthrough

**Files modified:** `python/burner_redis/__init__.py`
**Commit:** 8facc23
**Applied fix:** Replaced the `return str(value)` fallthrough at the end of `_coerce_value()` with a `raise TypeError` that includes the unsupported type name in the error message, matching redis-py's `Encoder.encode()` behavior. Lists, dicts, custom objects, and other unsupported types now raise a clear error instead of silently converting via `str()`.

### WR-02: Blocking lock acquire has timing drift

**Files modified:** `python/burner_redis/lock.py`
**Commit:** c2b5254
**Applied fix:** Added `import time` and replaced the `elapsed += self.sleep` counter with a `time.monotonic()` deadline. The deadline is computed once before the loop (`deadline = time.monotonic() + blocking_timeout`), and each iteration checks `time.monotonic() >= deadline` after a failed SET NX attempt. This accounts for actual wall-clock time including time spent in the SET call itself.

### WR-03: Pipeline halts execution on first command error

**Files modified:** `python/burner_redis/pipeline.py`, `tests/test_pipeline.py`
**Commit:** 0d054fe
**Applied fix:** Wrapped the per-command `await method(*args, **kwargs)` call in `execute()` with a try/except that catches `Exception` and appends the exception object to the results list instead of propagating it. This matches redis-py's pipeline behavior where all commands execute and errors appear inline. Updated `test_pipeline_wrongtype_error` to verify the new behavior: the test now queues three commands (set, hset on wrong type, get), verifies all three execute, checks the error is an inline Exception at index 1 with "WRONGTYPE" in its message, and confirms the commands before and after the error return correct results.

---

_Fixed: 2026-04-14T12:15:00Z_
_Fixer: Claude (gsd-code-fixer)_
_Iteration: 1_
