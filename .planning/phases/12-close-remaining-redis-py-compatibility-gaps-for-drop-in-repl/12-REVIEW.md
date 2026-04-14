---
phase: 12-close-remaining-redis-py-compatibility-gaps-for-drop-in-repl
reviewed: 2026-04-14T12:00:00Z
depth: standard
files_reviewed: 12
files_reviewed_list:
  - src/commands/pubsub.rs
  - src/store.rs
  - src/lib.rs
  - python/burner_redis/__init__.py
  - python/burner_redis/lock.py
  - python/burner_redis/pipeline.py
  - tests/test_coercion.py
  - tests/test_strings.py
  - tests/test_locking.py
  - tests/test_expiration.py
  - tests/test_streams.py
  - tests/test_pipeline.py
findings:
  critical: 1
  warning: 3
  info: 2
  total: 6
status: issues_found
---

# Phase 12: Code Review Report

**Reviewed:** 2026-04-14T12:00:00Z
**Depth:** standard
**Files Reviewed:** 12
**Status:** issues_found

## Summary

Reviewed the Rust core (store, lib, pubsub), Python bindings (init, lock, pipeline), and test suite for the burner-redis drop-in redis-py replacement. The Rust core is well-structured with consistent patterns for passive expiration, type checking, and error handling across all data type operations. The pub/sub glob matcher is solid with iterative backtracking and ReDoS protection. The Python bindings are clean and follow redis-py conventions closely.

One critical issue found: the Lock release operation is non-atomic, creating a TOCTOU race condition that could cause one lock holder to release another's lock. Three warnings relate to input validation gaps, timing drift in blocking lock acquisition, and pipeline error behavior diverging from redis-py semantics.

## Critical Issues

### CR-01: Lock release is non-atomic (TOCTOU race condition)

**File:** `python/burner_redis/lock.py:100-108`
**Issue:** The `release()` method performs GET, token comparison, and DELETE as three separate network-equivalent operations. Between the GET (line 101) and DELETE (line 108), the lock could expire and be re-acquired by another caller. The DELETE then removes the new holder's lock, violating mutual exclusion. Redis's `redis-py` Lock uses a Lua script for atomic check-and-delete.

**Fix:** Use a Lua script for atomic release, matching redis-py's implementation:
```python
RELEASE_SCRIPT = """
if redis.call("get", KEYS[1]) == ARGV[1] then
    return redis.call("del", KEYS[1])
else
    return 0
end
"""

async def release(self):
    if self.token is None:
        raise LockError("Cannot release an unlocked lock")
    result = await self._client.eval(RELEASE_SCRIPT, 1, self.name, self.token)
    if result != 1:
        raise LockError("Cannot release a lock that's no longer owned")
    self.token = None
```

## Warnings

### WR-01: _coerce_value accepts arbitrary types via str() fallthrough

**File:** `python/burner_redis/__init__.py:58`
**Issue:** The `_coerce_value` function falls through to `str(value)` for any type not explicitly handled (bytes, memoryview, bool, int, float, str). Redis-py's `Encoder.encode()` raises a `DataError` for unsupported types rather than silently converting. This means passing objects like lists, dicts, or custom classes will silently store their `str()` representation instead of raising an error, which could mask bugs in calling code.

**Fix:** Raise a `TypeError` for unsupported types instead of falling through:
```python
def _coerce_value(value):
    if isinstance(value, (bytes, memoryview)):
        return value
    if isinstance(value, bool):
        raise TypeError(
            "Invalid input of type: 'bool'. "
            "Convert to a bytes, string, int or float first."
        )
    if isinstance(value, (int, float)):
        return repr(value).encode()
    if isinstance(value, str):
        return value
    raise TypeError(
        f"Invalid input of type: '{type(value).__name__}'. "
        "Convert to a bytes, string, int or float first."
    )
```

### WR-02: Blocking lock acquire has timing drift

**File:** `python/burner_redis/lock.py:78-90`
**Issue:** The blocking acquire loop tracks elapsed time by summing `self.sleep` on each iteration (line 87), but does not account for time spent executing the `SET NX` call itself. When `sleep` is very small (e.g., 0.01s) and the SET call takes non-trivial time, `elapsed` underestimates actual wall time and the loop may significantly exceed `blocking_timeout`. Conversely, if `sleep` rounds introduce error, timing becomes imprecise.

**Fix:** Track actual wall-clock time using `asyncio.get_event_loop().time()` or `time.monotonic()`:
```python
import time

async def acquire(self, blocking=None, blocking_timeout=None):
    # ... (same as before until blocking loop)
    deadline = time.monotonic() + blocking_timeout if blocking_timeout is not None else None
    while True:
        result = await self._client.set(self.name, token, px=px, nx=True)
        if result is True:
            self.token = token
            return True
        if deadline is not None and time.monotonic() >= deadline:
            return False
        await asyncio.sleep(self.sleep)
```

### WR-03: Pipeline halts execution on first command error

**File:** `python/burner_redis/pipeline.py:20-28`
**Issue:** The `execute()` method runs commands sequentially and propagates the first exception immediately, skipping remaining commands. In redis-py's pipeline, all commands are sent and executed; errors for individual commands are returned inline in the results list (as exception objects). This behavioral divergence means callers relying on all commands executing (e.g., cleanup operations after failures) will not get expected behavior.

**Fix:** Catch exceptions per-command and return them inline in the results list:
```python
async def execute(self):
    results = []
    for method_name, args, kwargs in self._commands:
        method = getattr(self._client, method_name)
        try:
            result = await method(*args, **kwargs)
            results.append(result)
        except Exception as e:
            results.append(e)
    self._commands = []
    return results
```
Note: Consider whether this change should be made now or deferred -- the current test suite (`test_pipeline_wrongtype_error`) expects the exception to propagate, so the test would need updating too.

## Info

### IN-01: Mutable default argument in Script.__call__

**File:** `python/burner_redis/__init__.py:140`
**Issue:** `async def __call__(self, keys=[], args=[], client=None)` uses mutable default arguments (`[]`). While not a bug in this case (the lists are only read, not mutated), it violates Python best practice and can cause subtle issues if the method is ever modified to mutate the defaults.

**Fix:**
```python
async def __call__(self, keys=None, args=None, client=None):
    keys = keys if keys is not None else []
    args = args if args is not None else []
```

### IN-02: Duplicate test functions across test files

**File:** `tests/test_coercion.py` and `tests/test_strings.py`
**Issue:** Multiple test functions are duplicated across `test_coercion.py` and `test_strings.py`: `test_set_coercion_int`, `test_set_coercion_float`, `test_set_coercion_bool_rejected`, `test_set_coercion_bool_false_rejected`, `test_scan_iter_all`, `test_scan_iter_pattern`, `test_setex_basic`, `test_setex_with_ttl`, `test_setex_coercion`, and `test_lock_error_hierarchy` (in `test_coercion.py` and `test_locking.py`). This increases test run time and creates maintenance burden -- if one copy is updated, the other may be forgotten.

**Fix:** Remove duplicates from `test_coercion.py` and keep the canonical versions in their feature-specific test files (`test_strings.py` and `test_locking.py`). Or consolidate `test_coercion.py` to only contain the pure `_coerce_value()` unit tests that don't overlap.

---

_Reviewed: 2026-04-14T12:00:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
