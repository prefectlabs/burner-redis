# Requirements: Burner Redis

**Defined:** 2026-04-10
**Core Value:** A self-hosted Prefect server can start, run flows, and manage state using this library instead of an external Redis server

## v1 Requirements

Requirements for initial release. Each maps to roadmap phases.

### Foundation

- [x] **FOUND-01**: Rust core library with PyO3 bindings compiles and is importable from Python
- [x] **FOUND-02**: Python class implements `redis.asyncio.Redis`-compatible method signatures
- [x] **FOUND-03**: All command methods are async-compatible (awaitable from Python)

### String Commands

- [x] **STR-01**: User can SET a key with a string value
- [x] **STR-02**: SET supports NX (only if not exists) and XX (only if exists) flags
- [x] **STR-03**: SET supports EX (seconds) and PX (milliseconds) expiration flags
- [x] **STR-04**: User can GET a key's value (returns bytes or None)
- [x] **STR-05**: User can DELETE one or more keys
- [x] **STR-06**: User can check if a key EXISTS

### Hash Commands

- [x] **HASH-01**: User can HSET one or more field-value pairs on a hash
- [x] **HASH-02**: User can HGET a single field from a hash
- [x] **HASH-03**: User can HDEL one or more fields from a hash
- [x] **HASH-04**: User can HVALS to get all values from a hash

### Set Commands

- [x] **SET-01**: User can SADD members to a set
- [x] **SET-02**: User can SMEMBERS to get all members of a set
- [x] **SET-03**: User can SISMEMBER to check if a value is in a set
- [x] **SET-04**: User can SREM to remove members from a set

### Sorted Set Commands

- [x] **ZSET-01**: User can ZADD members with scores to a sorted set
- [x] **ZSET-02**: User can ZREM members from a sorted set
- [x] **ZSET-03**: User can ZRANGE to get members by index range
- [x] **ZSET-04**: User can ZRANGEBYSCORE to get members by score range
- [x] **ZSET-05**: User can ZRANGESTORE to store a range result into a new key
- [x] **ZSET-06**: User can ZREMRANGEBYSCORE to remove members by score range

### Stream Commands

- [x] **STRM-01**: User can XADD entries to a stream with auto-generated IDs
- [x] **STRM-02**: User can XREAD entries from one or more streams
- [x] **STRM-03**: User can XLEN to get the number of entries in a stream
- [x] **STRM-04**: User can XTRIM a stream by maxlen or minid
- [x] **STRM-05**: User can XGROUP CREATE to create a consumer group
- [x] **STRM-06**: User can XGROUP DESTROY to remove a consumer group
- [x] **STRM-07**: User can XREADGROUP to read as a consumer in a group
- [x] **STRM-08**: User can XACK to acknowledge processed messages
- [ ] **STRM-09**: User can XAUTOCLAIM to reclaim idle pending messages
- [ ] **STRM-10**: User can XINFO GROUPS to inspect consumer groups on a stream
- [ ] **STRM-11**: User can XINFO CONSUMERS to inspect consumers in a group

### Lua Scripting

- [ ] **LUA-01**: User can EVAL a Lua script with KEYS and ARGV arrays
- [ ] **LUA-02**: User can EVALSHA to execute a cached script by SHA1 hash
- [ ] **LUA-03**: Lua scripts can call redis.call() and redis.pcall() to execute Redis commands
- [ ] **LUA-04**: User can SCRIPT LOAD to cache a script and get its SHA1
- [ ] **LUA-05**: User can SCRIPT EXISTS to check if scripts are cached

### Pipeline

- [ ] **PIPE-01**: User can create a pipeline, queue multiple commands, and execute them as a batch
- [ ] **PIPE-02**: Pipeline returns results as a list in command order
- [ ] **PIPE-03**: Pipeline supports async context manager usage

### Key Expiration

- [x] **EXP-01**: Keys with TTL expire and are no longer accessible after expiration
- [x] **EXP-02**: Expiration supports both seconds and milliseconds precision
- [x] **EXP-03**: Expired keys are cleaned up (passive on access + active sweep)

### Persistence

- [ ] **PERS-01**: User can manually flush all data to disk via API call
- [ ] **PERS-02**: Data automatically persists on graceful shutdown
- [ ] **PERS-03**: On startup, data is restored from a previous flush if available
- [ ] **PERS-04**: Persistence uses crash-safe write (write-then-rename with fsync)

### Distribution

- [ ] **DIST-01**: Published as a PyPI package with pre-built wheels
- [ ] **DIST-02**: Wheels available for manylinux (x86_64, aarch64), macOS (x86_64, arm64)

### Locking

