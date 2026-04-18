---
gsd_state_version: 1.0
milestone: v1.0
milestone_name: milestone
status: awaiting-checkpoint
stopped_at: Phase 13 Plan 03 Task 1 complete — recipe.yaml drafted + conda-smithy lint clean; pausing at Task 2 (checkpoint:human-verify — developer must fork conda-forge/staged-recipes, push recipe, open PR)
last_updated: "2026-04-18T03:19:00Z"
last_activity: 2026-04-18 -- Phase 13 Plan 03 Task 1 commit 665ca40; recipe draft at /tmp/phase-13-staged-recipes/recipe-draft.yaml; maintainer corrected from ajstreed (404) to desertaxle (verified); checkpoint awaiting developer action
progress:
  total_phases: 13
  completed_phases: 12
  total_plans: 28
  completed_plans: 27
  percent: 96
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-04-10)

**Core value:** A self-hosted Prefect server can start, run flows, and manage state using this library instead of an external Redis server
**Current focus:** Phase 13 — publish-burner-redis-to-conda-forge

## Current Position

Phase: 13 (publish-burner-redis-to-conda-forge) — EXECUTING (checkpoint pending)
Plan: 3 of 3
Status: Plan 03 Task 1 of 4 complete (recipe drafted + lint clean); paused at Task 2 checkpoint:human-verify (developer must fork conda-forge/staged-recipes, push recipe-draft.yaml, open PR)
Last activity: 2026-04-18 -- Phase 13 Plan 03 Task 1 commit 665ca40; waiting on developer to open staged-recipes PR

Progress: [██████░░░░] 67%

## Performance Metrics

**Velocity:**

- Total plans completed: 25
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
| 11 | 2 | - | - |
| 12 | 2 | - | - |

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
| Phase 13 P01 | 3min | 2 tasks | 1 files |
| Phase 13 P02 | 3min | 1 tasks | 2 files |

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
- [Phase 13]: 0.1.2 sdist passed feedstock-readiness audit — no pyproject.toml fix needed, no 0.1.3 release cut (pinned_version=0.1.2 for conda-forge recipe)
- [Phase 13]: maturin 1.x ships Cargo.lock in sdist by default — no explicit `[tool.maturin].include` needed
- [Phase 13]: No `cargo vendor` in sdist — `CARGO_NET_OFFLINE=true` + pre-populated cargo cache proves offline-build capability, which is strictly harder than conda-forge CI's actual network posture
- [Phase 13]: Pinned cargo-bundle-licenses to 4.0.0 — latest 4.2.0 requires rustc 1.86 (via cargo_metadata 0.23); our toolchain is 1.85 (edition 2024 MSRV). 4.0.0 emits equivalent YAML schema with `package_name:` field
- [Phase 13]: All 57 bundled Rust crates fall in the permissive license set (MIT / Apache-2.0 / Unlicense / Unicode-3.0 / Apache-2.0 WITH LLVM-exception) — no GPL/AGPL/MPL/proprietary; no dep upgrade or swap required
- [Phase 13]: mlua-sys 0.6.8 `text: NOT FOUND` is cosmetic — SPDX ID is cleanly `MIT` in Cargo.toml; LICENSE text lives at mlua workspace repo root, not in the subcrate dir (standard Rust-workspace packaging quirk)

### Pending Todos

None yet.

### Roadmap Evolution

- Phase 10 added: Add PUB/SUB support (SUBSCRIBE, UNSUBSCRIBE, PUBLISH, PSUBSCRIBE, PUNSUBSCRIBE, and message dispatch)
- Phase 11 added: Close redis-py compatibility gaps for pydocket integration
- Phase 12 added: Close remaining redis-py compatibility gaps for drop-in replacement
- Phase 13 added: Publish burner-redis to conda-forge (pre-plan context committed in CONTEXT.md from 2026-04-17 brainstorm; absorbs three pending todos: verify-sdist-contains-cargo-lock, audit-rust-dep-licenses, submit-conda-forge-feedstock)

### Blockers/Concerns

- [Phase 13 Plan 03] Awaiting developer action on Task 2 checkpoint: fork `conda-forge/staged-recipes`, push `/tmp/phase-13-staged-recipes/recipe-draft.yaml` to `recipes/burner-redis/recipe.yaml` on a branch, open PR. Resume file: `.planning/phases/13-publish-burner-redis-to-conda-forge/13-03-PLAN.md`. Submission note: `.planning/notes/phase-13-feedstock-submission.md` (frontmatter `staged_recipes_pr_url` field must be filled in after push).

### Quick Tasks Completed

