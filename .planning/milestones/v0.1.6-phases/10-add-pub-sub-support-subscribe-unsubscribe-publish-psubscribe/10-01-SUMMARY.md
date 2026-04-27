---
phase: 10-add-pub-sub-support
plan: 01
subsystem: pubsub
tags: [tokio-broadcast, glob-matching, pyo3, async, pubsub]

# Dependency graph
requires:
  - phase: 01-foundation-and-string-commands
    provides: Store struct, PyO3 BurnerRedis class, future_into_py pattern
provides:
  - PubSubRegistry with Tokio broadcast-based fan-out in Store
  - glob_match function for Redis-style pattern matching
  - subscribe/unsubscribe/publish/psubscribe/punsubscribe Store methods
  - PUBSUB CHANNELS/NUMSUB/NUMPAT introspection methods
  - 10 PyO3 async bindings on BurnerRedis for pub/sub commands
  - _subscribe_listener background Tokio task for Python asyncio.Queue bridging
affects: [10-02, lua-scripting-publish]

# Tech tracking
tech-stack:
  added: [tokio::sync::broadcast]
  patterns: [PubSubRegistry separate from keyspace RwLock, iterative glob matching with backtracking, Python::try_attach for GIL in background Tokio tasks]

key-files:
  created: [src/commands/pubsub.rs]
  modified: [src/store.rs, src/commands/mod.rs, src/lib.rs]

key-decisions:
  - "Used Python::try_attach (PyO3 0.28.3) instead of Python::with_gil for GIL acquisition in background Tokio tasks"
  - "PubSubRegistry uses its own RwLock independent of data RwLock to prevent deadlocks with Lua scripts"
  - "Broadcast channel capacity of 4096 messages with Lagged error handling (log and continue)"
  - "Iterative glob_match with star_pi/star_si backtracking prevents ReDoS (T-10-01)"

patterns-established:
  - "pub(crate) pubsub field on Store for cross-module access from lib.rs"
  - "Background Tokio task pattern: tokio::spawn with Python::try_attach for pushing messages to asyncio.Queue"

requirements-completed: [PUBSUB-01, PUBSUB-02, PUBSUB-03, PUBSUB-04, PUBSUB-05, PUBSUB-06]

# Metrics
duration: 4min
completed: 2026-04-14
---

# Phase 10 Plan 01: Rust Pub/Sub Engine Summary

**Tokio broadcast-based pub/sub engine with PubSubRegistry, iterative glob_match, and 10 PyO3 async bindings on BurnerRedis**

## Performance

- **Duration:** 4 min
- **Started:** 2026-04-14T02:46:52Z
- **Completed:** 2026-04-14T02:51:29Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- PubSubRegistry with Tokio broadcast channel for message fan-out, subscriber tracking via dual HashMap index (channel->subscribers and subscriber->channels)
- Iterative glob_match implementation resistant to ReDoS attacks, verified by 20 Rust unit tests covering all Redis glob syntax features
- 10 PyO3 async methods on BurnerRedis: publish, _new_subscriber, _subscribe_listener, subscribe_channels, unsubscribe_channels, psubscribe_patterns, punsubscribe_patterns, pubsub_channels, pubsub_numsub, pubsub_numpat
- Background Tokio task in _subscribe_listener bridges Rust broadcast to Python asyncio.Queue via put_nowait

## Task Commits

Each task was committed atomically:

1. **Task 1: PubSubRegistry in Store with subscribe/unsubscribe/publish methods and glob_match** - `b78026d` (feat)
2. **Task 2: PyO3 async bindings on BurnerRedis for all pub/sub commands** - `836e839` (feat)

## Files Created/Modified
- `src/commands/pubsub.rs` - Created: glob_match function with iterative backtracking algorithm and 20 unit tests
- `src/store.rs` - Modified: PubSubMessage struct, PubSubRegistry struct, pubsub RwLock field on Store, 10 pub/sub methods (new_subscriber, subscribe, unsubscribe, psubscribe, punsubscribe, publish, pubsub_channels, pubsub_numsub, pubsub_numpat, pubsub_sender)
- `src/commands/mod.rs` - Modified: added `pub mod pubsub` declaration
- `src/lib.rs` - Modified: 10 new #[pymethods] for pub/sub commands with async Python bindings

## Decisions Made
- Used Python::try_attach (PyO3 0.28.3 API) instead of Python::with_gil (deprecated/removed) for GIL acquisition in background Tokio tasks -- consistent with existing codebase patterns
- PubSubRegistry uses its own RwLock independent of the data RwLock -- prevents deadlocks when PUBLISH is called from Lua scripts (which hold the data write lock)
- Broadcast channel capacity set to 4096 messages -- RecvError::Lagged handled with eprintln warning and continue (fire-and-forget semantics, T-10-02 mitigation)
- Iterative glob_match with star_pi/star_si backtracking variables bounds complexity to O(m*n) -- prevents stack overflow and exponential blowup on adversarial patterns (T-10-01 mitigation)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed Python::with_gil to Python::try_attach**
- **Found during:** Task 2 (PyO3 async bindings)
- **Issue:** Plan specified `Python::with_gil` for GIL acquisition in background Tokio task, but PyO3 0.28.3 does not have `with_gil` -- it uses `try_attach` instead
- **Fix:** Replaced `Python::with_gil(|py| { ... })` with `Python::try_attach(|py| { ... })` which returns `Option<R>` and matches the existing codebase pattern
- **Files modified:** src/lib.rs
- **Verification:** cargo check passes, pattern consistent with existing PyO3 0.28.3 usage in codebase
- **Committed in:** 836e839 (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 bug fix)
**Impact on plan:** Necessary API correction for PyO3 0.28.3 compatibility. No scope creep.

## Issues Encountered
None beyond the PyO3 API deviation noted above.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Rust-side pub/sub engine is complete and ready for Plan 02 (Python PubSub class)
- All Store methods and PyO3 bindings are in place for the Python wrapper to consume
- pubsub_sender() method available for future Lua PUBLISH integration
- pub(crate) visibility on pubsub field enables _subscribe_listener access from lib.rs

## Self-Check: PASSED

- All 4 files verified present (1 created, 3 modified)
- Both task commits verified: b78026d, 836e839
- SUMMARY.md verified present
- All 29 acceptance criteria pass
- cargo check passes, cargo test --lib passes (96 tests, 0 failures)

---
*Phase: 10-add-pub-sub-support*
*Completed: 2026-04-14*
