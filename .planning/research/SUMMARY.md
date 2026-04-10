# Project Research Summary

**Project:** Burner Redis
**Domain:** Embedded Redis-compatible in-process database (Rust core + Python bindings)
**Researched:** 2026-04-10
**Confidence:** HIGH

## Executive Summary

Burner Redis is an embedded, in-process Redis-compatible database written in Rust with Python bindings via PyO3. Its purpose is to serve as a zero-configuration drop-in replacement for `redis.asyncio.Redis` so that a self-hosted Prefect server can operate without a separate Redis deployment. No existing embedded Redis alternative supports the combination of sorted sets, streams with consumer groups, and Lua scripting that Prefect requires -- making this project genuinely novel. The recommended approach is a Rust core engine using standard library data structures (HashMap, BTreeMap, HashSet) behind a `parking_lot::RwLock`, exposed to Python through a thin PyO3 binding layer with a pure-Python wrapper class that matches `redis.asyncio.Redis` method signatures.

The core technical challenge is fidelity, not performance. Prefect uses Lua scripts for atomic multi-key operations (lease management, causal event ordering), Redis Streams with consumer groups for its entire messaging subsystem, and sorted sets for expiration tracking. Each of these has subtle semantics that must be reproduced exactly. The Lua type conversion rules between Lua and Redis types are particularly treacherous -- Redis converts floats to integers, truncates sparse arrays, and handles boolean/nil differently than most developers expect. Consumer groups have a complex internal state machine (Pending Entry Lists, delivery counts, idle times) where partial implementation causes silent message loss.

The primary risks are: (1) GIL deadlocks when Rust concurrency primitives interact with Python's GIL -- this must be designed out from the start by releasing the GIL before any Rust-side work; (2) Lua scripting type conversion fidelity -- must be validated against real Redis output, not just documentation; (3) redis-py API surface mismatch -- Pipeline methods must NOT be coroutines (only `execute()` is), Lock requires token-based ownership, and return types must be byte-exact. The mitigation strategy is to build compatibility test suites early that run identical operations against both real Redis and burner-redis and compare results.

## Key Findings

### Recommended Stack

The stack is well-established Rust ecosystem tooling with high confidence across the board. Rust 1.85+ (Edition 2024) provides the core engine, with PyO3 0.28.x for Python bindings and maturin 1.13.x for wheel building and distribution. All key dependencies are actively maintained and widely used.

**Core technologies:**
- **Rust 1.85+ (Edition 2024):** Core engine language -- memory safety without GC, zero-cost abstractions, excellent Python interop via PyO3
- **PyO3 0.28.x + maturin 1.13.x:** Rust-to-Python bindings and wheel building -- the standard toolchain for Rust-Python projects. Use `abi3-py39` for single-wheel-per-platform builds
- **Tokio 1.51.x (LTS):** Async runtime for background tasks (expiration sweeps, persistence timers). Use current-thread runtime, not multi-threaded
- **mlua 0.11.x (Lua 5.4):** Embedded Lua interpreter for EVAL/EVALSHA. Only actively maintained Rust-Lua binding. Lua 5.4 is backward-compatible with Prefect's scripts; can switch to 5.1 feature if strict compatibility is needed
- **parking_lot 0.12.x:** RwLock for the main keyspace -- 1.5-5x faster than std, and simpler than DashMap for atomic multi-key operations (Lua scripts, pipelines)
- **rmp-serde 1.3.x (MessagePack):** Persistence format -- self-describing (handles schema evolution), cross-language debuggable. Bincode is no longer viable (RUSTSEC-2025-0141, unmaintained)
- **bytes 1.11.x:** Zero-copy reference-counted byte buffers for key/value storage
- **thiserror 2.0.x:** Error type derivation for clean Rust error hierarchy

**Critical version note:** Bincode is unmaintained and must not be used (RUSTSEC-2025-0141). Use rmp-serde for persistence.

### Expected Features

Prefect's Redis usage is narrow but deep -- it uses a specific set of commands heavily, including complex features (Streams, Lua, sorted sets) that most Redis alternatives skip entirely.

