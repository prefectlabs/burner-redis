# Roadmap: Burner Redis

## Overview

This roadmap takes burner-redis from an empty Rust crate to a published PyPI package that can replace `redis.asyncio.Redis` for Prefect server. The journey begins by proving the end-to-end Python-Rust bridge with string commands, adds collection data types and expiration, tackles the complex stream/consumer-group subsystem, layers on Lua scripting and pipelines for atomic operations, adds persistence and locking, and finishes with cross-platform wheel distribution. Each phase delivers a coherent, testable capability that builds on what came before.

## Phases

**Phase Numbering:**
- Integer phases (1, 2, 3): Planned milestone work
- Decimal phases (2.1, 2.2): Urgent insertions (marked with INSERTED)

Decimal phases appear between their surrounding integers in numeric order.

- [ ] **Phase 1: Foundation and String Commands** - Rust core engine with PyO3 bindings, command dispatch, and full string command support (SET/GET/DELETE/EXISTS)
- [ ] **Phase 2: Hash and Set Commands** - Hash and set data structure support for lease metadata and membership tracking
- [ ] **Phase 3: Sorted Set Commands** - Sorted set support with score-based range queries for expiration tracking and event ordering
- [ ] **Phase 4: Key Expiration** - TTL-based key expiry with passive and active cleanup strategies
- [ ] **Phase 5: Stream Commands and Consumer Groups** - Full Redis Streams support including consumer groups, PEL, and message recovery
- [ ] **Phase 6: Lua Scripting** - Embedded Lua interpreter with EVAL/EVALSHA and redis.call() dispatching to all commands
- [ ] **Phase 7: Pipeline and Locking** - Batched command execution and Lock/AsyncLock semantics for distributed locking
- [ ] **Phase 8: Persistence** - Flush-to-disk and reload-from-disk with crash-safe writes
- [ ] **Phase 9: Distribution** - PyPI package with pre-built wheels for Linux and macOS
- [x] **Phase 10: Pub/Sub** - Redis pub/sub with SUBSCRIBE, UNSUBSCRIBE, PUBLISH, PSUBSCRIBE, PUNSUBSCRIBE, and PUBSUB introspection (completed 2026-04-14)
- [x] **Phase 11: Pydocket Compatibility** - Close redis-py compatibility gaps for pydocket integration (completed 2026-04-14)

## Phase Details

### Phase 1: Foundation and String Commands
**Goal**: A Python package is importable and can execute string commands (SET with all flags, GET, DELETE, EXISTS) through an async API that matches redis.asyncio.Redis signatures
**Depends on**: Nothing (first phase)
**Requirements**: FOUND-01, FOUND-02, FOUND-03, STR-01, STR-02, STR-03, STR-04, STR-05, STR-06
**Success Criteria** (what must be TRUE):
  1. User can `from burner_redis import BurnerRedis`, create an instance, and `await` SET/GET operations that return bytes
  2. User can SET a key with NX, XX, EX, and PX flags and observe correct conditional and expiration behavior
  3. User can DELETE one or more keys and verify they no longer exist via EXISTS
  4. All command methods are async-compatible (awaitable) and match redis.asyncio.Redis method signatures
**Plans:** 2 plans
Plans:
- [x] 01-01-PLAN.md — Project scaffold, Store engine, and PyO3 module entry point
- [x] 01-02-PLAN.md — String command implementations (SET/GET/DELETE/EXISTS) with Python tests

### Phase 2: Hash and Set Commands
**Goal**: Users can store and retrieve hash field-value pairs and set members, enabling Prefect's lease metadata and dead-letter-queue membership patterns
**Depends on**: Phase 1
**Requirements**: HASH-01, HASH-02, HASH-03, HASH-04, SET-01, SET-02, SET-03, SET-04
**Success Criteria** (what must be TRUE):
  1. User can HSET multiple fields on a hash key and retrieve individual fields with HGET
  2. User can HDEL fields from a hash and get all remaining values with HVALS
  3. User can SADD members to a set and verify membership with SISMEMBER
  4. User can SMEMBERS to list all members and SREM to remove specific members
