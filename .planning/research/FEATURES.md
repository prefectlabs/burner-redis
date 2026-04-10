# Feature Research

**Domain:** Embedded Redis-compatible database for Prefect server
**Researched:** 2026-04-10
**Confidence:** HIGH (based on PROJECT.md requirements validated against Prefect's known Redis usage patterns)

## Feature Landscape

### Table Stakes (Users Expect These)

These features are non-negotiable. Without them, burner-redis cannot serve as a drop-in replacement for `redis.asyncio.Redis` in Prefect's self-hosted server. Missing any of these means Prefect simply will not function.

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| **String commands (SET/GET/DELETE/EXISTS)** | Foundation of all Redis usage; SET with NX/XX/EX/PX flags is used for locking and state | LOW | SET flags (NX, XX, EX, PX) are critical -- lock acquisition uses SET with NX+EX atomically. Must return correct types (OK, nil, bytes). |
| **Hash commands (HSET/HGET/HDEL/HVALS)** | Lease metadata, dead letter queue data stored in hashes | LOW | Straightforward key-field-value storage. HSET must support multiple field-value pairs in one call. |
| **Set commands (SADD/SMEMBERS/SISMEMBER/SREM)** | Dead letter queue tracking uses sets for membership | LOW | Basic set operations. No complex set math (SUNION, SDIFF) needed. |
| **Sorted set commands (ZADD/ZREM/ZRANGE/ZRANGEBYSCORE/ZRANGESTORE/ZREMRANGEBYSCORE)** | Concurrency lease expiration tracking, event ordering | MEDIUM | Score-based ordering is central to lease management. ZRANGEBYSCORE with score ranges for expiration windows. ZRANGESTORE is newer (Redis 6.2.0+) -- copies ranges between keys. |
| **Stream commands (XADD/XREAD/XREADGROUP/XLEN/XACK/XAUTOCLAIM/XTRIM/XINFO GROUPS/XINFO CONSUMERS/XGROUP CREATE/XGROUP DESTROY)** | Prefect's entire messaging subsystem runs on Redis Streams with consumer groups | HIGH | This is the most complex feature area. Must implement stream IDs (timestamp-sequence), consumer groups with pending entry lists (PEL), message acknowledgment, auto-claiming of idle messages, and stream metadata inspection. |
| **Lua scripting (EVAL/EVALSHA)** | Prefect uses Lua scripts for atomic multi-key operations: lease create/revoke/renew, causal event ordering with follower tracking | HIGH | Requires embedding a Lua 5.1 interpreter (via mlua crate in Rust). Must expose redis.call() and redis.pcall() to Lua. KEYS[] and ARGV[] arrays must be populated correctly. EVALSHA requires a script cache with SHA1 lookup. |
| **Script management (SCRIPT LOAD/SCRIPT EXISTS)** | redis-py's register_script() uses SCRIPT LOAD + EVALSHA pattern to avoid resending script text | MEDIUM | Must maintain an in-memory SHA1-to-script cache. SCRIPT EXISTS returns array of 0/1 for each SHA1 queried. redis-py relies on this for pipeline optimization. |
| **Pipeline support** | Prefect batches related operations for atomicity and performance | MEDIUM | Must buffer commands and execute them sequentially, returning results as a list. Pipeline in embedded context is simpler (no network batching needed) but must preserve ordering and atomicity semantics. |
| **Key expiration (TTL-based)** | Lock timeouts, lease expiration, stream entry cleanup | MEDIUM | Needs both passive (check-on-access) and active (periodic sweep) expiration. Must support both seconds (EX) and milliseconds (PX) precision. TTL/PTTL commands for querying remaining time. |
| **Drop-in redis.asyncio.Redis API** | Prefect code must work without changes -- the whole value proposition | HIGH | Must implement the same method signatures, return types, and error behaviors as redis-py's async client. Includes pipeline(), register_script(), lock(), and all command methods. |
| **Async-first Python API** | Prefect is async-first; all Redis calls use await | MEDIUM | PyO3 async integration via pyo3-asyncio or tokio-based approach. Every command method must be an async def returning an awaitable. |

### Differentiators (Competitive Advantage)

These features are what make burner-redis valuable beyond just "it works." They justify using this over telling users to install Redis.

| Feature | Value Proposition | Complexity | Notes |
|---------|-------------------|------------|-------|
| **Zero-configuration startup** | `import burner_redis; db = burner_redis.Redis()` -- no server, no port, no connection string. Prefect self-hosted "just works." | LOW | This is the core value prop. In-process means no connection establishment, no network errors, no port conflicts. |
| **Flush to disk / reload from disk** | State survives process restarts. Manual save API + automatic persist on shutdown gives users data safety without Redis's complexity. | MEDIUM | Custom binary format (not RDB/AOF). Serialize all data structures atomically. Restore on startup. Design for correctness over Redis compatibility. |
| **Pre-built wheels for all platforms** | `pip install burner-redis` works on Linux, macOS, Windows without a Rust toolchain | MEDIUM | maturin + GitHub Actions for manylinux (x86_64, aarch64), macOS (x86_64, arm64), Windows. This is a distribution concern but directly impacts adoption. |
| **Embedded Lua with full redis.call()** | Prefect's complex Lua scripts (lease management, causal ordering) work unchanged. No need to rewrite Lua into native Rust equivalents. | HIGH | Most embedded Redis alternatives (Vedis, mini-redis) do NOT support Lua scripting. This is a genuine differentiator that makes the drop-in claim real. |
| **Memory efficiency for single-process use** | No serialization/deserialization overhead, no network buffers, no connection pooling. Data lives in the same process memory. | LOW | Inherent advantage of embedded architecture. Worth measuring and documenting. |
| **Graceful degradation on missing commands** | Clear error messages for unimplemented commands rather than silent failures or crashes | LOW | Return Redis-compatible error responses: "ERR unknown command 'SUBSCRIBE'". Helps users understand the boundary. |

### Anti-Features (Commonly Requested, Often Problematic)

These are features that seem reasonable but should be explicitly excluded. They would increase complexity without serving Prefect's needs, or would fundamentally conflict with the embedded architecture.

| Feature | Why Requested | Why Problematic | Alternative |
|---------|---------------|-----------------|-------------|
| **Network server / Redis wire protocol (RESP)** | "Make it a lightweight Redis server replacement" | Defeats the entire purpose. Adds TCP listener, connection management, RESP parsing, security surface. This is not a Redis server -- it is an embedded database. | Direct in-process API. If users need a network server, they should use Redis/Valkey/Dragonfly. |
| **Pub/Sub (SUBSCRIBE/PUBLISH)** | "Redis has pub/sub, so should this" | Prefect uses Streams, not pub/sub. Pub/sub requires subscriber management, pattern matching, message fan-out -- significant complexity for zero Prefect value. | Streams with consumer groups cover Prefect's messaging needs. |
| **Cluster/Sentinel support** | "What about high availability?" | Single-process embedded database. Clustering is architecturally impossible in-process. | If users need HA, they should use real Redis with replication. burner-redis is for the "single server, zero infrastructure" use case. |
| **Replication** | "Sync data between instances" | Same as cluster -- fundamentally incompatible with embedded, in-process design. | Out of scope. Use real Redis for multi-node deployments. |
| **ACL / Authentication** | "Security best practice" | Runs in the same process as the application. There is no network boundary to protect. Auth adds API complexity for zero security benefit. | Process-level security (OS permissions, container isolation) is the right layer. |
| **Full Redis command coverage** | "Be a complete Redis" | 200+ commands, most unused by Prefect. Each command is implementation + testing cost. Vedis has 70+ commands and still lacks sorted sets and streams. | Implement only what Prefect uses. Architecture should allow adding commands later, but don't build speculatively. |
| **RDB/AOF persistence format** | "Compatibility with Redis backups" | Adds massive complexity (RDB is a complex binary format with LZF compression, type-specific encoding). No one will restore a burner-redis dump into real Redis. | Custom persistence format optimized for burner-redis's data structures. Simpler, faster, and purpose-built. |
| **Transactions (MULTI/EXEC/WATCH)** | "Redis supports transactions" | Prefect uses Lua scripts for atomicity, not MULTI/EXEC. WATCH requires optimistic locking with retry logic. Pipelines + Lua scripts cover all atomicity needs. | Lua EVAL provides stronger atomicity guarantees (single-threaded execution) than MULTI/EXEC. |
| **Blocking commands (BLPOP/BRPOP)** | "Useful for job queues" | Prefect doesn't use list-based blocking. Streams with XREADGROUP BLOCK handle this pattern. Blocking list commands require thread coordination complexity. | XREADGROUP with BLOCK parameter covers the blocking-read pattern Prefect actually uses. |
| **Key-space notifications** | "React to key changes" | Requires event system, pattern matching on key names, pub/sub infrastructure. Prefect doesn't use this. | Not needed. Prefect's event system uses explicit stream messages. |

## Feature Dependencies

```
[String Commands (SET/GET/DELETE/EXISTS)]
    |
    +--required-by--> [Key Expiration (TTL)]
    |                     |
    |                     +--required-by--> [Distributed Locking (Lock/AsyncLock)]
    |
    +--required-by--> [Lua Scripting (EVAL/EVALSHA)]
                          |
                          +--required-by--> [Script Management (SCRIPT LOAD/EXISTS)]

[Hash Commands] --required-by--> [Concurrency Lease Storage]
[Set Commands]  --required-by--> [Dead Letter Queue]
[Sorted Set Commands] --required-by--> [Concurrency Lease Expiration Tracking]
                          |
                          +--required-by--> [Causal Event Ordering]

[Stream Commands (XADD/XREAD/XLEN/XTRIM)]
    |
    +--required-by--> [Consumer Groups (XREADGROUP/XACK/XGROUP CREATE)]
                          |
                          +--required-by--> [Message Recovery (XAUTOCLAIM)]
                          |
                          +--required-by--> [Stream Introspection (XINFO GROUPS/CONSUMERS)]

[Pipeline Support]
    |
    +--enhances--> [All Commands] (batching for atomicity)
    +--requires--> [Script Management] (register_script sends SCRIPT LOAD in pipeline)

[Lua Scripting]
    |
    +--requires--> [String Commands] (redis.call('SET', ...))
    +--requires--> [Hash Commands] (redis.call('HSET', ...))
    +--requires--> [Sorted Set Commands] (redis.call('ZADD', ...))
    +--requires--> [Key Expiration] (redis.call('EXPIRE', ...))

[Drop-in API Compatibility]
    |
    +--requires--> [All Table Stakes Features]
    +--requires--> [Async Python Bindings]

[Flush to Disk]
    |
    +--requires--> [All Data Structures] (must serialize strings, hashes, sets, sorted sets, streams)
    +--requires--> [Key Expiration Metadata] (must persist TTL information)
```

### Dependency Notes

- **Lua Scripting requires all basic data structures:** Prefect's Lua scripts call redis.call() on strings, hashes, sorted sets, and keys. The Lua engine must dispatch to the same command implementations used directly.
- **Pipeline requires Script Management:** redis-py's register_script() uses SCRIPT LOAD inside pipelines to ensure scripts are cached before EVALSHA calls within the same pipeline.
- **Consumer Groups require base Stream commands:** XREADGROUP is built on top of XADD/XREAD; consumer groups track offsets into the underlying stream.
- **XAUTOCLAIM requires Consumer Groups:** Auto-claiming idle messages requires the Pending Entry List (PEL) that consumer groups maintain.
- **Flush to Disk requires all data structures:** Persistence must handle every data type, including streams with their internal ID sequences and consumer group state.
- **Key Expiration requires String Commands first:** Expiration metadata is attached to keys; keys must exist before TTL can be set. The active expiration sweep needs access to the key store.

## MVP Definition

### Launch With (v1)

The absolute minimum to run a Prefect self-hosted server with burner-redis instead of Redis.

- [ ] **String commands (SET with NX/XX/EX/PX, GET, DELETE, EXISTS)** -- Foundation for everything; locking depends on SET NX EX
- [ ] **Hash commands (HSET, HGET, HDEL, HVALS)** -- Lease metadata storage
- [ ] **Set commands (SADD, SMEMBERS, SISMEMBER, SREM)** -- DLQ tracking
- [ ] **Sorted set commands (ZADD, ZREM, ZRANGE, ZRANGEBYSCORE, ZRANGESTORE, ZREMRANGEBYSCORE)** -- Lease expiration, event ordering
- [ ] **Stream commands (XADD, XREAD, XREADGROUP, XLEN, XACK, XAUTOCLAIM, XTRIM, XINFO GROUPS, XINFO CONSUMERS, XGROUP CREATE, XGROUP DESTROY)** -- Entire messaging subsystem
- [ ] **Lua scripting (EVAL, EVALSHA) with redis.call()/redis.pcall()** -- Atomic lease and event operations
- [ ] **Script management (SCRIPT LOAD, SCRIPT EXISTS)** -- redis-py register_script() compatibility
- [ ] **Pipeline support** -- Batched atomic operations
- [ ] **Key expiration** -- TTL-based with passive + active expiry
- [ ] **Drop-in redis.asyncio.Redis API** -- Method signatures, return types, error behavior matching redis-py
- [ ] **Async Python bindings** -- PyO3 with async/await support

### Add After Validation (v1.x)

Features that improve the experience but are not needed for initial "does it work?" validation.

- [ ] **Flush to disk / reload from disk** -- Trigger: users want state to survive server restarts. Start with manual save() API, then add auto-persist on shutdown.
- [ ] **Distributed locking (Lock/AsyncLock compatible)** -- Trigger: Prefect's lock usage in docket scheduler. May work implicitly through SET NX EX + Lua scripts, but explicit Lock class API may be needed for direct redis-py Lock usage.
- [ ] **Pre-built wheels for all platforms** -- Trigger: users on non-Linux platforms try to install. Start with Linux x86_64, expand to macOS arm64, Windows.
- [ ] **EXPIRE/TTL/PTTL/PERSIST commands** -- Trigger: code paths that explicitly query or modify TTL outside of SET EX. These are simple once key expiration exists.
- [ ] **TYPE command** -- Trigger: diagnostic code paths that check key types. Simple to implement.
- [ ] **KEYS/SCAN pattern matching** -- Trigger: admin or debugging tools that enumerate keys. SCAN is preferred over KEYS for large datasets but in embedded context KEYS is safe.
- [ ] **FLUSHDB/FLUSHALL** -- Trigger: test cleanup. Trivial to implement.

### Future Consideration (v2+)

Features to consider only after burner-redis is proven in production use.

- [ ] **Additional string commands (MSET, MGET, INCR, DECR, APPEND)** -- Defer: Prefect doesn't currently use these, but they're common enough that external integrations might want them.
- [ ] **List commands (LPUSH, RPUSH, LPOP, RPOP, LRANGE, LLEN)** -- Defer: Prefect uses streams, not lists. Only add if a concrete use case emerges.
- [ ] **HyperLogLog, Bitmaps, Geospatial** -- Defer: Specialized data structures with no current Prefect use case.
- [ ] **Background persistence (periodic auto-save)** -- Defer: Start with explicit save. Auto-save adds timer complexity and potential performance impact during writes.
- [ ] **Memory usage introspection (INFO, DBSIZE, MEMORY USAGE)** -- Defer: Useful for monitoring but not functional requirements.

## Feature Prioritization Matrix

| Feature | User Value | Implementation Cost | Priority |
|---------|------------|---------------------|----------|
| String commands (SET/GET/DEL/EXISTS) | HIGH | LOW | P1 |
| Hash commands (HSET/HGET/HDEL/HVALS) | HIGH | LOW | P1 |
| Set commands (SADD/SMEMBERS/SISMEMBER/SREM) | HIGH | LOW | P1 |
| Sorted set commands (ZADD/ZREM/ZRANGE/ZRANGEBYSCORE/ZRANGESTORE/ZREMRANGEBYSCORE) | HIGH | MEDIUM | P1 |
| Stream commands (full suite) | HIGH | HIGH | P1 |
| Lua scripting (EVAL/EVALSHA) | HIGH | HIGH | P1 |
| Script management (SCRIPT LOAD/EXISTS) | HIGH | LOW | P1 |
| Key expiration (TTL-based) | HIGH | MEDIUM | P1 |
| Pipeline support | HIGH | MEDIUM | P1 |
| Drop-in async API | HIGH | HIGH | P1 |
| Async Python bindings (PyO3) | HIGH | MEDIUM | P1 |
| Flush to disk / reload | MEDIUM | MEDIUM | P2 |
| Distributed locking (Lock class) | MEDIUM | LOW | P2 |
| Pre-built wheels (all platforms) | MEDIUM | MEDIUM | P2 |
| EXPIRE/TTL/PTTL/PERSIST | LOW | LOW | P2 |
| KEYS/SCAN | LOW | LOW | P3 |
| FLUSHDB/FLUSHALL | LOW | LOW | P3 |
| Additional string commands | LOW | LOW | P3 |
| List commands | LOW | MEDIUM | P3 |

**Priority key:**
- P1: Must have for launch -- Prefect cannot function without these
- P2: Should have, add shortly after initial validation
- P3: Nice to have, future consideration

## Competitor Feature Analysis

| Feature | Vedis (C, embedded) | mini-redis (Rust, learning) | rsedis (Rust, server) | Dragonfly (C++, server) | burner-redis (our approach) |
|---------|---------------------|----------------------------|----------------------|------------------------|---------------------------|
| In-process / embedded | Yes | No (TCP server) | No (TCP server) | No (TCP server) | Yes |
| String commands | Yes (full) | Yes (basic) | Yes | Yes | Yes (SET with all flags) |
| Hash commands | Yes | No | Partial | Yes | Yes |
| Set commands | Yes | No | Partial | Yes | Yes |
| Sorted set commands | No | No | Partial | Yes | Yes |
| Stream commands | No | No | No | Yes | Yes |
| Consumer groups | No | No | No | Yes | Yes |
| Lua scripting | No | No | No | Yes | Yes (via mlua crate) |
| Pipeline support | N/A (in-process) | No | Partial | Yes | Yes (API compat layer) |
| Key expiration | Yes | Yes (basic) | Partial | Yes | Yes |
| Persistence | Yes (built-in) | No | No | Yes (snapshots) | Yes (custom format) |
| Python bindings | Yes (vedis-python) | No | No | N/A (client lib) | Yes (PyO3, native) |
| Async Python API | No | No | No | N/A | Yes |
| Active maintenance | No (abandoned ~2014) | Minimal (learning project) | No (abandoned) | Yes (active) | Yes (new project) |

**Key insight:** No existing embedded Redis-compatible database supports both Streams and Lua scripting. Vedis is the closest embedded option but lacks sorted sets, streams, and Lua -- three features Prefect critically depends on. This means burner-redis is genuinely novel in its feature combination.

## Complexity Assessment by Subsystem

### Low Complexity
- **String commands:** HashMap with metadata. SET flags are conditional logic.
- **Hash commands:** Nested HashMap. Straightforward CRUD.
- **Set commands:** HashSet. Standard set operations.
- **Script management:** SHA1 hash map for script cache. Trivial once Lua engine exists.

### Medium Complexity
- **Sorted set commands:** Need a data structure that supports both score-based ordering (BTreeMap or skip list) and member lookup (HashMap). Dual-index pattern.
- **Key expiration:** Passive check on every access + active periodic sweep. Need a sorted structure for "next expiry" tracking. Timer wheel or sorted expiry queue.
- **Pipeline support:** Command buffer + sequential execution + result collection. Simpler than network Redis since no RESP parsing needed.
- **Persistence:** Serialize all data structures to a binary format. Challenge is atomicity (snapshot while in use) and completeness (every data type including stream state).
- **Async Python bindings:** PyO3 + pyo3-asyncio. The challenge is bridging Rust's ownership model with Python's GC while maintaining async compatibility.

### High Complexity
- **Stream commands:** Streams are the most complex Redis data structure. Require: auto-generated IDs (timestamp-sequence), consumer groups with independent read cursors, pending entry lists (PEL) per consumer, message acknowledgment tracking, XAUTOCLAIM scanning PEL for idle entries, XINFO introspection of group/consumer state, XTRIM with MAXLEN/MINID strategies. This is effectively implementing a message broker.
- **Lua scripting:** Embedding Lua 5.1 via mlua crate, exposing redis.call()/redis.pcall() that dispatch back to the command engine, managing KEYS[]/ARGV[] arrays, SHA1 caching, error propagation between Lua and Rust. The Lua-to-Rust bridge must handle type conversion (Lua tables to Redis arrays, Lua strings to Redis bulk strings).
- **Drop-in API compatibility:** Not just implementing commands, but matching redis-py's exact behavior: return types (bytes vs str), error classes, pipeline chaining, register_script() returning callable Script objects, Lock class with acquire/release/extend semantics. Subtle behavioral differences will cause Prefect to break.

## Sources

- [Vedis - Embedded Redis Implementation](https://github.com/symisc/vedis) - Embedded C library, 70+ commands, no streams/sorted sets/Lua
- [mini-redis - Tokio Learning Project](https://github.com/tokio-rs/mini-redis/) - Rust, learning only, very limited command set
- [rsedis - Redis in Rust](https://github.com/seppo0010/rsedis) - Rust server reimplementation, abandoned
- [Dragonfly - Redis Compatible Server](https://www.dragonflydb.io/) - Full Redis compatibility but server-only, not embeddable
- [Redis Streams Documentation](https://redis.io/docs/latest/develop/data-types/streams/) - Authoritative reference for stream semantics
- [Redis Lua Scripting](https://redis.io/docs/latest/develop/programmability/eval-intro/) - EVAL/EVALSHA specification
- [Redis SET Command](https://redis.io/docs/latest/commands/set/) - Full flag reference (NX, XX, EX, PX, GET, KEEPTTL)
- [Redis XAUTOCLAIM](https://redis.io/docs/latest/commands/xautoclaim/) - PEL scanning and message reclaiming
- [Redis Key Expiration Internals](https://www.pankajtanwar.in/blog/how-redis-expires-keys-a-deep-dive-into-how-ttl-works-internally-in-redis) - Passive + active expiration strategy
- [redis-py Lua Scripting](https://redis.readthedocs.io/en/stable/lua_scripting.html) - register_script() / Script object API
- [redis-py Async Examples](https://redis.readthedocs.io/en/stable/examples/asyncio_examples.html) - Pipeline and async API surface
- [Prefect Redis Integration Docs](https://docs.prefect.io/integrations/prefect-redis) - Configuration and subsystem overview
- [Prefect ConcurrencyLeaseStorage PR](https://github.com/PrefectHQ/prefect/pull/18646) - Redis-based lease storage with sorted sets + Lua
- [Prefect Stream Trimming Fix PR](https://github.com/PrefectHQ/prefect/pull/18642) - XINFO/XGROUP consumer management
- [Prefect EVALSHA Issue](https://github.com/prefecthq/prefect/issues/19798) - Confirms Lua scripting is critical path
- [mlua - Lua Bindings for Rust](https://github.com/mlua-rs/rlua) - Preferred Rust crate for embedding Lua 5.1
- [Distributed Locks with Redis](https://redis.io/docs/latest/develop/clients/patterns/distributed-locks/) - Lock pattern specification
- [redis-py Distributed Locks](https://deepwiki.com/redis/redis-py/4.3-async-patterns) - Lock/AsyncLock implementation details

---
*Feature research for: Embedded Redis-compatible database for Prefect server*
*Researched: 2026-04-10*