**Must have (table stakes -- Prefect will not function without these):**
- String commands (SET with NX/XX/EX/PX, GET, DELETE, EXISTS) -- foundation for locking and state
- Hash commands (HSET, HGET, HDEL, HVALS) -- lease metadata, DLQ data
- Set commands (SADD, SMEMBERS, SISMEMBER, SREM) -- DLQ membership tracking
- Sorted set commands (ZADD, ZREM, ZRANGE, ZRANGEBYSCORE, ZRANGESTORE, ZREMRANGEBYSCORE) -- lease expiration, event ordering
- Stream commands (full suite: XADD, XREAD, XREADGROUP, XLEN, XACK, XAUTOCLAIM, XTRIM, XINFO, XGROUP) -- entire messaging subsystem
- Lua scripting (EVAL/EVALSHA with redis.call()/redis.pcall()) -- atomic multi-key lease and event operations
- Script management (SCRIPT LOAD, SCRIPT EXISTS) -- redis-py register_script() compatibility
- Pipeline support -- batched atomic operations
- Key expiration (TTL-based, passive + active) -- lock timeouts, lease expiration
- Drop-in redis.asyncio.Redis async API -- the entire value proposition

**Should have (add shortly after validation):**
- Flush to disk / reload from disk -- state surviving process restarts
- Lock/AsyncLock class API -- explicit redis-py Lock compatibility
- Pre-built wheels for all platforms -- Linux, macOS, Windows
- EXPIRE/TTL/PTTL/PERSIST commands -- explicit TTL manipulation

**Defer (v2+):**
- MSET/MGET/INCR/DECR, List commands, HyperLogLog, Bitmaps, Geospatial
- Background auto-save, memory introspection (INFO, DBSIZE)

**Explicit anti-features (never build):**
- Network server / RESP protocol, Pub/Sub, Cluster/Sentinel, Replication, ACL, MULTI/EXEC transactions, RDB/AOF format

### Architecture Approach

The architecture follows a layered pattern: a thin Python wrapper class (`BurnerRedis`) that implements `redis.asyncio.Redis` method signatures, delegating through a PyO3 FFI boundary to a Rust storage engine. The Python layer handles API shape compatibility; all logic lives in Rust. The Rust engine uses a single `parking_lot::RwLock<HashMap<Bytes, Entry>>` keyspace with a typed `Value` enum (String, Hash, Set, SortedSet, Stream). Commands are organized in modules by data type with a dispatch router. Lua scripts execute while holding the write lock for atomicity.

**Major components:**
1. **BurnerRedis (Python)** -- Drop-in replacement class matching redis.asyncio.Redis method signatures. Pure Python async def methods that delegate to Rust.
2. **PyO3 Bindings (Rust)** -- FFI boundary layer. Releases GIL via `py.allow_threads()` before calling engine. Converts types at the boundary.
3. **Storage Engine (Rust)** -- Owns the keyspace (RwLock<HashMap>). Thread-safe, single-writer design. All data structure operations live here.
4. **Command Router (Rust)** -- Dispatches command names to typed handler functions. Extensible via trait/table pattern.
5. **Expiration Manager (Rust)** -- BTreeSet sorted by expiration timestamp. Dual lazy (check-on-access) + active (periodic sweep via Tokio timer) expiration.
6. **Lua Engine (Rust, mlua)** -- EVAL/EVALSHA with redis.call() callbacks that re-enter the command dispatch. SHA1 script caching.
7. **Persistence (Rust)** -- Snapshot serialize via serde + rmp-serde. Write-then-rename for crash safety. Versioned format with checksums.

**Key architectural decision -- RwLock vs DashMap:** Research surfaced a tension between DashMap (sharded concurrent HashMap, better throughput for independent key access) and RwLock<HashMap> (simpler, better for multi-key atomicity). The recommendation is **parking_lot::RwLock<HashMap>** because: Lua scripts and pipelines require consistent locking across multiple keys; DashMap makes this harder and introduces split-brain risks. The single-writer pattern matches Redis's own model. Performance difference is negligible for an embedded database where operations are microsecond-scale.

**Key architectural decision -- sync vs async Rust functions:** Rust engine functions should be **synchronous** from Python's perspective. The operations complete in microseconds (HashMap lookups), so the overhead of bridging two async runtimes exceeds the operation time. Use `py.allow_threads()` to release the GIL during Rust work, and `async def` wrappers in the Python layer to make them awaitable. Reserve `pyo3-async-runtimes` only for genuinely long-running operations if they arise (e.g., blocking XREADGROUP).

### Critical Pitfalls

1. **GIL deadlocks (Phase 1)** -- Rust locks held while calling back into Python create ABBA deadlocks with the GIL. Prevention: release the GIL before acquiring any Rust lock; design the Rust core to be completely Python-independent; use `Py<T>` for stored data, never `Bound<'py, T>`.