**Plans:** 2 plans
Plans:
- [x] 02-01-PLAN.md — Extend Store engine with Hash/Set value types and WRONGTYPE errors
- [x] 02-02-PLAN.md — Python async methods for hash/set commands with pytest suite

### Phase 3: Sorted Set Commands
**Goal**: Users can manage scored members in sorted sets with range queries and range-based removals, enabling Prefect's lease expiration tracking and causal event ordering
**Depends on**: Phase 1
**Requirements**: ZSET-01, ZSET-02, ZSET-03, ZSET-04, ZSET-05, ZSET-06
**Success Criteria** (what must be TRUE):
  1. User can ZADD members with scores and ZREM members from a sorted set
  2. User can ZRANGE by index and ZRANGEBYSCORE by score range, receiving members in sorted order
  3. User can ZRANGESTORE to copy a range result into a new key
  4. User can ZREMRANGEBYSCORE to remove all members within a score range
**Plans:** 2 plans
Plans:
- [x] 03-01-PLAN.md — Extend Store engine with SortedSet type (dual-index BTreeMap+HashMap) and 6 Rust methods
- [x] 03-02-PLAN.md — Python async methods for sorted set commands with pytest suite

### Phase 4: Key Expiration
**Goal**: Keys with TTL expire automatically and are cleaned up, so that locks, leases, and temporary data do not persist indefinitely
**Depends on**: Phase 1
**Requirements**: EXP-01, EXP-02, EXP-03
**Success Criteria** (what must be TRUE):
  1. A key set with a TTL is no longer accessible after the TTL elapses
  2. Expiration works at both seconds and milliseconds precision
  3. Expired keys are cleaned up via both passive (on-access check) and active (periodic sweep) strategies, preventing memory leaks
**Plans:** 2 plans
Plans:
- [x] 04-01-PLAN.md — Add sweep_expired() to Store engine and spawn background Tokio sweep task
- [x] 04-02-PLAN.md — Python integration tests for passive and active expiration across all data types

### Phase 5: Stream Commands and Consumer Groups
**Goal**: Users can publish messages to streams, consume them via consumer groups with acknowledgment and recovery, enabling Prefect's entire messaging subsystem
**Depends on**: Phase 1, Phase 4
**Requirements**: STRM-01, STRM-02, STRM-03, STRM-04, STRM-05, STRM-06, STRM-07, STRM-08, STRM-09, STRM-10, STRM-11
**Success Criteria** (what must be TRUE):
  1. User can XADD entries to a stream with auto-generated IDs and XREAD them back in order
  2. User can create consumer groups with XGROUP CREATE, read as consumers with XREADGROUP, and acknowledge with XACK
  3. User can XAUTOCLAIM to reclaim idle pending messages from other consumers
  4. User can inspect stream state with XINFO GROUPS and XINFO CONSUMERS
  5. User can XTRIM streams by maxlen or minid to bound memory usage
**Plans:** 3 plans
Plans:
- [x] 05-01-PLAN.md — Stream data structure + XADD/XREAD/XLEN/XTRIM with Python bindings and tests
- [x] 05-02-PLAN.md — Consumer group core: XGROUP CREATE/DESTROY, XREADGROUP, XACK with Python bindings and tests
- [x] 05-03-PLAN.md — Message recovery and introspection: XAUTOCLAIM, XINFO GROUPS/CONSUMERS with Python bindings and tests


### Phase 6: Lua Scripting
**Goal**: Users can execute Lua scripts that atomically operate across multiple keys and data types, enabling Prefect's atomic lease and event operations
**Depends on**: Phase 1, Phase 2, Phase 3, Phase 4, Phase 5
**Requirements**: LUA-01, LUA-02, LUA-03, LUA-04, LUA-05
**Success Criteria** (what must be TRUE):
  1. User can EVAL a Lua script with KEYS and ARGV arrays and receive correct return values
  2. User can SCRIPT LOAD to cache a script, then execute it via EVALSHA with the returned SHA1 hash
  3. Lua scripts can call redis.call() and redis.pcall() to execute any supported Redis command, with correct type conversion between Lua and Redis types
  4. User can SCRIPT EXISTS to check whether scripts are cached
