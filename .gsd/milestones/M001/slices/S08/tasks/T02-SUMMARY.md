# T02: Wire persistence into the Python API: add `persistence_path` parameter to BurnerRedis constructor for auto-restore on startup and auto-save on shutdown, expose `save()` async method for manual persistence, and register an atexit handler for graceful shutdown persistence.

**Slice:** S08 — **Milestone:** M001

## Legacy Summary

---
phase: 08-persistence
plan: 02
subsystem: database
tags: [persistence, atexit, pyo3, python-api, save-restore]

# Dependency graph
requires:
  - phase: 08-persistence-01
    provides: Store::save(), Store::load_into(), PersistableStore snapshot types, crash-safe write pattern
provides:
  - BurnerRedis(persistence_path=...) constructor with auto-restore on startup
  - atexit handler for auto-save on graceful shutdown
  - save() async method for manual persistence
  - _save_sync() synchronous method for atexit
  - persistence_path read-only property
affects: [09-packaging-and-distribution]

# Tech tracking
tech-stack:
  added: []
  patterns: [PyCFunction::new_closure for atexit registration from Rust constructor, atexit exception suppression for T-08-06]

key-files:
  created: [tests/test_persistence.py]
  modified: [src/lib.rs]

key-decisions:
  - "Atexit registration in Rust via PyCFunction::new_closure -- avoids Python subclassing/wrapping complexity, captures Arc<Store> and path directly"
  - "atexit handler silently ignores save errors (let _ = ...) -- process exit must not be blocked by save failure (T-08-06 mitigation)"
  - "save() defaults to persistence_path then burner-redis.dat -- consistent with redis-py patterns"

patterns-established:
  - "PyCFunction::new_closure pattern for registering Python callbacks from Rust constructors"
  - "Graceful degradation: corrupt persistence file logs warning to stderr and starts empty"

requirements-completed: [PERS-01, PERS-02, PERS-03]

# Metrics
duration: 4min
completed: 2026-04-11
---

# Phase 8 Plan 2: Python Persistence API Summary

**BurnerRedis(persistence_path="...") auto-restore/auto-save with atexit handler registered from Rust via PyCFunction::new_closure**

## Performance

- **Duration:** 4 min
- **Started:** 2026-04-11T04:00:38Z
- **Completed:** 2026-04-11T04:04:43Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments
- BurnerRedis constructor accepts optional persistence_path for zero-config persistence
- Automatic data restore on startup from persistence file (missing file starts empty, corrupt file warns and starts empty)
- Atexit handler registered from Rust constructor via PyCFunction::new_closure for graceful shutdown persistence
- save() async method and _save_sync() synchronous method for manual persistence
- 12 integration tests covering all data types, edge cases, and all 4 PERS requirements
- Full regression suite: 250/250 tests pass

## Task Commits

Each task was committed atomically:

1. **Task 1: Add persistence_path to BurnerRedis constructor and save() method** - `4d64b35` (feat)
2. **Task 2: Add atexit handler and Python integration tests** - `d4247ad` (test)

## Files Created/Modified
- `src/lib.rs` - Added persistence_path field, modified constructor for auto-restore/atexit, added save()/\_save_sync()/persistence_path getter
- `tests/test_persistence.py` - 12 integration tests for persistence Python API

## Decisions Made
- **Atexit registration in Rust:** Used PyCFunction::new_closure to register atexit handler directly from the Rust constructor. This avoids needing Python subclassing (`#[pyclass(subclass)]`), wrapper classes, or factory functions. The closure captures `Arc<Store>` and the path string, providing direct access to the store for saving.
- **Silent atexit error handling:** The atexit closure uses `let _ = store.save(...)` to silently discard save errors. This mitigates T-08-06 (atexit handler failure should not block process exit).
- **save() path resolution:** save(path=None) checks explicit path arg first, then persistence_path, then defaults to "burner-redis.dat". Matches redis-py convention of sensible defaults.

## Deviations from Plan

None - plan executed exactly as written. The plan explored multiple approaches for atexit registration in its action description; the PyCFunction::new_closure approach (mentioned as the "FINAL DECISION" update in the plan) was implemented.

## Issues Encountered
None - plan executed as specified.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Phase 08 (Persistence) is fully complete: engine (Plan 01) + Python API (Plan 02)
- All data types round-trip correctly through MessagePack persistence
- Ready for Phase 09: Packaging and Distribution

## Self-Check: PASSED

All files exist, all commits verified.

---
*Phase: 08-persistence*
*Completed: 2026-04-11*
