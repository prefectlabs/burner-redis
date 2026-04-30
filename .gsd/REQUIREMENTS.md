# Requirements: Burner Redis

**Status:** shipped archive from v0.1.6
**Core Value:** A self-hosted Prefect server can start, run flows, and manage state using this library instead of an external Redis server.

This migrated snapshot preserves the v0.1.6 requirement set. All v1 requirements are satisfied/validated; v2 items remain deferred.

## v1 Requirements

### Foundation
- **FOUND-01** — validated: Rust core library with PyO3 bindings compiles and is importable from Python
- **FOUND-02** — validated: Python class implements `redis.asyncio.Redis`-compatible method signatures
- **FOUND-03** — validated: All command methods are async-compatible (awaitable from Python)

### String Commands
- **STR-01** — validated: User can SET a key with a string value
- **STR-02** — validated: SET supports NX and XX flags
- **STR-03** — validated: SET supports EX and PX expiration flags
- **STR-04** — validated: User can GET a key's value (returns bytes or None)
- **STR-05** — validated: User can DELETE one or more keys
- **STR-06** — validated: User can check if a key EXISTS

### Hash Commands
- **HASH-01** — validated: User can HSET one or more field-value pairs on a hash
- **HASH-02** — validated: User can HGET a single field from a hash
- **HASH-03** — validated: User can HDEL one or more fields from a hash
- **HASH-04** — validated: User can HVALS to get all values from a hash

### Set Commands
- **SET-01** — validated: User can SADD members to a set
- **SET-02** — validated: User can SMEMBERS to get all members of a set
- **SET-03** — validated: User can SISMEMBER to check if a value is in a set
- **SET-04** — validated: User can SREM to remove members from a set

### Sorted Set Commands
- **ZSET-01** — validated: User can ZADD members with scores to a sorted set
- **ZSET-02** — validated: User can ZREM members from a sorted set
- **ZSET-03** — validated: User can ZRANGE to get members by index range
- **ZSET-04** — validated: User can ZRANGEBYSCORE to get members by score range
- **ZSET-05** — validated: User can ZRANGESTORE to store a range result into a new key
- **ZSET-06** — validated: User can ZREMRANGEBYSCORE to remove members by score range

### Stream Commands
- **STRM-01** — validated: User can XADD entries to a stream with auto-generated IDs
- **STRM-02** — validated: User can XREAD entries from one or more streams
- **STRM-03** — validated: User can XLEN to get the number of entries in a stream
- **STRM-04** — validated: User can XTRIM a stream by maxlen or minid
- **STRM-05** — validated: User can XGROUP CREATE to create a consumer group
- **STRM-06** — validated: User can XGROUP DESTROY to remove a consumer group
- **STRM-07** — validated: User can XREADGROUP to read as a consumer in a group
- **STRM-08** — validated: User can XACK to acknowledge processed messages
- **STRM-09** — validated: User can XAUTOCLAIM to reclaim idle pending messages
- **STRM-10** — validated: User can XINFO GROUPS to inspect consumer groups on a stream
- **STRM-11** — validated: User can XINFO CONSUMERS to inspect consumers in a group

### List Commands
- **LIST-01** — validated: User can LPUSH one or more values onto the head of a list
- **LIST-02** — validated: User can RPUSH one or more values onto the tail of a list
- **LIST-03** — validated: User can LPOP with optional count
- **LIST-04** — validated: User can RPOP with the same semantics as LPOP
- **LIST-05** — validated: User can LRANGE with negative indices to slice a list
- **LIST-06** — validated: User can LLEN to get the length of a list
- **LIST-07** — validated: User can LINDEX to read an element at an index
- **LIST-08** — validated: User can LINSERT BEFORE or AFTER a pivot
- **LIST-09** — validated: User can LREM with positive, negative, or zero count
- **LIST-10** — validated: User can LSET to replace an element at an index
- **LIST-11** — validated: User can LTRIM to clamp a list to a range
- **LIST-12** — validated: User can LMOVE between two lists atomically
- **LIST-13** — validated: User can RPOPLPUSH (legacy alias for LMOVE RIGHT LEFT)
- **LIST-14** — validated: User can BRPOP/BLPOP with float-seconds timeout, multi-key scan
- **LIST-15** — validated: User can BLMOVE with timeout, atomic src/dst semantics
- **LIST-16** — validated: All list commands work in pipelines; 13 non-blocking work in Lua

### Lua Scripting
- **LUA-01** — validated: User can EVAL a Lua script with KEYS and ARGV arrays
- **LUA-02** — validated: User can EVALSHA to execute a cached script by SHA1 hash
- **LUA-03** — validated: Lua scripts can call redis.call() and redis.pcall() to execute Redis commands
- **LUA-04** — validated: User can SCRIPT LOAD to cache a script and get its SHA1
- **LUA-05** — validated: User can SCRIPT EXISTS to check if scripts are cached

### Pipeline
- **PIPE-01** — validated: User can create a pipeline, queue multiple commands, and execute them as a batch
- **PIPE-02** — validated: Pipeline returns results as a list in command order
- **PIPE-03** — validated: Pipeline supports async context manager usage

### Key Expiration
- **EXP-01** — validated: Keys with TTL expire and are no longer accessible after expiration
- **EXP-02** — validated: Expiration supports both seconds and milliseconds precision
- **EXP-03** — validated: Expired keys are cleaned up (passive on access + active sweep)

### Persistence
- **PERS-01** — validated: User can manually flush all data to disk via API call
- **PERS-02** — validated: Data automatically persists on graceful shutdown
- **PERS-03** — validated: On startup, data is restored from a previous flush if available
- **PERS-04** — validated: Persistence uses crash-safe write (write-then-rename with fsync)

### Distribution
- **DIST-01** — validated: Published as a PyPI package with pre-built wheels
- **DIST-02** — validated: Wheels available for manylinux (x86_64, aarch64), macOS (x86_64, arm64)

### Locking
- **LOCK-01** — validated: User can acquire and release locks with Lock/AsyncLock semantics
- **LOCK-02** — validated: Locks support timeout, blocking, and token-based ownership

## v2 Requirements

Deferred to future release.

- **ECMD-01** — active: TTL/PTTL commands for querying remaining time on keys
- **ECMD-02** — active: KEYS/SCAN for key enumeration
- **ECMD-03** — active: TYPE command to check key type
- **ECMD-04** — active: FLUSHDB/FLUSHALL for clearing the database
- **ECMD-05** — active: INFO command for database statistics
- **EDIST-01** — active: Windows wheel support
- **EDIST-02** — active: Published as a standalone Rust crate on crates.io

## Out of Scope

- Network server / Redis wire protocol
- Pub/Sub (SUBSCRIBE/PUBLISH)
- Cluster/Sentinel support
- Replication
- ACL / Authentication
- Full Redis command coverage
- RDB/AOF persistence format
- MULTI/EXEC/WATCH transactions
- Key-space notifications
