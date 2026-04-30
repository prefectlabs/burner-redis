# T01: Add Rust-layer Store methods and PyO3 bindings for keys(pattern), ttl(name), mget(*keys), and xpending summary form.

**Slice:** S12 — **Milestone:** M001

## Legacy Summary

---
phase: 12-close-remaining-redis-py-compatibility-gaps-for-drop-in-repl
plan: 01
subsystem: api
tags: [redis, glob, keys, ttl, mget, xpending, pyo3]

# Dependency graph
requires:
  - phase: 05-stream-commands-and-consumer-groups
    provides: Stream data structures, consumer groups, xreadgroup, xpending_range
  - phase: 10-add-pub-sub-support-subscribe-unsubscribe-publish-psubscribe
    provides: glob_match function in pubsub.rs
provides:
  - "glob_match with [a-z] character range support (D-04)"
  - "Store.keys() method for key enumeration with glob filtering"
  - "Store.ttl() method returning Redis-compatible TTL semantics"
  - "Store.mget() method for atomic multi-key reads"
  - "Store.xpending_summary() method for aggregated pending message info"
  - "PyO3 async bindings for keys, ttl, mget, xpending"
affects: [12-02, python-api, redis-compatibility]

# Tech tracking
tech-stack:
  added: []
  patterns: [glob-range-matching, multi-key-read-pattern, xpending-summary-aggregation]

key-files:
  created: []
  modified:
    - src/commands/pubsub.rs
    - src/store.rs
    - src/lib.rs

key-decisions:
  - "Used write lock for ttl() to enable passive expiration cleanup on access (T-12-04 mitigation)"
  - "mget() uses single read lock for atomicity across all keys"
  - "xpending_summary uses write lock matching xpending_range pattern for passive expiration"

patterns-established:
  - "Key enumeration: iterate HashMap with glob_match filter, skip expired entries"
  - "Multi-key atomic reads: single read lock, map over keys with None for missing/wrong-type"

requirements-completed: [D-03, D-04, D-10, D-11, D-13]

# Metrics
duration: 8min
completed: 2026-04-14
---

# Phase 12 Plan 01: Rust Store Methods and PyO3 Bindings Summary

**Enhanced glob_match with [a-z] range support and added keys(), ttl(), mget(), xpending_summary() Store methods with PyO3 async bindings**

## Performance

- **Duration:** 8 min
- **Started:** 2026-04-14T21:29:43Z
- **Completed:** 2026-04-14T21:37:35Z
- **Tasks:** 2
- **Files modified:** 3

## Accomplishments
- Enhanced glob_match to support character ranges like [a-z], [0-9] inside character classes (D-04)
- Added four new Store methods: keys(), ttl(), mget(), xpending_summary() with full Redis-compatible semantics
- Added four PyO3 async bindings exposing all new methods to Python callers
- All 113 Rust tests pass including 17 new tests for the added functionality

## Task Commits

Each task was committed atomically:

1. **Task 1: Enhance glob_match and add Store methods** - `6c6d267` (test: failing tests) + `7e73cc7` (feat: implementation)
2. **Task 2: Add PyO3 async bindings** - `39e33a9` (feat)

_Note: Task 1 followed TDD with separate test and implementation commits_

## Files Created/Modified
- `src/commands/pubsub.rs` - Enhanced glob_match with [a-z] character range support inside character class parsing
- `src/store.rs` - Added keys(), ttl(), mget(), xpending_summary() Store methods with Rust unit tests
- `src/lib.rs` - Added PyO3 async bindings for keys, ttl, mget, xpending

## Decisions Made
- Used write lock for ttl() to enable passive expiration cleanup on access, matching the existing get() pattern and mitigating T-12-04
- mget() uses a single read lock for atomicity -- all key reads happen under one lock acquisition
- xpending_summary() follows the same write-lock + passive-expiration pattern as xpending_range()
- Fixed test signatures to match actual xadd(key, fields_hashmap, id) and xreadgroup(group, consumer, keys, ids, count) APIs

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed xpending_summary test method signatures**
- **Found during:** Task 1 (TDD GREEN phase)
- **Issue:** Plan-provided test code used incorrect method signatures for xadd and xreadgroup (wrong arg order, wrong types)
- **Fix:** Updated tests to match actual API: xadd(Bytes, HashMap, Option<StreamId>) and xreadgroup with separate keys/ids slices
- **Files modified:** src/store.rs (test section)
- **Verification:** All 113 tests pass
- **Committed in:** 7e73cc7 (Task 1 commit)

**2. [Rule 1 - Bug] Fixed PyObject type reference in xpending binding**
- **Found during:** Task 2 (PyO3 bindings)
- **Issue:** Plan used `PyObject` type which is not in scope; codebase uses `Py<PyAny>` pattern
- **Fix:** Changed `PyResult<PyObject>` to `PyResult<Py<PyAny>>` matching existing code patterns
- **Files modified:** src/lib.rs
- **Verification:** Compilation succeeds, maturin builds wheel
- **Committed in:** 39e33a9 (Task 2 commit)

---

**Total deviations:** 2 auto-fixed (2 bugs)
**Impact on plan:** Both fixes corrected plan-provided code to match actual codebase APIs. No scope change.

## Issues Encountered
None beyond the auto-fixed deviations above.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- All four Rust methods are available and tested, ready for Python-layer wrappers in Plan 02
- PyO3 bindings are active and callable from async Python code
- glob_match range support enables accurate key pattern matching

## Self-Check: PASSED

All files exist, all commits verified, all acceptance criteria met. 113 Rust tests pass. Python module builds and imports successfully.

---
*Phase: 12-close-remaining-redis-py-compatibility-gaps-for-drop-in-repl*
*Completed: 2026-04-14*