| # | Description | Date | Commit | Directory |
|---|-------------|------|--------|-----------|
| 260411-b8i | Add integration tests that simulate how Prefect servers use Redis to verify compatibility | 2026-04-11 | 5fd90f5 | [260411-b8i-add-integration-tests-that-simulate-how-](./quick/260411-b8i-add-integration-tests-that-simulate-how-/) |
| 260411-ipj | Run integration tests in CI on merge to main | 2026-04-11 | 577c6e8 | [260411-ipj-run-integration-tests-in-ci-on-merge-to-](./quick/260411-ipj-run-integration-tests-in-ci-on-merge-to-/) |
| 260413-vbg | Update the integration tests that ensure compatibility with pydocket include PUB/SUB coverage | 2026-04-14 | 4cc3619 | [260413-vbg-update-the-integration-tests-that-ensure](./quick/260413-vbg-update-the-integration-tests-that-ensure/) |
| 260414-9ub | Update pydocket to use burner-redis and run its test suite to verify compatibility | 2026-04-14 | 842ee25 | [260414-9ub-update-pydocket-to-use-burner-redis-and-](./quick/260414-9ub-update-pydocket-to-use-burner-redis-and-/) |
| 260414-ap2 | Implement xpending_range | 2026-04-14 | d47e426 | [260414-ap2-implement-xpending-range](./quick/260414-ap2-implement-xpending-range/) |
| 260414-tgx | Fix 3 redis-py compatibility gaps causing docket test failures | 2026-04-15 | 44b8826 | [260414-tgx-fix-3-redis-py-compatibility-gaps-causin](./quick/260414-tgx-fix-3-redis-py-compatibility-gaps-causin/) |
| 260415-an2 | Eliminate async overhead with sync fast path and native pipeline execution | 2026-04-15 | 9e1fa38 | [260415-an2-eliminate-async-overhead-with-sync-fast-](./quick/260415-an2-eliminate-async-overhead-with-sync-fast-/) |
| 260415-gtu | Add MIT LICENSE file and set up dynamic versioning | 2026-04-15 | a0ae4da | [260415-gtu-add-mit-license-file-and-set-up-dynamic-](./quick/260415-gtu-add-mit-license-file-and-set-up-dynamic-/) |
| 260415-hb8 | Update PyPI publishing to use trusted publishers OIDC | 2026-04-15 | 3cfe3b6 | [260415-hb8-update-pypi-publishing-to-use-trusted-pu](./quick/260415-hb8-update-pypi-publishing-to-use-trusted-pu/) |
| 260415-mn4 | Fix asyncio.wait_for cancellation bug in pubsub get_message | 2026-04-15 | a8161ab | [260415-mn4-fix-asyncio-wait-for-cancellation-bug-in](./quick/260415-mn4-fix-asyncio-wait-for-cancellation-bug-in/) |
| 260415-tc2 | Add pydocket test suite CI workflow on merge and PR to main | 2026-04-16 | b4739d7 | [260415-tc2-add-pydocket-test-suite-ci-workflow-on-m](./quick/260415-tc2-add-pydocket-test-suite-ci-workflow-on-m/) |
| 260415-u8k | Run pydocket tests against all supported Python versions (3.10-3.14) | 2026-04-16 | n/a | [260415-u8k-run-the-pydocket-tests-against-all-suppo](./quick/260415-u8k-run-the-pydocket-tests-against-all-suppo/) |
| 260415-us1 | Add Python version matrix to pydocket and unit test CI workflows | 2026-04-16 | 21ab7f5 | [260415-us1-add-python-version-matrix-to-pydocket-an](./quick/260415-us1-add-python-version-matrix-to-pydocket-an/) |
| 260415-vor | Fix three redis-py compat issues: xreadgroup blocking, xpending_range NOGROUP, PubSub cancellation test | 2026-04-16 | 505d106 | [260415-vor-fix-three-redis-py-compat-issues-xreadgr](./quick/260415-vor-fix-three-redis-py-compat-issues-xreadgr/) |
| 260416-axy | Pipeline.execute() raise_on_error=True by default (redis-py compat) | 2026-04-16 | a15cfad | [260416-axy-pipeline-execute-raise-on-error-true-by-](./quick/260416-axy-pipeline-execute-raise-on-error-true-by-/) |
| 260416-cea | Fix three redis-py stream compat gaps: xread blocking, $ stream ID, xinfo_stream method | 2026-04-16 | f3fbb84 | [260416-cea-fix-three-redis-py-stream-compat-gaps-xr](./quick/260416-cea-fix-three-redis-py-stream-compat-gaps-xr/) |
| 260416-gqd | Add CI guard against accidental hard dependency on redis | 2026-04-16 | 2647c54 | [260416-gqd-add-ci-guard-against-accidental-hard-dep](./quick/260416-gqd-add-ci-guard-against-accidental-hard-dep/) |
| 260416-hbn | Add test-passing gates to the release workflow | 2026-04-16 | dd34627 | [260416-hbn-add-test-passing-gates-to-the-release-wo](./quick/260416-hbn-add-test-passing-gates-to-the-release-wo/) |
| 260416-k68 | Add tag↔Cargo.toml version guard to .github/workflows/release.yml | 2026-04-16 | 3512c2d | [260416-k68-add-tag-cargo-toml-version-guard-to-gith](./quick/260416-k68-add-tag-cargo-toml-version-guard-to-gith/) |

## Session Continuity

Last session: 2026-04-18T03:19:00Z
Stopped at: Phase 13 Plan 03 Task 1 complete (commit 665ca40); paused at Task 2 checkpoint — developer must fork conda-forge/staged-recipes and open PR before execution resumes
Resume file: .planning/phases/13-publish-burner-redis-to-conda-forge/13-03-PLAN.md
Resume point: Task 2 (checkpoint:human-verify, blocking) — verify staged_recipes_pr_url recorded in .planning/notes/phase-13-feedstock-submission.md frontmatter, then continue to Task 3 (CI iteration) + Task 4 (post-merge verify + SUMMARY)