2. **Lua type conversion fidelity (Phase 2)** -- Redis has counterintuitive conversion rules (floats become integers, true becomes 1, false becomes nil, sparse tables truncate). Prevention: port Redis's actual conversion code from `scripting.c`, not the documentation; build byte-for-byte comparison tests against real Redis before integrating Prefect scripts.

3. **Consumer group state machine (Phase 2-3)** -- The PEL, delivery counts, idle times, XAUTOCLAIM, and the distinction between `>` (new messages) and `0` (pending re-delivery) in XREADGROUP form a complex state machine. Prevention: implement the full state machine including recovery paths before exposing stream commands; test with Prefect's actual messaging code.

4. **redis-py API surface mismatch (Phase 1, ongoing)** -- Pipeline methods are NOT coroutines (only `execute()` is), Lock needs token-based ownership with extend/reacquire, return types must be bytes not str. Prevention: study `redis.asyncio.client.py` source code; build compatibility tests running identical operations against both implementations.

5. **Key expiration timing (Phase 2)** -- Lazy-only expiration leaks memory; active-only causes latency spikes. Prevention: implement both from the start; use BTreeSet sorted by timestamp for deterministic active sweeps; cap keys expired per cycle.

## Implications for Roadmap

Based on research, the project has clear dependency chains that dictate phase ordering. The architecture research explicitly lays out a 9-phase build order. Here is a consolidated 6-phase structure that groups logically and respects dependencies.

### Phase 1: Foundation and End-to-End Path
**Rationale:** Everything depends on the core types, engine, and Python-Rust bridge. Getting a single command (SET/GET) working end-to-end through Python -> PyO3 -> Rust validates the entire architectural approach before investing in features.
**Delivers:** Working Python package with BurnerRedis class, string commands (SET with all flags, GET, DELETE, EXISTS), and the command dispatch framework.
**Addresses:** String commands (table stakes), async Python bindings, drop-in API skeleton, command dispatch architecture
**Avoids:** GIL deadlocks (establish the `allow_threads` pattern), async runtime mismatch (prove the sync-from-Python approach), command dispatch sprawl (build the router framework)
**Stack elements:** Rust, PyO3, maturin, parking_lot, bytes, thiserror

### Phase 2: Core Data Types and Expiration
**Rationale:** Hash, Set, and Sorted Set commands are independent of each other and can be built in parallel once the dispatch pattern from Phase 1 is established. Expiration must come before Streams because stream entries and locks depend on TTL.
**Delivers:** Full hash, set, sorted set command support. Key expiration with passive + active strategies. EXPIRE/TTL/PTTL commands.
**Addresses:** Hash commands, set commands, sorted set commands (all table stakes), key expiration
**Avoids:** Key expiration timing bugs (implement dual strategy from the start)
**Stack elements:** BTreeMap + HashMap dual-index for sorted sets, BTreeSet for expiration tracking, Tokio timers for active sweeps

### Phase 3: Streams and Consumer Groups
**Rationale:** Streams are the most complex data structure and Prefect's most critical Redis dependency (the entire messaging subsystem). They depend on the engine patterns established in Phases 1-2 and benefit from the key expiration infrastructure. This phase deserves dedicated focus.
**Delivers:** Full stream commands (XADD, XREAD, XREADGROUP, XLEN, XACK, XAUTOCLAIM, XTRIM, XINFO, XGROUP CREATE/DESTROY), consumer groups with PEL, delivery tracking, and message recovery.
**Addresses:** Stream commands (table stakes -- the highest-complexity feature area)
**Avoids:** Consumer group state machine bugs (implement full PEL, delivery counts, idle time tracking before exposing any commands)

### Phase 4: Lua Scripting and Pipelines
**Rationale:** Lua scripting depends on ALL command types because redis.call() must dispatch to any command. Pipelines are a batching wrapper over commands and benefit from being built when command implementations are stable. Both features enable Prefect's atomic multi-key operations.
**Delivers:** EVAL/EVALSHA with redis.call()/redis.pcall(), SCRIPT LOAD/EXISTS, Pipeline with execute(). Script SHA1 caching.
**Addresses:** Lua scripting, script management, pipeline support (all table stakes)
**Avoids:** Lua type conversion fidelity issues (build conversion test suite first, engine second; test against real Redis outputs)
**Stack elements:** mlua (Lua 5.4), sha1 crate

