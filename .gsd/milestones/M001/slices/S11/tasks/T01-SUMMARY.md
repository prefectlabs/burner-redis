# T01: Implement the three known compatibility fixes: XREADGROUP blocking support, XCLAIM command, and XTRIM approximate parameter.

**Slice:** S11 — **Milestone:** M001

## Legacy Summary

---
phase: 11-close-redis-py-compatibility-gaps-for-pydocket-integration
plan: 01
subsystem: streams
tags: [xreadgroup, blocking, xclaim, xtrim, tokio-notify, consumer-groups, lua-dispatch]

# Dependency graph
requires:
  - phase: 05-stream-commands-and-consumer-groups
    provides: XADD, XREAD, XREADGROUP, XACK, XAUTOCLAIM, XTRIM store and PyO3 bindings
  - phase: 06-lua-scripting
    provides: LuaEngine::execute and dispatch_command for Lua script support
provides:
  - Blocking XREADGROUP via tokio::sync::Notify wake-on-XADD signaling
  - XCLAIM command across Store, PyO3, Pipeline, and Lua dispatch layers
  - XTRIM approximate parameter acceptance for redis-py compatibility
affects: [11-02, pydocket-integration]

# Tech tracking
tech-stack:
  added: [tokio::sync::Notify, tokio macros feature]
  patterns: [dispatch_command_inner wrapper for return type extension, format_xreadgroup_result helper for code deduplication, Cell-based flag propagation in Lua scope]

key-files:
  created: []
  modified: [src/store.rs, src/lib.rs, src/scripting.rs, python/burner_redis/pipeline.py, tests/test_streams.py, Cargo.toml, Cargo.lock]

key-decisions:
  - "Used dispatch_command_inner wrapper to avoid modifying all Ok(RedisValue) returns when adding XADD bool flag"
  - "Used std::cell::Cell<bool> for had_xadd flag propagation through Lua scope closures"
  - "Global stream_notify (single Notify for all streams) accepted -- spurious wakeups cause one O(1) re-read, acceptable for embedded use"
  - "XTRIM approximate parameter accepted but ignored -- embedded DB always trims exactly"

patterns-established:
  - "dispatch_command returns (RedisValue, bool) tuple via inner/outer wrapper pattern for extensible signaling"
  - "format_xreadgroup_result helper for consistent Python result formatting across blocking and non-blocking paths"

requirements-completed: [D-03, D-06, D-07, D-08]

# Metrics
duration: 8min
completed: 2026-04-14
---

# Phase 11 Plan 01: XREADGROUP Blocking, XCLAIM Command, XTRIM Approximate Summary

**XREADGROUP blocking via tokio::sync::Notify with Lua XADD wake-through, XCLAIM for PEL transfer across all 4 layers, XTRIM approximate parameter acceptance**

## Performance

- **Duration:** 8 min
- **Started:** 2026-04-14T17:36:19Z
- **Completed:** 2026-04-14T17:44:52Z
- **Tasks:** 2
- **Files modified:** 7

## Accomplishments
- XREADGROUP with block parameter now waits for new stream entries via tokio::select!, waking on XADD (both direct and from Lua scripts)
- XCLAIM fully implemented across Store, PyO3, Pipeline, and Lua dispatch layers with min_idle_time, idle, force, justid, retrycount support
- XTRIM accepts the approximate parameter for redis-py API compatibility
- 9 new tests added (3 blocking XREADGROUP + 5 XCLAIM + 1 XTRIM approximate), all 291 tests pass

## Task Commits

Each task was committed atomically:

1. **Task 1: XREADGROUP blocking support with tokio::sync::Notify** - `3a50130` (feat)
2. **Task 2: XCLAIM command implementation + XTRIM approximate parameter** - `0e5a071` (feat)

## Files Created/Modified
- `src/store.rs` - Added stream_notify field (Arc<Notify>), notify_waiters() in xadd, had_xadd propagation in eval/evalsha, XCLAIM method
- `src/lib.rs` - Blocking XREADGROUP via tokio::select!, format_xreadgroup_result helper, XCLAIM PyO3 binding, XTRIM approximate parameter
- `src/scripting.rs` - dispatch_command_inner wrapper with (RedisValue, bool) return, Cell-based had_xadd tracking, XCLAIM Lua dispatch
- `python/burner_redis/pipeline.py` - xclaim pipeline method, xtrim approximate parameter
- `tests/test_streams.py` - 9 new tests for blocking XREADGROUP, XCLAIM, XTRIM approximate
- `Cargo.toml` - Added tokio macros feature for select! macro
- `Cargo.lock` - Updated with tokio-macros dependency

## Decisions Made
- Used dispatch_command_inner wrapper pattern to avoid changing 50+ Ok(RedisValue) returns when extending dispatch_command return type to include XADD bool flag
- Used std::cell::Cell<bool> to propagate had_xadd flag through Lua scope closures (RefCell already used for data, Cell sufficient for single bool)
- Global stream_notify wakes all blocked readers on any XADD -- spurious wakeups acceptable for embedded single-process use (T-11-02)
- XTRIM approximate parameter accepted but ignored since embedded DB always trims exactly

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Added tokio macros feature for select! macro**
- **Found during:** Task 1 (XREADGROUP blocking)
- **Issue:** tokio::select! requires the `macros` feature which was not enabled in Cargo.toml
- **Fix:** Added `macros` to tokio features list, ran `cargo update tokio` to resolve tokio-macros dependency
- **Files modified:** Cargo.toml, Cargo.lock
- **Verification:** maturin develop compiles successfully
- **Committed in:** 3a50130 (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Minor -- adding a feature flag to an existing dependency. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- All three known compatibility gaps (blocking XREADGROUP, XCLAIM, XTRIM approximate) are resolved
- Plan 02 can proceed with additional gap fixes or integration testing
- pydocket test suite should now have better pass rates with these fixes

## Self-Check: PASSED

All 7 modified files exist. Both task commits (3a50130, 0e5a071) verified in git log. SUMMARY.md exists at expected path.

---
*Phase: 11-close-redis-py-compatibility-gaps-for-pydocket-integration*
*Completed: 2026-04-14*
