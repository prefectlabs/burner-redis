---
gsd_state_version: 1.0
milestone: v1.0
milestone_name: milestone
status: executing
stopped_at: Phase 10 context gathered
last_updated: "2026-04-14T03:09:04.225Z"
last_activity: 2026-04-14
progress:
  total_phases: 10
  completed_phases: 10
  total_plans: 21
  completed_plans: 21
  percent: 100
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-04-10)

**Core value:** A self-hosted Prefect server can start, run flows, and manage state using this library instead of an external Redis server
**Current focus:** Phase 10 — add-pub-sub-support-subscribe-unsubscribe-publish-psubscribe

## Current Position

Phase: 10
Plan: Not started
Status: Executing Phase 10
Last activity: 2026-04-14

Progress: [░░░░░░░░░░] 0%

## Performance Metrics

**Velocity:**

- Total plans completed: 21
- Average duration: -
- Total execution time: 0 hours

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
|-------|-------|-------|----------|
| 01 | 2 | - | - |
| 02 | 2 | - | - |
| 03 | 2 | - | - |
| 04 | 2 | - | - |
| 05 | 3 | - | - |
| 06 | 2 | - | - |
| 07 | 2 | - | - |
| 08 | 2 | - | - |
| 09 | 2 | - | - |
| 10 | 2 | - | - |

**Recent Trend:**

- Last 5 plans: -
- Trend: -

*Updated after each plan completion*
| Phase 01 P01 | 5min | 2 tasks | 8 files |
| Phase 01 P02 | 7min | 2 tasks | 4 files |
| Phase 02 P01 | 3min | 2 tasks | 4 files |
| Phase 02 P02 | 4min | 2 tasks | 4 files |
| Phase 03 P01 | 4min | 2 tasks | 4 files |
| Phase 03 P02 | 5min | 2 tasks | 3 files |
| Phase 04 P01 | 3min | 2 tasks | 2 files |
| Phase 04 P02 | 3min | 2 tasks | 1 files |
| Phase 05 P01 | 5min | 2 tasks | 5 files |
| Phase 05 P02 | 5min | 2 tasks | 3 files |
| Phase 05 P03 | 4min | 2 tasks | 3 files |
| Phase 06 P01 | 8min | 2 tasks | 4 files |
| Phase 06 P02 | 4min | 2 tasks | 2 files |
| Phase 07 P01 | 3min | 2 tasks | 3 files |
| Phase 07 P02 | 3min | 2 tasks | 3 files |
| Phase 08 P01 | 4min | 2 tasks | 4 files |
| Phase 08 P02 | 4min | 2 tasks | 2 files |
| Phase 09 P01 | 1min | 2 tasks | 2 files |
| Phase 09 P02 | 3min | 2 tasks | 3 files |

## Accumulated Context

### Decisions

Decisions are logged in PROJECT.md Key Decisions table.
Recent decisions affecting current work:

