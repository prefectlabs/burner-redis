# T02: Add Python async bindings for EVAL, EVALSHA, SCRIPT LOAD, and SCRIPT EXISTS commands, plus a comprehensive pytest suite that validates Lua scripting end-to-end including redis.

**Slice:** S06 — **Milestone:** M001

## Legacy Summary

---
phase: 06-lua-scripting
plan: 02
subsystem: scripting-bindings
tags: [lua, eval, evalsha, script-load, script-exists, python-bindings, pytest]
dependency_graph:
  requires: [lua-engine, script-cache, redis-call-dispatch]
  provides: [python-eval, python-evalsha, python-script-load, python-script-exists]
  affects: [lib]
tech_stack:
  added: []
  patterns: [redis-value-to-py-conversion, numkeys-splitting, try-attach-for-gil]
key_files:
  created:
    - tests/test_scripting.py
  modified:
    - src/lib.rs
decisions:
  - Used Python::try_attach for GIL acquisition in async blocks (PyO3 0.28.3 pattern)
  - Used Py<PyAny> instead of PyObject type alias for return types
  - redis_value_to_py handles recursive Array conversion for nested Lua tables
metrics:
  duration: 4min
  completed: 2026-04-11
  tasks: 2
  files: 2
---

# Phase 06 Plan 02: Lua Scripting Python Bindings Summary

Python async bindings for EVAL, EVALSHA, SCRIPT LOAD, SCRIPT EXISTS with redis-py compatible signatures and comprehensive pytest suite validating all 5 LUA requirements including redis.call() dispatch to all data types.

## Commits

| Task | Name | Commit | Key Files |
|------|------|--------|-----------|
| 1 | Add Python async methods for eval, evalsha, script_load, script_exists | 9a42a0e | src/lib.rs |
| 2 | Comprehensive pytest suite for Lua scripting commands | 217fa3d | tests/test_scripting.py |

## Implementation Details

### Python Bindings (src/lib.rs)

- **redis_value_to_py()**: Recursive converter from RedisValue enum to Python objects (BulkString->bytes, Integer->int, Nil->None, Status->bytes, Error->exception, Array->list)
- **eval()**: `#[pyo3(signature = (script, numkeys, *keys_and_args))]` matching redis-py; splits first numkeys args as KEYS, rest as ARGV
- **evalsha()**: Same signature pattern as eval but takes SHA1 hex instead of script text; propagates NOSCRIPT error
- **script_load()**: Returns SHA1 hex string for cached script
- **script_exists()**: `#[pyo3(signature = (*args))]` accepting variadic SHA1 strings, returns list of bools

### Test Suite (tests/test_scripting.py)

- **37 tests** covering all 5 LUA requirements
- LUA-01 (8 tests): EVAL with string, integer, nil, table array, false, KEYS/ARGV splitting
- LUA-02 (4 tests): EVALSHA after script_load, after eval auto-cache, unknown SHA, with keys/args
- LUA-03 (14 tests): redis.call() for GET/SET/DEL/EXISTS, HSET/HGET/HDEL/HVALS, SADD/SMEMBERS/SISMEMBER/SREM, ZADD/ZRANGE/ZREM/ZRANGEBYSCORE/ZREMRANGEBYSCORE, XADD; redis.pcall() success/error; WRONGTYPE and unknown command errors
- LUA-04 (3 tests): SCRIPT LOAD returns 40-char hex, matches Python hashlib, idempotent
- LUA-05 (4 tests): SCRIPT EXISTS for loaded/not-loaded/multiple/auto-cached scripts

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Used Python::try_attach instead of Python::with_gil**
- **Found during:** Task 1
- **Issue:** PyO3 0.28.3 does not have `Python::with_gil`; it uses `Python::try_attach` which returns `Option<R>`
- **Fix:** Replaced `Python::with_gil(|py| ...)` with `Python::try_attach(|py| ...).ok_or_else(|| ...)?`
- **Files modified:** src/lib.rs
- **Commit:** 9a42a0e

**2. [Rule 1 - Bug] Used Py<PyAny> instead of PyObject type alias**
- **Found during:** Task 1
- **Issue:** `PyObject` is not directly in scope in PyO3 0.28.3; need to use `Py<PyAny>`
- **Fix:** Changed function signature and Vec type to use `Py<PyAny>`
- **Files modified:** src/lib.rs
- **Commit:** 9a42a0e

## Decisions Made

1. **Python::try_attach for GIL in async blocks** -- Consistent with existing patterns in the codebase (zrange, xread, etc. all use try_attach)
2. **Recursive redis_value_to_py** -- Handles arbitrarily nested Lua tables returned as RedisValue::Array containing more Arrays
3. **numkeys splitting in Python binding layer** -- Matches redis-py signature exactly: eval(script, numkeys, *keys_and_args)

## Known Stubs

None -- all methods are fully implemented and tested.

## Threat Surface Scan

No new threat surfaces beyond those documented in the plan's threat model. SHA1 hash validation for EVALSHA is handled by the store layer (returns NOSCRIPT for unknown hashes, T-06-05). No new network endpoints or file access patterns introduced.