- [ ] **LOCK-01**: User can acquire and release locks with Lock/AsyncLock semantics
- [ ] **LOCK-02**: Locks support timeout, blocking, and token-based ownership

## v2 Requirements

Deferred to future release. Tracked but not in current roadmap.

### Extended Commands

- **ECMD-01**: TTL/PTTL commands for querying remaining time on keys
- **ECMD-02**: KEYS/SCAN for key enumeration
- **ECMD-03**: TYPE command to check key type
- **ECMD-04**: FLUSHDB/FLUSHALL for clearing the database
- **ECMD-05**: INFO command for database statistics

### Extended Distribution

- **EDIST-01**: Windows wheel support
- **EDIST-02**: Published as a standalone Rust crate on crates.io

## Out of Scope

Explicitly excluded. Documented to prevent scope creep.

| Feature | Reason |
|---------|--------|
| Network server / Redis wire protocol | Embedded in-process only -- no TCP, no RESP parsing |
| Pub/Sub (SUBSCRIBE/PUBLISH) | Prefect uses Streams, not pub/sub |
| Cluster/Sentinel support | Single-process embedded -- architecturally incompatible |
| Replication | No multi-node support needed |
| ACL / Authentication | Runs in-process -- no auth boundary to protect |
| Full Redis command coverage | Only implement what Prefect uses |
| RDB/AOF persistence format | Custom format is simpler and purpose-built |
| MULTI/EXEC/WATCH transactions | Prefect uses Lua scripts for atomicity |
| Blocking list commands (BLPOP/BRPOP) | Prefect uses Streams, not blocking lists |
| Key-space notifications | Prefect uses explicit stream messages |

## Traceability

Which phases cover which requirements. Updated during roadmap creation.

| Requirement | Phase | Status |
|-------------|-------|--------|
| FOUND-01 | Phase 1 | Complete |
| FOUND-02 | Phase 1 | Complete |
| FOUND-03 | Phase 1 | Complete |
| STR-01 | Phase 1 | Complete |
| STR-02 | Phase 1 | Complete |
| STR-03 | Phase 1 | Complete |
| STR-04 | Phase 1 | Complete |
| STR-05 | Phase 1 | Complete |
| STR-06 | Phase 1 | Complete |
| HASH-01 | Phase 2 | Complete |
| HASH-02 | Phase 2 | Complete |
| HASH-03 | Phase 2 | Complete |
| HASH-04 | Phase 2 | Complete |
| SET-01 | Phase 2 | Complete |
| SET-02 | Phase 2 | Complete |
| SET-03 | Phase 2 | Complete |
| SET-04 | Phase 2 | Complete |
| ZSET-01 | Phase 3 | Complete |
| ZSET-02 | Phase 3 | Complete |
| ZSET-03 | Phase 3 | Complete |
| ZSET-04 | Phase 3 | Complete |
| ZSET-05 | Phase 3 | Complete |
| ZSET-06 | Phase 3 | Complete |
| STRM-01 | Phase 5 | Complete |
| STRM-02 | Phase 5 | Complete |
| STRM-03 | Phase 5 | Complete |
| STRM-04 | Phase 5 | Complete |
| STRM-05 | Phase 5 | Complete |
| STRM-06 | Phase 5 | Complete |
| STRM-07 | Phase 5 | Complete |
| STRM-08 | Phase 5 | Complete |
| STRM-09 | Phase 5 | Pending |
| STRM-10 | Phase 5 | Pending |
| STRM-11 | Phase 5 | Pending |
| LUA-01 | Phase 6 | Pending |
| LUA-02 | Phase 6 | Pending |
| LUA-03 | Phase 6 | Pending |
| LUA-04 | Phase 6 | Pending |
| LUA-05 | Phase 6 | Pending |
| PIPE-01 | Phase 7 | Pending |
| PIPE-02 | Phase 7 | Pending |
| PIPE-03 | Phase 7 | Pending |
| EXP-01 | Phase 4 | Complete |
| EXP-02 | Phase 4 | Complete |
| EXP-03 | Phase 4 | Complete |
| PERS-01 | Phase 8 | Pending |
| PERS-02 | Phase 8 | Pending |
| PERS-03 | Phase 8 | Pending |
| PERS-04 | Phase 8 | Pending |
| DIST-01 | Phase 9 | Pending |
| DIST-02 | Phase 9 | Pending |
| LOCK-01 | Phase 7 | Pending |
| LOCK-02 | Phase 7 | Pending |

**Coverage:**
- v1 requirements: 53 total
- Mapped to phases: 53
- Unmapped: 0

---
*Requirements defined: 2026-04-10*
*Last updated: 2026-04-10 after roadmap creation*