- Roadmap: 9 phases derived from 53 requirements with fine granularity
- Roadmap: Phases 2/3/4 can execute in parallel (all depend only on Phase 1)
- [Phase 01]: Used tokio::runtime::Builder variable pattern for pyo3_async_runtimes::tokio::init owned Builder requirement
- [Phase 01]: Added python-source = python to pyproject.toml for maturin mixed python/rust project layout
- [Phase 01]: Switched Tokio runtime from current-thread to multi-thread for future_into_py compatibility
- [Phase 01]: Used owned String/Vec<u8> extraction instead of borrowed &str/&[u8] for PyO3 0.28.3 abi3 compatibility
- [Phase 02]: ValueData enum with 3 variants over separate maps per type -- keeps single keyspace with RwLock, matches Redis single-key model
- [Phase 02]: GET returns None for non-string types -- WRONGTYPE error raised at Python layer for API compatibility
- [Phase 02]: Used generic PyException with WRONGTYPE message string for error conversion -- keeps Rust layer simple
- [Phase 02]: SMEMBERS returns HashSet<Vec<u8>> from Rust for automatic PyO3 conversion to Python set type
- [Phase 02]: ResponseError class with conditional redis.exceptions subclassing for compatibility
- [Phase 03]: Used OrderedFloat<f64> for BTreeMap key ordering -- handles NaN correctly for sorted set score-based range queries
- [Phase 03]: Dual-index SortedSet pattern (BTreeMap + HashMap) matches Redis skiplist+dict for O(1) member lookup and O(log n) range queries
- [Phase 03]: Used Python::try_attach (PyO3 0.28.3) for conditional return types in async blocks -- withscores changes output between list[bytes] and list[tuple[bytes, float]]
- [Phase 04]: Single write lock for sweep_expired() instead of read-then-write to avoid race conditions and improve efficiency
- [Phase 04]: Background Tokio task with Weak<Store> reference for self-terminating active expiration sweep at 100ms interval
- [Phase 04]: Focused expiration tests on string keys only since SET is the only command supporting EX/PX; hash/set/sorted-set TTL requires future EXPIRE command
- [Phase 05]: Used BTreeMap<(u64,u64), HashMap<Bytes,Bytes>> for stream entries -- ordered insertion and efficient range queries for XREAD
- [Phase 05]: Scaffolded ConsumerGroup/Consumer/PendingEntry structs in Plan 01 to avoid ValueData enum changes in Plan 02
- [Phase 05]: XREAD returns None for empty results and skips non-existent streams, matching redis-py behavior
- [Phase 05]: Used sentinel (u64::MAX, u64::MAX) for dollar ID resolution in XGROUP CREATE -- avoids string passing through store layer
- [Phase 05]: XACK iterates all consumers to find PEL entry -- O(consumers) but simple and correct for embedded use
- [Phase 05]: XAUTOCLAIM scans all consumers PELs linearly -- acceptable for embedded single-process use
- [Phase 05]: XINFO idle time uses min PEL entry duration -- represents most recent consumer activity
- [Phase 06]: Used mlua 0.10 with vendored Lua 5.4 to avoid system library dependency
- [Phase 06]: dispatch_command operates directly on raw HashMap (not Store methods) because caller already holds write lock
- [Phase 06]: Lock ordering enforced: scripts lock released before data lock acquired (deadlock prevention)
- [Phase 06]: Used Python::try_attach for GIL in async blocks (PyO3 0.28.3 pattern, consistent with existing codebase)
- [Phase 06]: redis_value_to_py recursive converter handles nested Lua tables as arbitrarily deep PyList structures
- [Phase 07]: Monkey-patch BurnerRedis.pipeline() in __init__.py instead of Rust-side method -- pure Python Pipeline class
- [Phase 07]: Pipeline command methods are synchronous (buffer-only), only execute() is async -- matches redis-py Pipeline behavior
- [Phase 07]: Token-based lock ownership using UUID strings compared against GET result bytes for safe release verification
- [Phase 07]: Non-atomic GET-then-DELETE for lock release acceptable for in-process embedded database with no network partitions
- [Phase 07]: Monkey-patch BurnerRedis.lock() in __init__.py consistent with pipeline() factory pattern
- [Phase 08]: PersistableStore snapshot pattern: parallel types using Vec<u8>/u64 instead of Bytes/Instant to keep serde concerns separate from runtime
- [Phase 08]: TTL persisted as milliseconds-remaining (relative duration) rather than absolute timestamp for portability
- [Phase 08]: PendingEntry delivery_time reset to Instant::now() on load -- conservative for XAUTOCLAIM idle time calculations
- [Phase 08]: Atexit registration in Rust via PyCFunction::new_closure -- captures Arc<Store> and path, avoids Python subclassing
- [Phase 08]: atexit handler silently ignores save errors (let _ = ...) to not block process exit (T-08-06)
- [Phase 08]: save() path resolution: explicit arg > persistence_path > burner-redis.dat default
- [Phase 09]: 4-target build matrix: linux x86_64/aarch64 + macOS x86_64/arm64 (no Windows)
- [Phase 09]: No caching or sccache -- keep CI workflow simple for initial version
- [Phase 09]: PyPI auth via MATURIN_PYPI_TOKEN secret with OIDC id-token permission for future trusted publisher migration
- [Phase 09]: GitHub Release uses softprops/action-gh-release@v2 with auto-generated release notes

### Pending Todos

None yet.

### Roadmap Evolution

- Phase 10 added: Add PUB/SUB support (SUBSCRIBE, UNSUBSCRIBE, PUBLISH, PSUBSCRIBE, PUNSUBSCRIBE, and message dispatch)

### Blockers/Concerns

None yet.

### Quick Tasks Completed

| # | Description | Date | Commit | Directory |
|---|-------------|------|--------|-----------|
| 260411-b8i | Add integration tests that simulate how Prefect servers use Redis to verify compatibility | 2026-04-11 | 5fd90f5 | [260411-b8i-add-integration-tests-that-simulate-how-](./quick/260411-b8i-add-integration-tests-that-simulate-how-/) |
| 260411-ipj | Run integration tests in CI on merge to main | 2026-04-11 | 577c6e8 | [260411-ipj-run-integration-tests-in-ci-on-merge-to-](./quick/260411-ipj-run-integration-tests-in-ci-on-merge-to-/) |

## Session Continuity

Last session: 2026-04-14T02:10:09.205Z
Stopped at: Phase 10 context gathered
Resume file: .planning/phases/10-add-pub-sub-support-subscribe-unsubscribe-publish-psubscribe/10-CONTEXT.md
