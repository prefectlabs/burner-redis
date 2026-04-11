---
phase: 04-key-expiration
plan: 01
subsystem: database
tags: [tokio, expiration, sweep, background-task, parking-lot]

# Dependency graph
requires:
  - phase: 01-foundation-and-string-commands
    provides: "Store with RwLock<HashMap<Bytes, ValueEntry>>, passive expiry on GET, TTL support"
provides:
  - "sweep_expired() method on Store for active key expiration"
  - "Background Tokio task spawned on BurnerRedis instantiation"
  - "Weak<Store> reference pattern for task lifecycle management"
affects: [05-stream-commands, 09-persistence]

# Tech tracking
tech-stack:
  added: []
  patterns: ["Background Tokio task with Weak<Arc> self-termination", "Bounded sweep sampling (20 keys max per cycle)"]

key-files:
  created: []
  modified:
    - src/store.rs
    - src/lib.rs

key-decisions:
  - "Single write lock for sweep instead of read-then-write to avoid race conditions"
  - "100ms sweep interval balances memory cleanup with CPU overhead"
  - "HashMap iteration order used as pseudo-random sampling (no explicit RNG needed)"

patterns-established:
  - "Background task pattern: Arc::downgrade + Weak::upgrade loop for self-terminating tasks"
  - "Bounded critical section: sweep checks at most 20 keys per cycle (T-04-01 mitigation)"

requirements-completed: [EXP-01, EXP-02, EXP-03]

# Metrics
duration: 3min
completed: 2026-04-11
---

# Phase 04 Plan 01: Active Key Expiration Summary

**Active expiration sweep with background Tokio task sampling 20 keys per 100ms cycle using Weak<Store> self-termination**

## Performance

- **Duration:** 3 min
- **Started:** 2026-04-11T01:15:00Z
- **Completed:** 2026-04-11T01:18:19Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments
- Added sweep_expired() method to Store that samples up to 20 keys with TTLs and removes expired ones in a single write lock acquisition
- Spawned background Tokio task in BurnerRedis::new() that calls sweep_expired() every 100ms using Weak<Store> for lifecycle management
- Task self-terminates when all Arc<Store> references are dropped (no resource leak)
- All 71 Rust tests and 101 Python integration tests pass

## Task Commits

Each task was committed atomically:

1. **Task 1: Add sweep_expired() method to Store** - `7aebcbf` (feat)
2. **Task 2: Spawn background sweep task in BurnerRedis::new()** - `44bba58` (feat)

## Files Created/Modified
- `src/store.rs` - Added sweep_expired() method and two unit tests (test_sweep_expired, test_sweep_max_20_keys)
- `src/lib.rs` - Modified BurnerRedis::new() to spawn background sweep task with Weak<Store> reference

## Decisions Made
- Used single write lock acquisition in sweep_expired() for efficiency (check + remove in one pass) rather than read-lock-then-write-lock pattern from plan's alternative approach
- HashMap iteration order serves as pseudo-random sampling -- no explicit RNG needed since HashMap internal hashing provides sufficient randomness
- 100ms sweep interval chosen per plan spec -- balances timely cleanup with low CPU overhead

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
- `cargo build` fails with linker errors for PyO3 cdylib (missing Python symbols) -- this is expected behavior for PyO3 projects. Used `cargo test` for Rust verification and `maturin develop` for full Python integration build. Not a real issue.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- Active expiration complements existing passive expiration -- expired keys now cleaned up even when never accessed
- Ready for Phase 04 Plan 02 (TTL commands: EXPIRE, PEXPIRE, TTL, PTTL)
- Background task pattern established for any future periodic maintenance tasks

## Self-Check: PASSED

- All key files exist (src/store.rs, src/lib.rs)
- Commit 7aebcbf found (Task 1)
- Commit 44bba58 found (Task 2)

---
*Phase: 04-key-expiration*
*Completed: 2026-04-11*
