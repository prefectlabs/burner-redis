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
- [ ] **Phase 10: Pub/Sub** - Redis pub/sub with SUBSCRIBE, UNSUBSCRIBE, PUBLISH, PSUBSCRIBE, PUNSUBSCRIBE, and PUBSUB introspection

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
**Plans:** 1/2 plans executed

Plans:
- [x] 10-01-PLAN.md — Rust pub/sub engine: PubSubRegistry in Store, broadcast fan-out, glob matching, PyO3 bindings
- [ ] 10-02-PLAN.md — Python PubSub class, Pipeline/Lua PUBLISH integration, and comprehensive test suite

## Progress

**Execution Order:**
Phases execute in numeric order: 1 -> 2 -> 3 -> 4 -> 5 -> 6 -> 7 -> 8 -> 9 -> 10

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
| 10. Pub/Sub | 1/2 | In Progress|  |