**Plans:** 2 plans
Plans:
- [x] 06-01-PLAN.md — Lua VM setup, redis.call()/redis.pcall() dispatch, script cache, and Store eval/evalsha methods
- [x] 06-02-PLAN.md — Python async bindings (EVAL, EVALSHA, SCRIPT LOAD, SCRIPT EXISTS) with comprehensive pytest suite

### Phase 7: Pipeline and Locking
**Goal**: Users can batch commands for atomic execution and acquire/release distributed locks with timeout and ownership semantics
**Depends on**: Phase 1, Phase 4
**Requirements**: PIPE-01, PIPE-02, PIPE-03, LOCK-01, LOCK-02
**Success Criteria** (what must be TRUE):
  1. User can create a pipeline, queue multiple commands, and execute them as a batch receiving results in command order
  2. Pipeline supports async context manager usage (`async with client.pipeline() as pipe`)
  3. User can acquire a lock with a timeout, verify ownership with a token, and release it
  4. Locks support blocking acquisition and automatic expiration
**Plans:** 2 plans
Plans:
- [x] 07-01-PLAN.md — Pipeline class with command buffering, batch execution, and async context manager
- [x] 07-02-PLAN.md — Lock class with token-based ownership, blocking acquire, and timeout expiration

### Phase 8: Persistence
**Goal**: Database state survives process restarts through manual flush and automatic shutdown persistence with crash-safe writes
**Depends on**: Phase 1, Phase 2, Phase 3, Phase 5
**Requirements**: PERS-01, PERS-02, PERS-03, PERS-04
**Success Criteria** (what must be TRUE):
  1. User can call a flush API to persist all data to disk
  2. On graceful shutdown, data is automatically persisted without explicit user action
  3. On startup, previously persisted data is automatically restored
  4. Persistence uses crash-safe write-then-rename with fsync so partial writes never corrupt state
**Plans:** 2 plans
Plans:
- [x] 08-01-PLAN.md — Serde derives, rmp-serde serialization, and crash-safe persistence module
- [x] 08-02-PLAN.md — Python API (persistence_path constructor, save() method, atexit handler) with integration tests

### Phase 9: Distribution
**Goal**: Users can install burner-redis from PyPI with pre-built wheels for their platform without needing a Rust toolchain
**Depends on**: Phase 1 through Phase 8
**Requirements**: DIST-01, DIST-02
**Success Criteria** (what must be TRUE):
  1. Package is published on PyPI and installable via `pip install burner-redis`
  2. Pre-built wheels are available for manylinux (x86_64, aarch64) and macOS (x86_64, arm64)
**Plans:** 2 plans
Plans:
- [x] 09-01-PLAN.md — CI workflow with maturin-action for cross-platform wheel building
- [x] 09-02-PLAN.md — PyPI publishing and release automation

### Phase 10: Add PUB/SUB support (SUBSCRIBE, UNSUBSCRIBE, PUBLISH, PSUBSCRIBE, PUNSUBSCRIBE, and message dispatch)

**Goal:** Users can subscribe to channels and patterns, publish messages with fire-and-forget semantics, and consume messages via an async PubSub class matching the redis-py interface -- enabling pydocket compatibility and general Redis pub/sub usage
**Requirements**: PUBSUB-01, PUBSUB-02, PUBSUB-03, PUBSUB-04, PUBSUB-05, PUBSUB-06, PUBSUB-07, PUBSUB-08, PUBSUB-09, PUBSUB-10, PUBSUB-11, PUBSUB-12
**Depends on:** Phase 9
**Success Criteria** (what must be TRUE):
  1. User can SUBSCRIBE to channels and receive published messages via PubSub.get_message() or PubSub.listen()
  2. User can PSUBSCRIBE to glob patterns and receive matching messages as pmessage type
  3. User can PUBLISH messages to channels, receiving subscriber count as return value
  4. PubSub class supports handler callbacks, ignore_subscribe_messages, and run_in_thread()
  5. PUBLISH works inside Lua scripts via redis.call() and inside Pipelines
  6. PUBSUB CHANNELS/NUMSUB/NUMPAT introspection commands return correct data
**Plans:** 2/2 plans complete