### Phase 5: API Compatibility, Locking, and Persistence
**Rationale:** With all commands implemented, this phase focuses on making the drop-in claim real: Lock/AsyncLock class, persistence for state survival, and thorough API compatibility testing against redis-py behavior. These features are what move burner-redis from "demo" to "usable."
**Delivers:** Lock class with acquire/release/extend/reacquire and token ownership. Flush to disk / reload from disk with crash-safe write-then-rename and format versioning. Full API compatibility test suite.
**Addresses:** Distributed locking (P2), persistence (P2), drop-in API polish (P1 completion)
**Avoids:** Persistence corruption (write-then-rename + checksums from day one), API surface mismatch (compatibility tests against real redis-py)
**Stack elements:** serde, rmp-serde (MessagePack) for persistence

### Phase 6: Distribution and Polish
**Rationale:** Cross-platform wheel building is the final step before PyPI publication. This phase also covers type stubs, documentation, and any remaining polish items.
**Delivers:** Pre-built wheels for Linux (x86_64, aarch64), macOS (x86_64, arm64), Windows (x86_64). Type stubs (.pyi). CI pipeline. PyPI publication.
**Addresses:** Pre-built wheels (P2), type stubs, distribution
**Stack elements:** maturin-action, GitHub Actions CI matrix

### Phase Ordering Rationale

- **Phases 1-2 first** because every subsequent feature depends on the storage engine, type system, and command dispatch framework. The GIL handling pattern and Python-Rust boundary design must be proven before scaling to more commands.
- **Streams (Phase 3) before Lua (Phase 4)** because Lua scripts call redis.call() on stream commands. Streams are also the highest-risk, highest-complexity feature and benefit from early focus while the codebase is still small.
- **Lua and Pipelines together (Phase 4)** because both are meta-operations that compose other commands. Pipelines also need script management (register_script uses SCRIPT LOAD in pipelines).
- **Persistence late (Phase 5)** because serde derives must be added to all data structures, which means all types must be finalized. The core product works without persistence.
- **Distribution last (Phase 6)** because it has no feature dependencies and is purely a packaging concern.

### Research Flags

Phases likely needing deeper research during planning:
- **Phase 3 (Streams):** Consumer group semantics are the most complex area. Needs dedicated research into Redis stream internals, PEL management, XAUTOCLAIM behavior, and XREADGROUP blocking semantics. Validate against Prefect's actual messaging code paths.
- **Phase 4 (Lua Scripting):** Lua-to-Redis type conversion rules are notoriously subtle. Needs research into Redis `scripting.c` source code for exact conversion behavior. Consider whether Lua 5.4 vs 5.1 number handling (integer/float distinction) creates compatibility issues.
- **Phase 5 (API Compatibility):** Needs research into Prefect's exact redis-py usage patterns -- grep the Prefect codebase for all redis method calls to identify the precise API surface needed.

Phases with standard, well-documented patterns (likely skip phase research):
- **Phase 1 (Foundation):** PyO3 + maturin setup is thoroughly documented. HashMap-based storage is straightforward.
- **Phase 2 (Core Data Types):** Hash, Set, Sorted Set implementations follow standard patterns. Expiration is well-documented in Redis literature.
- **Phase 6 (Distribution):** maturin-action and GitHub Actions CI are templated workflows.

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | All technologies are mature, actively maintained, with official documentation. Every recommendation backed by primary sources. Bincode deprecation verified via RUSTSEC advisory. |
| Features | HIGH | Feature list derived directly from Prefect's known Redis usage (PRs, issues, integration docs). Clear separation between table stakes and deferrals. Competitor analysis confirms the feature combination is novel. |
| Architecture | HIGH | Architecture follows established patterns (mini-redis reference, PyO3 documentation, Redis internals). One tension resolved: RwLock over DashMap for multi-key atomicity. |
| Pitfalls | HIGH | Pitfalls verified across multiple sources (PyO3 GitHub discussions, Redis documentation, community issue trackers). GIL deadlock is the most commonly reported PyO3 issue. |

**Overall confidence:** HIGH

### Gaps to Address

