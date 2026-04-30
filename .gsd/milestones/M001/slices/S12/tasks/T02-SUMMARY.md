# T02: Add Python-layer value coercion, exception hierarchy alignment, convenience commands (scan_iter, setex), pipeline stubs for all new commands, and comprehensive test coverage for the entire Phase 12 feature set.

**Slice:** S12 — **Milestone:** M001

## Legacy Summary

---
phase: 12-close-remaining-redis-py-compatibility-gaps-for-drop-in-repl
plan: 02
subsystem: python-api
tags: [redis, coercion, exceptions, scan_iter, setex, pipeline, compatibility]

# Dependency graph
requires:
  - phase: 12-close-remaining-redis-py-compatibility-gaps-for-drop-in-repl
    plan: 01
    provides: Rust Store methods (keys, ttl, mget, xpending_summary) and PyO3 bindings
provides:
  - "_coerce_value helper for redis-py compatible value encoding (D-01, D-02)"
  - "set() monkey-patch with coercion before Rust call"
  - "setex(name, time, value) convenience wrapper (D-12)"
  - "scan_iter(match=pattern) async generator (D-05)"
  - "LockError conditional subclassing from redis.exceptions.LockError (D-06)"
  - "NoScriptError conditional subclassing from redis.exceptions.NoScriptError (D-07)"
  - "Pipeline stubs for keys, ttl, mget, setex, xpending (D-09)"
  - "Comprehensive test coverage for all Phase 12 features"
affects: [python-api, redis-compatibility, pipeline]

# Tech tracking
tech-stack:
  added: []
  patterns: [value-coercion, conditional-exception-subclassing, async-generator-wrapper, monkey-patch-with-original-ref]

key-files:
  created:
    - tests/test_coercion.py
  modified:
    - python/burner_redis/__init__.py
    - python/burner_redis/lock.py
    - python/burner_redis/pipeline.py
    - tests/test_strings.py
    - tests/test_locking.py
    - tests/test_expiration.py
    - tests/test_streams.py
    - tests/test_pipeline.py

key-decisions:
  - "Bool check before int check in _coerce_value since bool is subclass of int in Python"
  - "Used repr().encode() for int/float coercion to match redis-py Encoder.encode() behavior"
  - "scan_iter pipeline stub raises NotImplementedError since async generators cannot be pipelined"
  - "Preserved _original_set reference for coerced set wrapper to avoid infinite recursion"

patterns-established:
  - "Value coercion: _coerce_value rejects bools, coerces int/float via repr().encode()"
  - "Conditional exception hierarchy: define base class, then redefine as subclass of redis.exceptions.X"
  - "Async generator wrapper: scan_iter wraps keys() call with yield loop"

requirements-completed: [D-01, D-02, D-05, D-06, D-07, D-08, D-09, D-12]

# Metrics
duration: 6min
completed: 2026-04-14
---

# Phase 12 Plan 02: Python Layer Coercion, Exceptions, and Test Coverage Summary

**Value coercion rejecting bools and encoding int/float, LockError/NoScriptError exception hierarchy alignment, scan_iter async generator, setex wrapper, pipeline stubs, and comprehensive test coverage for all Phase 12 features**

## Performance

- **Duration:** 6 min
- **Started:** 2026-04-14T21:53:33Z
- **Completed:** 2026-04-14T21:59:11Z
- **Tasks:** 2
- **Files modified:** 9

## Accomplishments
- Added _coerce_value() helper that rejects bools and coerces int/float to bytes via repr().encode(), matching redis-py Encoder.encode() behavior (D-01, D-02)
- Monkey-patched BurnerRedis.set() to apply value coercion before calling Rust layer
- Added setex(name, time, value) convenience wrapper delegating to set() with ex=time (D-12)
- Added scan_iter(match=pattern) async generator wrapping keys() for redis-py compatibility (D-05)
- Aligned LockError to conditionally subclass redis.exceptions.LockError (D-06)
- Added NoScriptError with conditional redis.exceptions.NoScriptError subclassing (D-07)
- Added pipeline stubs for keys, ttl, mget, setex, xpending (D-09)
- Added comprehensive test coverage: 339 total tests pass, 0 failures

## Task Commits

Each task was committed atomically:

1. **Task 1: Value coercion, exceptions, scan_iter, setex** - `7728ee7` (test: failing tests) + `b82ac25` (feat: implementation)
2. **Task 2: Pipeline stubs and comprehensive tests** - `feba16b` (feat: stubs and tests)

_Task 1 followed TDD with separate test and implementation commits_

## Files Created/Modified
- `python/burner_redis/__init__.py` - Added _coerce_value(), coerced set() wrapper, setex(), scan_iter(), NoScriptError
- `python/burner_redis/lock.py` - LockError conditional subclassing from redis.exceptions.LockError
- `python/burner_redis/pipeline.py` - Added keys, ttl, mget, setex, xpending stubs
- `tests/test_coercion.py` - Focused tests for _coerce_value, LockError hierarchy, scan_iter, setex
- `tests/test_strings.py` - Added coercion, keys, scan_iter, setex, mget tests
- `tests/test_locking.py` - Added LockError hierarchy test
- `tests/test_expiration.py` - Added TTL command tests (missing key, no expiry, with expiry)
- `tests/test_streams.py` - Added xpending summary tests (with pending, empty)
- `tests/test_pipeline.py` - Added pipeline integration tests for new commands

## Decisions Made
- Bool check before int check in _coerce_value since bool is subclass of int in Python
- Used repr().encode() for int/float coercion to match redis-py Encoder.encode() behavior
- scan_iter pipeline stub raises NotImplementedError since async generators cannot be pipelined (matches redis-py behavior)
- Preserved _original_set reference for coerced set wrapper to avoid infinite recursion

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
- pytest and pytest-asyncio not installed in worktree venv (resolved with uv pip install)
- redis package not installed for exception hierarchy tests (resolved with uv pip install redis)

## User Setup Required
None - no external service configuration required.

## Self-Check: PASSED

All 10 files verified present. All 3 commits verified in git log. All acceptance criteria confirmed. 339 tests pass with 0 failures.