Plans:
- [x] 10-01-PLAN.md — Rust pub/sub engine: PubSubRegistry in Store, broadcast fan-out, glob matching, PyO3 bindings
- [x] 10-02-PLAN.md — Python PubSub class, Pipeline/Lua PUBLISH integration, and comprehensive test suite

### Phase 11: Close redis-py compatibility gaps for pydocket integration

**Goal:** Pydocket's full test suite passes against BurnerRedis with zero xfails/skips, and every gap fixed has regression test coverage in our own suite
**Requirements**: D-01, D-02, D-03, D-04, D-05, D-06, D-07, D-08, D-09, D-10
**Depends on:** Phase 10
**Success Criteria** (what must be TRUE):
  1. XREADGROUP with block parameter waits for new stream entries instead of returning immediately, fixing the ~19% delayed task delivery race
  2. XCLAIM command is fully implemented with all redis-py parameters (idle, force, justid, retrycount, min_idle_time)
  3. XTRIM accepts the approximate parameter without error
  4. All pydocket integration tests pass with zero xfails and zero skips
  5. Regression tests cover every gap fixed in this phase
**Plans:** 2/2 plans complete

Plans:
- [x] 11-01-PLAN.md — XREADGROUP blocking with tokio::sync::Notify, XCLAIM implementation, XTRIM approximate parameter
- [x] 11-02-PLAN.md — Pydocket test suite validation, gap closure, regression tests, zero xfails

### Phase 12: Close remaining redis-py compatibility gaps for drop-in replacement

**Goal:** burner-redis is a true drop-in replacement for redis.asyncio.Redis with value coercion, key enumeration, TTL inspection, exception hierarchy alignment, and missing convenience commands -- no wrapper shims needed
**Requirements**: D-01, D-02, D-03, D-04, D-05, D-06, D-07, D-08, D-09, D-10, D-11, D-12, D-13
**Depends on:** Phase 11
**Success Criteria** (what must be TRUE):
  1. set(key, 42) coerces integer to string bytes; set(key, True) raises TypeError -- matching redis-py exactly
  2. keys(pattern) returns all matching keys with full Redis glob syntax including [a-z] ranges
  3. scan_iter(match=pattern) yields keys as an async generator
  4. ttl(name) returns seconds remaining (-1 no TTL, -2 missing key)
  5. xpending(name, groupname) summary form returns dict with pending/min/max/consumers
  6. setex(name, time, value) stores a key with TTL
  7. mget(*keys) returns a list of values with None for missing keys
  8. LockError is subclass of redis.exceptions.LockError when redis is installed
  9. Pipeline has stubs for all new commands
**Plans:** 2/2 plans complete

Plans:
- [x] 12-01-PLAN.md — Rust glob range support, Store methods (keys, ttl, mget, xpending_summary), PyO3 bindings
- [x] 12-02-PLAN.md — Python value coercion, exception hierarchy, scan_iter, setex, pipeline stubs, and comprehensive tests

## Progress

**Execution Order:**
Phases execute in numeric order: 1 -> 2 -> 3 -> 4 -> 5 -> 6 -> 7 -> 8 -> 9 -> 10 -> 11 -> 12

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. Foundation and String Commands | 2/2 | Complete | - |
| 2. Hash and Set Commands | 2/2 | Complete | - |
| 3. Sorted Set Commands | 2/2 | Complete | - |
| 4. Key Expiration | 2/2 | Complete | - |
| 5. Stream Commands and Consumer Groups | 3/3 | Complete | - |
| 6. Lua Scripting | 2/2 | Complete | - |
| 7. Pipeline and Locking | 2/2 | Complete | - |
| 8. Persistence | 0/2 | Planning complete | - |
| 9. Distribution | 0/2 | Not started | - |
| 10. Pub/Sub | 2/2 | Complete    | 2026-04-14 |
| 11. Pydocket Compatibility | 2/2 | Complete    | 2026-04-14 |
| 12. Drop-in Replacement | 2/2 | Complete    | 2026-04-14 |
| 13. Publish burner-redis to conda-forge | 2/3 | Complete    | 2026-04-24 |

### Phase 13: Publish burner-redis to conda-forge

