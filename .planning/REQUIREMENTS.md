# Requirements: Burner Redis

**Defined:** 2026-04-10
**Core Value:** A self-hosted Prefect server can start, run flows, and manage state using this library instead of an external Redis server

## v1 Requirements

Requirements for initial release. Each maps to roadmap phases.

### Foundation

- [ ] **FOUND-01**: Rust core library with PyO3 bindings compiles and is importable from Python
- [ ] **FOUND-02**: Python class implements `redis.asyncio.Redis`-compatible method signatures
- [ ] **FOUND-03**: All command methods are async-compatible (awaitable from Python)

### String Commands

- [ ] **STR-01**: User can SET a key with a string value
- [ ] **STR-02**: SET supports NX (only if not exists) and XX (only if exists) flags
- [ ] **STR-03**: SET supports EX (seconds) and PX (milliseconds) expiration flags
- [ ] **STR-04**: User can GET a key's value (returns bytes or None)
- [ ] **STR-05**: User can DELETE one or more keys
- [ ] **STR-06**: User can check if a key EXISTS

### Hash Commands

- [ ] **HASH-01**: User can HSET one or more field-value pairs on a hash
- [ ] **HASH-02**: User can HGET a single field from a hash
- [ ] **HASH-03**: User can HDEL one or more fields from a hash
- [ ] **HASH-04**: User can HVALS to get all values from a hash

### Set Commands

- [ ] **SET-01**: User can SADD members to a set
- [ ] **SET-02**: User can SMEMBERS to get all members of a set
- [ ] **SET-03**: User can SISMEMBER to check if a value is in a set
- [ ] **SET-04**: User can SREM to remove members from a set

### Sorted Set Commands

- [ ] **ZSET-01**: User can ZADD members with scores to a sorted set
- [ ] **ZSET-02**: User can ZREM members from a sorted set
- [ ] **ZSET-03**: User can ZRANGE to get members by index range
- [ ] **ZSET-04**: User can ZRANGEBYSCORE to get members by score range
- [ ] **ZSET-05**: User can ZRANGESTORE to store a range result into a new key
- [ ] **ZSET-06**: User can ZREMRANGEBYSCORE to remove members by score range

### Stream Commands

- [ ] **STRM-01**: User can XADD entries to a stream with auto-generated IDs
- [ ] **STRM-02**: User can XREAD entries from one or more streams
- [ ] **STRM-03**: User can XLEN to get the number of entries in a stream
- [ ] **STRM-04**: User can XTRIM a stream by maxlen or minid
- [ ] **STRM-05**: User can XGROUP CREATE to create a consumer group
- [ ] **STRM-06**: User can XGROUP DESTROY to remove a consumer group
- [ ] **STRM-07**: User can XREADGROUP to read as a consumer in a group
- [ ] **STRM-08**: User can XACK to acknowledge processed messages
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

- [ ] **EXP-01**: Keys with TTL expire and are no longer accessible after expiration
- [ ] **EXP-02**: Expiration supports both seconds and milliseconds precision
- [ ] **EXP-03**: Expired keys are cleaned up (passive on access + active sweep)

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
| Network server / Redis wire protocol | Embedded in-process only — no TCP, no RESP parsing |
| Pub/Sub (SUBSCRIBE/PUBLISH) | Prefect uses Streams, not pub/sub |
| Cluster/Sentinel support | Single-process embedded — architecturally incompatible |
| Replication | No multi-node support needed |
| ACL / Authentication | Runs in-process — no auth boundary to protect |
| Full Redis command coverage | Only implement what Prefect uses |
| RDB/AOF persistence format | Custom format is simpler and purpose-built |
| MULTI/EXEC/WATCH transactions | Prefect uses Lua scripts for atomicity |
| Blocking list commands (BLPOP/BRPOP) | Prefect uses Streams, not blocking lists |
| Key-space notifications | Prefect uses explicit stream messages |

## Traceability

Which phases cover which requirements. Updated during roadmap creation.

| Requirement | Phase | Status |
|-------------|-------|--------|
| FOUND-01 | TBD | Pending |
| FOUND-02 | TBD | Pending |
| FOUND-03 | TBD | Pending |
| STR-01 | TBD | Pending |
| STR-02 | TBD | Pending |
| STR-03 | TBD | Pending |
| STR-04 | TBD | Pending |
| STR-05 | TBD | Pending |
| STR-06 | TBD | Pending |
| HASH-01 | TBD | Pending |
| HASH-02 | TBD | Pending |
| HASH-03 | TBD | Pending |
| HASH-04 | TBD | Pending |
| SET-01 | TBD | Pending |
| SET-02 | TBD | Pending |
| SET-03 | TBD | Pending |
| SET-04 | TBD | Pending |
| ZSET-01 | TBD | Pending |
| ZSET-02 | TBD | Pending |
| ZSET-03 | TBD | Pending |
| ZSET-04 | TBD | Pending |
| ZSET-05 | TBD | Pending |
| ZSET-06 | TBD | Pending |
| STRM-01 | TBD | Pending |
| STRM-02 | TBD | Pending |
| STRM-03 | TBD | Pending |
| STRM-04 | TBD | Pending |
| STRM-05 | TBD | Pending |
| STRM-06 | TBD | Pending |
| STRM-07 | TBD | Pending |
| STRM-08 | TBD | Pending |
| STRM-09 | TBD | Pending |
| STRM-10 | TBD | Pending |
| STRM-11 | TBD | Pending |
| LUA-01 | TBD | Pending |
| LUA-02 | TBD | Pending |
| LUA-03 | TBD | Pending |
| LUA-04 | TBD | Pending |
| LUA-05 | TBD | Pending |
| PIPE-01 | TBD | Pending |
| PIPE-02 | TBD | Pending |
| PIPE-03 | TBD | Pending |
| EXP-01 | TBD | Pending |
| EXP-02 | TBD | Pending |
| EXP-03 | TBD | Pending |
| PERS-01 | TBD | Pending |
| PERS-02 | TBD | Pending |
| PERS-03 | TBD | Pending |
| PERS-04 | TBD | Pending |
| DIST-01 | TBD | Pending |
| DIST-02 | TBD | Pending |
| LOCK-01 | TBD | Pending |
| LOCK-02 | TBD | Pending |

**Coverage:**
- v1 requirements: 48 total
- Mapped to phases: 0
- Unmapped: 48

---
*Requirements defined: 2026-04-10*
*Last updated: 2026-04-10 after initial definition*