- **Lua 5.4 vs 5.1 number handling:** STACK.md recommends Lua 5.4 for integer support, but PITFALLS.md warns that Redis uses Lua 5.1 and number handling differs between versions (5.3+ distinguishes integers and floats). This needs validation during Phase 4: run Prefect's actual Lua scripts on both Lua 5.1 and 5.4 to verify identical behavior. mlua supports both via a feature flag, so switching is trivial.
- **DashMap vs RwLock finalization:** ARCHITECTURE.md uses DashMap in diagrams and examples, while STACK.md recommends parking_lot::RwLock. Both approaches work. The synthesis recommendation is RwLock for multi-key atomicity simplicity, but this should be validated with a prototype in Phase 1. If per-key concurrency proves necessary, DashMap can be introduced later.
- **Blocking XREADGROUP:** Prefect uses XREADGROUP with BLOCK for long-polling on streams. In an embedded database, "blocking" means the Python caller awaits until new data arrives. This requires a notification mechanism (e.g., Tokio watch channel) that wakes waiters when XADD inserts new data. The async bridge approach (pyo3-async-runtimes) may be needed specifically for this command, even if all other commands are synchronous. Needs Phase 3 research.
- **Prefect's exact API surface:** While we know the commands Prefect uses, the exact method signatures, keyword arguments, and return type expectations need validation by grepping the Prefect source code. This should happen at the start of Phase 1.
- **Persistence format versioning:** rmp-serde (MessagePack) is self-describing but the schema evolution story needs design during Phase 5 -- specifically how to handle loading a persistence file created by an older version of burner-redis when new data types or fields have been added.

## Sources

### Primary (HIGH confidence)
- [PyO3 documentation (v0.28.3)](https://pyo3.rs/) -- Bindings, GIL management, memory model
- [maturin documentation (v1.13.1)](https://www.maturin.rs/) -- Build system, wheel distribution, CI scaffolding
- [pyo3-async-runtimes (v0.28.0)](https://docs.rs/pyo3-async-runtimes/) -- Async bridge between Python asyncio and Rust Tokio
- [mlua documentation (v0.11.6)](https://docs.rs/mlua/) -- Lua embedding, Send constraints, version features
- [Tokio (v1.51.1 LTS)](https://tokio.rs/) -- Async runtime, timers, channels
- [parking_lot (v0.12.5)](https://docs.rs/parking_lot/) -- Fast RwLock/Mutex implementations
- [serde (v1.0.228)](https://serde.rs/) -- Serialization framework
- [rmp-serde (v1.3.1)](https://docs.rs/rmp-serde/) -- MessagePack persistence format
- [Redis command documentation](https://redis.io/docs/latest/commands/) -- Authoritative command semantics
- [Redis Streams documentation](https://redis.io/docs/latest/develop/data-types/streams/) -- Consumer group semantics, PEL
- [Redis Lua API reference](https://redis.io/docs/latest/develop/programmability/lua-api/) -- Type conversion rules
- [redis-py source (asyncio client)](https://github.com/redis/redis-py/) -- Pipeline, Lock, Script API behavior
- [Bincode RUSTSEC-2025-0141](https://lib.rs/crates/bincode) -- Unmaintained advisory
- PyO3 GitHub discussions #3045, #3089 -- GIL deadlock patterns and mitigations

### Secondary (MEDIUM confidence)
- [Prefect Redis integration docs](https://docs.prefect.io/integrations/prefect-redis) -- Configuration and subsystem overview
- [Prefect ConcurrencyLeaseStorage PR #18646](https://github.com/PrefectHQ/prefect/pull/18646) -- Lua + sorted sets usage
- [Prefect Stream Trimming PR #18642](https://github.com/PrefectHQ/prefect/pull/18642) -- XINFO/XGROUP consumer management
- [Prefect EVALSHA Issue #19798](https://github.com/prefecthq/prefect/issues/19798) -- Confirms Lua scripting critical path
- [Redis key expiration internals](https://www.pankajtanwar.in/blog/how-redis-expires-keys-a-deep-dive-into-how-ttl-works-internally-in-redis) -- Lazy + active dual strategy
- [tokio-rs/mini-redis](https://github.com/tokio-rs/mini-redis/) -- Reference Rust Redis architecture

### Tertiary (LOW confidence)
- [fakeredis](https://fakeredis.readthedocs.io/) -- Reference implementation for edge case behavior validation
- [Vedis](https://github.com/symisc/vedis) -- Embedded C Redis alternative (abandoned ~2014, no streams/Lua)
- [rsedis](https://github.com/seppo0010/rsedis) -- Rust Redis reimplementation (abandoned, reference only)

---
*Research completed: 2026-04-10*
*Ready for roadmap: yes*