**Goal:** `conda install -c conda-forge burner-redis` works on linux-64, linux-aarch64, osx-64, osx-arm64, and win-64 — unblocking conda users of Prefect who pick up burner-redis transitively through pydocket
**Requirements**: (no REQ-IDs — external-distribution phase)
**Depends on:** Phase 12
**Plans:** 2/3 plans complete

Plans:
- [x] 13-01-PLAN.md — Verify PyPI sdist is feedstock-ready (Cargo.lock present, offline build passes); resolve version pin (0.1.2 or cut 0.1.3) — **PASS, pinned_version=0.1.2, sha256=189698190835809f73fdb5af9ead4962975181c7fc8297045a5d831c0d465add**
- [x] 13-02-PLAN.md — Rust dependency license audit with cargo-bundle-licenses; commit THIRDPARTY.yml — **PASS, cargo-bundle-licenses 4.0.0, all 57 crates permissive (MIT / Apache-2.0 / Unlicense / Unicode-3.0), no remediation required**
- [ ] 13-03-PLAN.md — Draft recipe on fork of conda-forge/staged-recipes, open PR, iterate on CI, verify post-merge feedstock publishes on all 5 platforms + smoke test

### Phase 14: List data type (LPUSH, BRPOP, BLPOP, and full list command set)

**Goal:** Add Redis list data type support to the engine with a 16-command surface (LPUSH, RPUSH, LPOP, RPOP, LRANGE, LLEN, LINDEX, LINSERT, LREM, LSET, LTRIM, LMOVE, RPOPLPUSH, BRPOP, BLPOP, BLMOVE) as a drop-in replacement for redis.asyncio.Redis list operations. Blocking commands integrate with the existing Tokio runtime using the Phase-11 `tokio::sync::Notify` pattern and respect Python asyncio cancellation/timeout semantics. Storage is `VecDeque<Bytes>` behind the existing `parking_lot::RwLock` keyspace.
- **Required commands:** LPUSH, BRPOP, BLPOP
- **Stretch goal (absorbed into this phase):** full list command coverage — RPUSH, LPOP, RPOP, LRANGE, LLEN, LINDEX, LINSERT, LREM, LSET, LTRIM, LMOVE, RPOPLPUSH, BLMOVE
- **Blocking commands** (BRPOP, BLPOP, BLMOVE) must integrate cleanly with the existing async/Tokio runtime and respect Python asyncio semantics (cancellation, timeouts, GIL release while blocked)
- **Storage:** `VecDeque<Bytes>` behind the keyspace `parking_lot::RwLock`
- **Scope-reversal note:** REQUIREMENTS.md currently lists BLPOP/BRPOP under Out of Scope; this phase removes that entry and adds LIST-01..LIST-16 (Plan 01 Task 1)

**Requirements**: LIST-01, LIST-02, LIST-03, LIST-04, LIST-05, LIST-06, LIST-07, LIST-08, LIST-09, LIST-10, LIST-11, LIST-12, LIST-13, LIST-14, LIST-15, LIST-16
**Depends on:** Phase 13
**Plans:** 3 plans

Plans:
- [x] 14-01-PLAN.md — Rust engine: ValueData::List variant, list_notify field, 13 non-blocking Store methods + blpop_poll/brpop_poll/lmove_atomic helpers, src/commands/lists.rs helpers, REQUIREMENTS.md update adding LIST-01..LIST-16 (Wave 1)
- [x] 14-02-PLAN.md — Python surface: 13 non-blocking #[pymethods], 3 blocking #[pymethods] (BRPOP/BLPOP/BLMOVE via future_into_py + tokio::select! + notify re-arm), value-coercion monkey-patches, tests/test_lists.py covering LIST-01..LIST-15 (Wave 2)
- [x] 14-03-PLAN.md — Lua + pipeline integration: had_list_mutation flag + 13 non-blocking + 3 blocking-reject arms in scripting.rs, 13 non-blocking arms in dispatch_pipeline_command, 16 Python pipeline stubs, Python-side blocking-aware Pipeline.execute() branch, LIST-16 tests, REQUIREMENTS.md finalize (Wave 3)
