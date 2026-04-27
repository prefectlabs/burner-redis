---
phase: 08-persistence
plan: 01
subsystem: database
tags: [serde, rmp-serde, messagepack, persistence, crash-safe]

# Dependency graph
requires:
  - phase: 05-stream-commands-and-consumer-groups
    provides: Stream, ConsumerGroup, Consumer, PendingEntry data structures
  - phase: 06-lua-scripting
    provides: Script cache (HashMap<String, String>) in Store
provides:
  - PersistableStore snapshot types for serializing Store to MessagePack
  - save_to_path with crash-safe write-tmp/fsync/rename pattern
  - load_from_path with missing-file and corrupt-file handling
  - Store::save() and Store::load_into() convenience methods
affects: [08-02-python-persistence-api]

# Tech tracking
tech-stack:
  added: [serde 1.0, rmp-serde 1.3]
  patterns: [PersistableStore snapshot pattern separating serde concerns from runtime types, crash-safe write-tmp/fsync/rename]

key-files:
  created: [src/persistence.rs]
  modified: [Cargo.toml, src/store.rs, src/lib.rs]

key-decisions:
  - "PersistableStore snapshot pattern: parallel types using Vec<u8>/u64 instead of Bytes/Instant to keep serde concerns separate from runtime"
  - "TTL persisted as milliseconds-remaining (relative duration) rather than absolute timestamp for portability"
  - "PendingEntry delivery_time reset to Instant::now() on load (conservative: treats all pending entries as freshly delivered)"

patterns-established:
  - "PersistableStore pattern: snapshot types mirror runtime types with serde-friendly primitives"
  - "Crash-safe file writes: write .tmp, fsync, atomic rename"

requirements-completed: [PERS-01, PERS-04]

# Metrics
duration: 4min
completed: 2026-04-11
---

# Phase 8 Plan 1: Persistence Engine Summary

**MessagePack persistence engine with crash-safe save/load using PersistableStore snapshot pattern and rmp-serde**

## Performance

- **Duration:** 4 min
- **Started:** 2026-04-11T03:53:45Z
- **Completed:** 2026-04-11T03:58:06Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- All Store data types (String, Hash, Set, SortedSet, Stream with ConsumerGroups) serializable to MessagePack via PersistableStore snapshot pattern
- Crash-safe persistence: write-tmp, fsync, atomic rename ensures partial writes never corrupt the target file
- Expired keys filtered out during save; script cache included in serialized output
- 4 new unit tests covering round-trip all types, missing file, corrupt file, and expired key exclusion

## Task Commits

Each task was committed atomically:

1. **Task 1: Add serde dependencies and derives to all Store data structures** - `95ff2b7` (feat)
2. **Task 2: Create persistence module with crash-safe save and load** - `cb8b0f1` (feat)

## Files Created/Modified
- `Cargo.toml` - Added serde 1.0 (with derive) and rmp-serde 1.3 dependencies
- `src/store.rs` - Added PersistableStore and related snapshot types with from_store/into_runtime conversions; added Store::save(), load_into(), data_write(), scripts_write() methods
- `src/persistence.rs` - New module with save_to_path (crash-safe), load_from_path, PersistenceError, and 4 unit tests
- `src/lib.rs` - Added `mod persistence` declaration

## Decisions Made
- **PersistableStore snapshot pattern:** Created parallel types (PersistableEntry, PersistableValueData, etc.) using Vec<u8> instead of Bytes and Option<u64> TTL instead of Option<Instant>. This cleanly separates serde concerns from runtime types, avoiding custom serde implementations on Bytes/Instant.
- **TTL as relative duration:** Persisted TTL as milliseconds-remaining rather than absolute timestamp. Portable and avoids clock drift issues on restore.
- **PendingEntry delivery_time reset on load:** Since Instant cannot be meaningfully persisted across restarts, delivery_time is set to Instant::now() on deserialization. This is conservative (treats entries as freshly delivered) and only affects XAUTOCLAIM idle time calculations.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed xadd/zadd/zrange test signatures**
- **Found during:** Task 2 (persistence tests)
- **Issue:** Test code used incorrect method signatures (xadd with string ID and Vec fields, zadd with 4 bools instead of 5, zrange without withscores parameter)
- **Fix:** Updated test calls to match actual Store API: xadd with HashMap and Option<StreamId>, zadd with 5 bool flags, zrange with withscores bool and tuple assertions
- **Files modified:** src/persistence.rs
- **Committed in:** cb8b0f1 (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** Minor test signature fix. No scope creep.

## Issues Encountered
None - plan executed as specified.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Persistence engine complete and tested at the Rust level
- Ready for Plan 02: Python API integration (BurnerRedis constructor persistence_path, atexit handler, Python save() method)
- Store::save() and Store::load_into() provide the bridge points Plan 02 will use

## Self-Check: PASSED

All files exist, all commits verified.

---
*Phase: 08-persistence*
*Completed: 2026-04-11*
