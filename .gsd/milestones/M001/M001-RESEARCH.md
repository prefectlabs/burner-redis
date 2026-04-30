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

# Architecture Research

**Domain:** Embedded Redis-compatible in-process database (Rust core + Python bindings)
**Researched:** 2026-04-10
**Confidence:** HIGH

## Standard Architecture

### System Overview

```
                         Python Process
 ================================================================
 |                                                              |
 |   Python Layer (burner_redis/)                               |
 |   ┌──────────────────────────────────────────────────────┐   |
 |   │  BurnerRedis (drop-in for redis.asyncio.Redis)       │   |
 |   │  ┌─────────┐  ┌──────────┐  ┌────────────────────┐  │   |
 |   │  │Commands  │  │Pipeline  │  │Lock/AsyncLock      │  │   |
 |   │  │Mixin     │  │Wrapper   │  │Wrapper             │  │   |
 |   │  └────┬─────┘  └────┬─────┘  └────────┬───────────┘  │   |
 |   │       │             │                  │              │   |
 |   │       └─────────────┴──────────────────┘              │   |
 |   │                     │                                 │   |
 |   │              execute_command()                        │   |
 |   └─────────────────────┼─────────────────────────────────┘   |
 |                         │  PyO3 FFI boundary                  |
 |   ======================│=====================================|
 |                         │                                     |
 |   Rust Core (src/)      ▼                                     |
 |   ┌──────────────────────────────────────────────────────┐   |
 |   │  PyO3 Bindings Layer (src/bindings.rs)               │   |
 |   │  - Command dispatch                                  │   |
 |   │  - Type conversion (Python <-> Rust)                 │   |
 |   │  - GIL release for engine calls                      │   |
 |   └─────────────────────┬────────────────────────────────┘   |
 |                         │                                     |
 |   ┌─────────────────────▼────────────────────────────────┐   |
 |   │  Command Router (src/commands/)                      │   |
 |   │  ┌────────┐ ┌──────┐ ┌─────┐ ┌──────┐ ┌──────────┐ │   |
 |   │  │ String │ │ Hash │ │ Set │ │Sorted│ │ Stream   │ │   |
 |   │  │  Cmds  │ │ Cmds │ │Cmds │ │ Set  │ │  Cmds    │ │   |
 |   │  └───┬────┘ └──┬───┘ └──┬──┘ └──┬───┘ └────┬─────┘ │   |
 |   │      │         │        │       │          │        │   |
 |   │  ┌───┴─────────┴────────┴───────┴──────────┴─────┐  │   |
 |   │  │              Key/Generic Cmds                  │  │   |
 |   │  │         (DEL, EXISTS, EXPIRE, TTL)             │  │   |
 |   │  └───────────────────┬────────────────────────────┘  │   |
 |   └─────────────────────┬┘───────────────────────────────┘   |
 |                         │                                     |
 |   ┌─────────────────────▼────────────────────────────────┐   |
 |   │  Storage Engine (src/engine.rs)                      │   |
 |   │  ┌─────────────────────────────────────────────────┐ │   |
 |   │  │  Keyspace (DashMap<String, Value>)              │ │   |
 |   │  │  - Sharded concurrent HashMap                   │ │   |
 |   │  │  - Each value tagged with type + optional TTL   │ │   |
 |   │  └─────────────────────────────────────────────────┘ │   |
 |   │  ┌─────────────────┐  ┌────────────────────────────┐ │   |
 |   │  │ Expiration Mgr  │  │  Lua Engine (mlua)         │ │   |
 |   │  │ - BTreeSet of   │  │  - EVAL/EVALSHA dispatch   │ │   |
 |   │  │   (Instant,Key) │  │  - redis.call() callback   │ │   |
 |   │  │ - Lazy + active │  │  - Script SHA1 cache       │ │   |
 |   │  └─────────────────┘  └────────────────────────────┘ │   |
 |   │  ┌─────────────────────────────────────────────────┐ │   |
 |   │  │  Persistence (src/persistence.rs)               │ │   |
 |   │  │  - Snapshot serialize via serde + bincode       │ │   |
 |   │  │  - Load on startup, save on shutdown            │ │   |
 |   │  │  - Manual flush API                             │ │   |
 |   │  └─────────────────────────────────────────────────┘ │   |
 |   └──────────────────────────────────────────────────────┘   |
 |                                                              |
 ================================================================
```

### Component Responsibilities

| Component | Responsibility | Typical Implementation |
|-----------|----------------|------------------------|
| **BurnerRedis (Python)** | Drop-in replacement for `redis.asyncio.Redis`. Implements the same method signatures Prefect calls. | Pure Python class with `async def` methods that delegate to Rust via PyO3 |
| **Pipeline Wrapper (Python)** | Buffers commands and executes them atomically as a batch | Python class that collects commands, calls a single Rust `pipeline_execute` |
| **Lock Wrapper (Python)** | Distributed lock semantics compatible with `redis.lock.Lock` | Python class using SET NX/EX + Lua scripts for atomic acquire/release |
| **PyO3 Bindings (Rust)** | FFI boundary: receives Python calls, converts types, dispatches to engine | `#[pyclass]`/`#[pymethods]` structs. Releases GIL before calling engine. |
| **Command Router (Rust)** | Parses command names + args, dispatches to correct data structure handler | Match on command name string, call typed handler functions |
| **Storage Engine (Rust)** | Owns all data. Thread-safe in-memory keyspace with typed values. | `DashMap<String, Entry>` where `Entry` holds typed value + metadata |
| **Expiration Manager (Rust)** | Tracks TTLs, removes expired keys both lazily and actively | `BTreeSet<(Instant, String)>` for ordered expiration + background purge |
| **Lua Engine (Rust)** | Executes EVAL/EVALSHA scripts with `redis.call()` callbacks | `mlua` crate with registered Rust functions for `redis.call`/`redis.pcall` |
| **Persistence (Rust)** | Serializes/deserializes entire keyspace to/from disk | `serde` + `bincode` for snapshot serialization to a single file |

## Recommended Project Structure

```
burner-redis/
├── Cargo.toml                  # Rust crate config (cdylib + pyo3)
├── pyproject.toml              # Python package config (maturin backend)
├── burner_redis/               # Python package
│   ├── __init__.py             # Exports BurnerRedis, Pipeline, Lock
│   ├── client.py               # BurnerRedis class (drop-in for redis.asyncio.Redis)
│   ├── pipeline.py             # Pipeline batching
│   ├── lock.py                 # Lock/AsyncLock implementation
│   └── _burner_redis.pyi       # Type stubs for the Rust extension
├── src/                        # Rust source
│   ├── lib.rs                  # PyO3 module definition, exports
│   ├── bindings.rs             # PyO3 pyclass/pymethods - the FFI boundary
│   ├── engine.rs               # Storage engine: keyspace, entry types
│   ├── commands/               # Command implementations
│   │   ├── mod.rs              # Command dispatch router
│   │   ├── string.rs           # GET, SET (with NX/XX/EX/PX)
│   │   ├── hash.rs             # HSET, HGET, HDEL, HVALS
│   │   ├── set.rs              # SADD, SMEMBERS, SISMEMBER, SREM
│   │   ├── sorted_set.rs       # ZADD, ZREM, ZRANGE, ZRANGEBYSCORE, etc.
│   │   ├── stream.rs           # XADD, XREAD, XREADGROUP, XACK, etc.
│   │   └── generic.rs          # DEL, EXISTS, EXPIRE, TTL, KEYS
│   ├── expiration.rs           # TTL tracking and background purge
│   ├── lua.rs                  # Lua scripting engine (EVAL/EVALSHA)
│   ├── persistence.rs          # Snapshot save/load
│   ├── types.rs                # Value enum, Entry struct, stream types
│   └── error.rs                # Error types mapping to Redis error responses
├── tests/                      # Integration tests
│   ├── test_strings.py         # Python-side string command tests
│   ├── test_streams.py         # Python-side stream tests
│   ├── test_lua.py             # Lua scripting tests
│   └── conftest.py             # Shared fixtures
└── benches/                    # Rust benchmarks
    └── engine_bench.rs         # Performance benchmarks
```

### Structure Rationale

- **`burner_redis/` (Python):** Separate Python package directory following maturin mixed Rust/Python layout. This is where the drop-in API compatibility lives. The Python layer is thin -- it translates `redis.asyncio.Redis` method signatures into calls to the Rust engine. Keeping this in Python (not Rust) makes it easy to match redis-py's exact method signatures, handle Python-native concerns like response callbacks, and iterate quickly on API compatibility.

- **`src/` (Rust):** Pure Rust engine with no Python awareness except in `bindings.rs` and `lib.rs`. The command modules mirror Redis's own command groupings. This separation means the engine can be tested independently from Rust without involving Python at all.

- **`src/commands/`:** One module per data type, matching Redis's own categorization. Each module contains pure functions that take a reference to the engine and return results. This makes adding new commands trivial -- add a function, wire it into the router.

- **`tests/`:** Python-side integration tests that exercise the full stack from Python through PyO3 to Rust. These are the primary correctness tests since they validate the actual API surface Prefect will use.

## Architectural Patterns

### Pattern 1: Thin Python Wrapper over Rust Engine

**What:** The Python `BurnerRedis` class implements the same method signatures as `redis.asyncio.Redis` but delegates all work to a Rust `#[pyclass]` engine object. The Python layer handles only API shape compatibility; all logic lives in Rust.

**When to use:** Always -- this is the core architectural pattern for the entire project.

**Trade-offs:** (+) Iterate quickly on API compatibility in Python, (+) Rust engine is independently testable, (+) Clear separation of concerns. (-) Two-language boundary adds complexity for debugging, (-) Type conversion overhead at the boundary (small but non-zero).

**Example:**
```python
# burner_redis/client.py
from burner_redis._burner_redis import Engine

class BurnerRedis:
    def __init__(self, db_path: str | None = None):
        self._engine = Engine(db_path)

    async def set(self, name: str, value, ex=None, px=None, nx=False, xx=False):
        """Drop-in compatible with redis.asyncio.Redis.set()"""
        return self._engine.set(name, value, ex=ex, px=px, nx=nx, xx=xx)

    async def get(self, name: str):
        return self._engine.get(name)
```

```rust
// src/bindings.rs
#[pyclass]
struct Engine {
    inner: Arc<StorageEngine>,
}

#[pymethods]
impl Engine {
    #[new]
    fn new(db_path: Option<String>) -> PyResult<Self> { ... }

    fn set(&self, py: Python<'_>, key: String, value: Vec<u8>,
           ex: Option<u64>, px: Option<u64>,
           nx: bool, xx: bool) -> PyResult<Option<bool>> {
        // Release GIL for the Rust-only work
        py.allow_threads(|| {
            self.inner.set(key, value, ex, px, nx, xx)
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))
        })
    }
}
```

### Pattern 2: Typed Value Enum with Sharded Keyspace

**What:** Store all Redis values in a single keyspace using a `DashMap<String, Entry>` where `Entry` contains a typed `Value` enum (String, Hash, Set, SortedSet, Stream) plus optional expiration metadata. DashMap provides lock-free concurrent reads and sharded writes.

**When to use:** For the core storage engine. This mirrors how Redis itself uses a single key namespace with type-tagged values.

**Trade-offs:** (+) Single lookup path for any key, (+) DashMap's sharding reduces contention vs `RwLock<HashMap>`, (+) Type safety via enum. (-) Slightly more complex than separate maps per type, (-) DashMap's API is more constrained than raw HashMap for complex mutations.

**Example:**
```rust
// src/types.rs
use bytes::Bytes;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Instant;

#[derive(Clone, Serialize, Deserialize)]
pub enum Value {
    String(Bytes),
    Hash(HashMap<String, Bytes>),
    Set(HashSet<String>),
    SortedSet(SortedSetData),
    Stream(StreamData),
}

#[derive(Clone)]
pub struct Entry {
    pub value: Value,
    pub expires_at: Option<Instant>,
}

// src/engine.rs
use dashmap::DashMap;

pub struct StorageEngine {
    keyspace: DashMap<String, Entry>,
    expirations: Mutex<BTreeSet<(Instant, String)>>,
}
```

### Pattern 3: Lua Engine with redis.call() Callback Bridge

**What:** Embed a Lua 5.4 interpreter via `mlua` and register Rust functions as `redis.call()` and `redis.pcall()` in the Lua global scope. When a Lua script calls `redis.call('SET', key, value)`, it invokes a Rust closure that routes through the same command dispatch as direct API calls. Scripts see the KEYS and ARGV tables just like real Redis.

**When to use:** For EVAL/EVALSHA support. Prefect uses Lua scripts for atomic multi-key operations (lease management, event ordering), so a real Lua interpreter is more reliable than trying to reimplement each script's semantics.

**Trade-offs:** (+) Faithful Redis Lua semantics, (+) Works with existing Prefect Lua scripts unchanged, (+) mlua is mature and well-maintained. (-) Adds a native dependency (Lua library), (-) Lua execution must hold the keyspace lock to ensure atomicity (but this matches Redis's single-threaded execution model for scripts).

**Example:**
```rust
// src/lua.rs
use mlua::{Lua, Result as LuaResult, Value as LuaValue};

pub struct LuaEngine {
    lua: Lua,
    script_cache: HashMap<String, String>,  // SHA1 -> script body
}

impl LuaEngine {
    pub fn new() -> Self {
        let lua = Lua::new();
        // redis.call and redis.pcall are registered per-execution
        // with a reference to the current engine state
        Self { lua, script_cache: HashMap::new() }
    }

    pub fn eval(
        &self,
        engine: &StorageEngine,
        script: &str,
        keys: Vec<String>,
        args: Vec<Bytes>,
    ) -> Result<RedisValue, RedisError> {
        let sha = sha1_hex(script);
        self.script_cache.entry(sha).or_insert(script.to_string());

        // Register redis.call as a Lua function that calls back into engine
        let redis_table = self.lua.create_table()?;
        redis_table.set("call", self.lua.create_function(|_, args: mlua::MultiValue| {
            // Parse command name + args, dispatch to engine
            // This runs inside the engine's lock scope for atomicity
        })?)?;
        self.lua.globals().set("redis", redis_table)?;

        // Set KEYS and ARGV tables
        // Execute script
        // Convert result back to RedisValue
    }
}
```

### Pattern 4: Dual Expiration Strategy (Lazy + Active)

**What:** Use two complementary strategies for key expiration. Lazy: check TTL on every key access, delete if expired. Active: a background task periodically scans the `BTreeSet<(Instant, String)>` and removes expired keys. This mirrors Redis's own dual strategy.

**When to use:** Always -- this is essential for correct TTL behavior.

**Trade-offs:** (+) Lazy ensures expired keys are never returned, (+) Active prevents memory leaks from unaccessed expired keys, (+) BTreeSet gives O(log n) ordered access to next expiration. (-) Background task adds complexity, (-) Must coordinate between lazy deletion and active deletion to avoid double-remove issues.

**Example:**
```rust
// src/expiration.rs
use std::collections::BTreeSet;
use std::sync::Mutex;
use std::time::Instant;

pub struct ExpirationManager {
    /// Sorted by expiration time for efficient "next to expire" queries
    expirations: Mutex<BTreeSet<(Instant, String)>>,
}

impl ExpirationManager {
    pub fn track(&self, key: String, expires_at: Instant) {
        self.expirations.lock().unwrap().insert((expires_at, key));
    }

    pub fn purge_expired(&self, keyspace: &DashMap<String, Entry>) -> Option<Instant> {
        let now = Instant::now();
        let mut expirations = self.expirations.lock().unwrap();
        while let Some((when, key)) = expirations.iter().next().cloned() {
            if when > now {
                return Some(when);  // Next expiration time for sleep
            }
            expirations.remove(&(when, key.clone()));
            keyspace.remove(&key);
        }
        None
    }
}
```

## Data Flow

### Command Execution Flow

```
Python: await client.set("mykey", "myvalue", ex=60)
    |
    ▼
BurnerRedis.set() [Python - burner_redis/client.py]
    |  Normalizes args to match redis-py signature
    ▼
Engine.set() [Rust PyO3 - src/bindings.rs]
    |  py.allow_threads(|| { ... })  -- releases GIL
    ▼
StorageEngine.set() [Rust - src/engine.rs]
    |  Acquires DashMap shard lock (automatic, per-key)
    |  Checks NX/XX conditions
    |  Inserts/updates Entry in keyspace
    |  Registers expiration if EX/PX provided
    ▼
Return Ok(true) → PyO3 converts to Python bool → Python receives True
```

### Lua Script Execution Flow

```
Python: await client.evalsha(sha, num_keys, *keys, *args)
    |
    ▼
Engine.evalsha() [Rust PyO3 - src/bindings.rs]
    |  py.allow_threads(|| { ... })
    ▼
LuaEngine.eval() [Rust - src/lua.rs]
    |  Look up script body by SHA1
    |  Register redis.call() callback pointing to StorageEngine
    |  Set KEYS and ARGV tables in Lua state
    |  Execute script
    |     |
    |     ▼ (within Lua execution)
    |   redis.call('ZADD', KEYS[1], score, member)
    |     |
    |     ▼ (Rust callback)
    |   StorageEngine.zadd()  -- same path as direct command
    |     |
    |     ▼ (returns to Lua)
    |   result available in Lua script
    |
    ▼
Convert Lua result → RedisValue → PyO3 → Python object
```

### Pipeline Execution Flow

```
Python:
    pipe = client.pipeline()
    pipe.set("a", "1")
    pipe.set("b", "2")
    pipe.get("a")
    results = await pipe.execute()
    |
    ▼
Pipeline collects commands as list of (cmd_name, args) [Python]
    |
    ▼
Engine.pipeline_execute(commands) [Rust PyO3]
    |  Acquires engine-wide lock (not per-shard)
    |  Executes each command sequentially
    |  Collects results into Vec
    |  Releases lock
    ▼
Returns list of results → Python receives [True, True, b"1"]
```

### Persistence Flow

```
Save (manual or shutdown):
    StorageEngine.save() → Acquire read snapshot
        → serde::Serialize entire keyspace
        → bincode::encode to bytes
        → Write to temp file
        → Atomic rename to target path

Load (startup):
    StorageEngine::load(path) → Read file bytes
        → bincode::decode
        → serde::Deserialize to keyspace
        → Rebuild expiration index
        → Discard already-expired keys
```

### Key Data Flows

1. **Direct command:** Python method call -> PyO3 binding (GIL released) -> command handler -> DashMap lookup/mutation -> return value converted back to Python
2. **Lua script:** Same as direct command, but the Lua interpreter sits between the PyO3 binding and the command handlers. `redis.call()` re-enters the command dispatch path.
3. **Pipeline:** Same as direct command, but commands are batched. The entire batch runs under a single lock scope for atomicity.
4. **Persistence:** Orthogonal to command flow. Triggered by explicit API call or shutdown hook. Serializes the keyspace snapshot.

## Scaling Considerations

| Scale | Architecture Adjustments |
|-------|--------------------------|
| Single Prefect server (target) | Current architecture is correct. DashMap sharding handles concurrent async tasks well. No changes needed. |
| Heavy stream workload (1000s of messages/sec) | Stream data structure may need a more memory-efficient representation than `Vec<StreamEntry>`. Consider a VecDeque or ring buffer with configurable max length. |
| Large keyspaces (100k+ keys) | DashMap scales well here. Persistence snapshot time may grow -- consider incremental/dirty-flag approach to avoid serializing unchanged data. |
| Multiple Prefect workers in same process | Already handled -- DashMap is thread-safe. Multiple Python async tasks hitting the same engine is the expected pattern. |

### Scaling Priorities

1. **First bottleneck: Lua script contention.** Lua scripts must execute atomically (matching Redis semantics), so they hold an exclusive lock. If Prefect runs many concurrent Lua scripts, this becomes a serialization point. Mitigation: Lua scripts in Prefect are short-lived (lease acquire/release), so this is unlikely to be a practical issue, but monitor script execution times.

2. **Second bottleneck: Persistence snapshot time.** As the keyspace grows, full-snapshot serialization time grows linearly. Mitigation: Use a background thread for serialization with a consistent snapshot (clone the keyspace into a serializable form, then serialize outside the lock). The `bincode` format is fast -- 135MB in ~4 seconds per community benchmarks.

## Anti-Patterns

### Anti-Pattern 1: Implementing the Redis Wire Protocol (RESP)

**What people do:** Build a TCP server that speaks RESP, then connect to it with `redis-py`.
**Why it's wrong:** The project is embedded/in-process. Adding a TCP server + RESP encoding/decoding adds latency, complexity, and a network dependency for something that should be a direct function call. It also requires managing a server lifecycle (start, stop, port allocation).
**Do this instead:** Direct function call through PyO3. The Python wrapper class implements the same method signatures as `redis.asyncio.Redis`, so Prefect code doesn't know the difference. No serialization, no sockets, no server management.

### Anti-Pattern 2: Single `Mutex<HashMap>` for the Entire Keyspace

**What people do:** Wrap the entire keyspace in one `std::sync::Mutex` or `tokio::sync::Mutex`.
**Why it's wrong:** Every command -- even reads on different keys -- contends for the same lock. With async Python tasks issuing concurrent commands, this becomes a severe bottleneck. The tokio version is even worse because holding it across `.await` points blocks the runtime.
**Do this instead:** Use `DashMap` which provides per-shard locking. Most commands touch one key and only lock one shard. For operations requiring atomicity across multiple keys (pipelines, Lua scripts), use a separate coordination mechanism.

### Anti-Pattern 3: Exposing Rust Async Functions Directly to Python

**What people do:** Use `pyo3-async-runtimes` to make every Rust function async and awaitable from Python, running a tokio runtime in the background.
**Why it's wrong:** For an in-process embedded database, most operations complete in microseconds (HashMap lookup). The overhead of bridging two async runtimes (Python asyncio + Rust tokio) exceeds the actual operation time. It adds massive complexity for no benefit. The async runtime bridge is also a source of subtle bugs around GIL management and context variables.
**Do this instead:** Make the Rust functions synchronous from Python's perspective. Release the GIL with `py.allow_threads()` so other Python tasks can run, but the Rust operation itself is a blocking call that completes nearly instantly. The Python `async def` wrapper makes it awaitable without needing Rust async. Only consider `pyo3-async-runtimes` if you add genuinely long-running async operations (network I/O, disk I/O that benefits from non-blocking).

### Anti-Pattern 4: Separate Storage Maps per Data Type

**What people do:** Use one `HashMap` for strings, another for hashes, another for sets, etc.
**Why it's wrong:** Redis has a single keyspace where a key can only be one type. Operations like `DEL`, `EXISTS`, `EXPIRE`, and `TYPE` must work across all types. Separate maps mean these operations must check every map, and type conflicts become impossible to detect.
**Do this instead:** Single `DashMap<String, Entry>` with a `Value` enum. Type checking happens naturally -- if you call `HGET` on a string key, the enum match fails and you return a WRONGTYPE error, just like Redis.

### Anti-Pattern 5: Trying to Subclass redis.asyncio.Redis

**What people do:** Inherit from `redis.asyncio.Redis` and override connection-related methods to intercept commands.
**Why it's wrong:** `redis.asyncio.Redis` has deep coupling to connection pools, RESP protocol parsing, and socket I/O. Subclassing creates a fragile dependency on redis-py internals that break across versions. The class hierarchy uses mixin inheritance (CoreCommands, RedisModuleCommands, SentinelCommands) that adds complexity without value for an embedded use case.
**Do this instead:** Create a standalone `BurnerRedis` class that implements the same public method signatures. Use duck typing -- Prefect calls `await redis.set(...)`, it doesn't check `isinstance(redis, Redis)`. If explicit protocol compliance is needed later, define a shared Protocol/ABC.

## Integration Points

### External Services

| Service | Integration Pattern | Notes |
|---------|---------------------|-------|
| Prefect Server | Drop-in replacement for `redis.asyncio.Redis` | Prefect instantiates `BurnerRedis` instead of `Redis`. Same method signatures. |
| File system | Persistence snapshots | Single file written atomically (write temp + rename). Path configurable at init. |
| PyPI | Distribution via maturin | Pre-built wheels for manylinux, macOS (x86_64 + arm64), Windows. `maturin build --release`. |

### Internal Boundaries

| Boundary | Communication | Notes |
|----------|---------------|-------|
| Python `BurnerRedis` <-> Rust `Engine` | Direct PyO3 function calls | GIL released before engine operations. Types converted at boundary (Python `bytes`/`str` <-> Rust `Vec<u8>`/`String`). |
| Rust `Engine` <-> `StorageEngine` | Direct Rust method calls | No serialization. Engine holds `Arc<StorageEngine>`. |
| `Command Router` <-> `StorageEngine` | Direct method calls | Each command function receives `&StorageEngine` and returns `Result<RedisValue, RedisError>`. |
| `LuaEngine` <-> `StorageEngine` | Rust closure callback | `redis.call()` in Lua invokes a Rust closure that calls back into StorageEngine. Must run under atomicity guarantee. |
| `ExpirationManager` <-> `StorageEngine` | Shared reference | ExpirationManager reads/writes its own `BTreeSet` and removes keys from the DashMap. Runs on a background thread or is triggered lazily. |
| `Persistence` <-> `StorageEngine` | Snapshot-and-serialize | Persistence module iterates the DashMap, serializes entries. Does not hold locks during disk I/O -- takes a snapshot first. |

## Build Order (Dependency Graph)

The components have clear build-order dependencies. This informs which phases to build in what sequence:

```
Phase 1: Foundation
  types.rs + error.rs                (no dependencies)
       │
       ▼
  engine.rs (StorageEngine)          (depends on types)
       │
       ▼
  commands/string.rs + generic.rs    (depends on engine + types)
       │
       ▼
  bindings.rs (PyO3 Engine class)    (depends on engine + commands)
       │
       ▼
  client.py (BurnerRedis)            (depends on bindings)

Phase 2: Core Data Types
  commands/hash.rs                   (depends on engine)
  commands/set.rs                    (depends on engine)
  commands/sorted_set.rs             (depends on engine)

Phase 3: Expiration
  expiration.rs                      (depends on engine)
       │  integrated into engine.rs

Phase 4: Streams
  commands/stream.rs                 (depends on engine, types - complex)
       │  consumer groups, PEL tracking

Phase 5: Lua Scripting
  lua.rs                             (depends on engine, commands - all of them)
       │  redis.call() must dispatch to any command

Phase 6: Pipeline
  pipeline support in bindings.rs    (depends on engine, all commands)
  pipeline.py                        (depends on pipeline bindings)

Phase 7: Persistence
  persistence.rs                     (depends on engine, types)
       │  serde derives on all types

Phase 8: Lock Support
  lock.py                            (depends on client, Lua or SET NX/EX)

Phase 9: Polish + Distribution
  maturin wheel builds
  type stubs
  packaging
```

**Build order rationale:**
- **types/engine first** because everything depends on the core storage layer.
- **String commands + bindings** next to get the full Python-to-Rust path working end-to-end as early as possible. This validates the entire architectural approach.
- **Hash/Set/SortedSet** can be done in parallel once the command dispatch pattern is established.
- **Expiration** before streams because streams with TTL need expiration support, and because TTL is simpler to implement.
- **Streams** are the most complex data structure (consumer groups, PEL, XREADGROUP blocking semantics) and benefit from the patterns established in simpler types.
- **Lua** comes after all command types exist because `redis.call()` needs to dispatch to any command.
- **Pipeline** comes after commands are stable since it's a batching wrapper.
- **Persistence** comes late because it requires `serde` derives on all types, which means all types must be finalized. It's also lower risk -- the core product works without persistence.
- **Lock** is a thin Python-layer feature built on SET NX + Lua scripts.

## Sources

- [tokio-rs/mini-redis](https://github.com/tokio-rs/mini-redis/) - Reference architecture for Rust in-memory Redis implementation
- [mini-redis db.rs](https://github.com/tokio-rs/mini-redis/blob/master/src/db.rs) - Shared state pattern with Mutex<State> + background expiration
- [DashMap](https://github.com/xacrimon/dashmap) - Concurrent sharded HashMap for Rust
- [mlua](https://github.com/mlua-rs/mlua) - Lua 5.x bindings for Rust (successor to rlua)
- [PyO3](https://pyo3.rs/) - Rust bindings for the Python interpreter
- [pyo3-async-runtimes](https://github.com/PyO3/pyo3-async-runtimes) - Async bridge between Python and Rust runtimes
- [Maturin](https://www.maturin.rs/) - Build and publish PyO3 crates as Python packages
- [Redis EVAL documentation](https://redis.io/docs/latest/commands/eval/) - Lua scripting semantics
- [Redis data structures](https://redis.io/technology/data-structures/) - Internal data structure design
- [Redis key expiration](https://redis.io/docs/latest/commands/expire/) - Lazy + active expiration strategy
- [redis-py asyncio client architecture](https://deepwiki.com/redis/redis-py/3.3-asyncio-client) - Client class hierarchy and execute_command pattern
- [rsedis](https://github.com/seppo0010/rsedis) - Redis re-implemented in Rust (reference)
- [bincode](https://docs.rs/bincode/latest/bincode/) - Fast binary serialization for Rust
- [serde](https://serde.rs/) - Serialization framework for Rust

---
*Architecture research for: Embedded Redis-compatible in-process database*
*Researched: 2026-04-10*

# Technology Stack

**Project:** Burner Redis
**Researched:** 2026-04-10

## Recommended Stack

### Language & Edition

| Technology | Version | Purpose | Why | Confidence |
|------------|---------|---------|-----|------------|
| Rust | 1.85+ (stable) | Core engine language | Memory safety without GC, zero-cost abstractions, excellent Python interop via PyO3. Edition 2024 (stable since Rust 1.85) brings async closures and improved lifetime capture -- both useful for the async bridge layer. | HIGH |
| Python | 3.9+ | Binding target | Minimum target for `redis.asyncio` users running Prefect. PyO3 supports CPython 3.7+, but 3.9+ is the practical floor for active Python versions in 2026. | HIGH |

### Python Bindings

| Technology | Version | Purpose | Why | Confidence |
|------------|---------|---------|-----|------------|
| [PyO3](https://pyo3.rs/) | 0.28.x (latest: 0.28.3) | Rust-to-Python bindings | The standard for Rust-Python interop. Mature, well-documented, actively maintained. Supports `#[pyclass]`, `#[pymethods]`, async via companion crate, and free-threaded CPython 3.13t. Use `abi3-py39` feature for single-wheel-per-platform builds. | HIGH |
| [maturin](https://www.maturin.rs/) | 1.13.x (latest: 1.13.1) | Build & publish wheels | The standard build tool for PyO3 projects. Handles manylinux compliance, cross-compilation, wheel building, and PyPI uploads. Use `maturin generate-ci github` for CI scaffolding. | HIGH |
| [pyo3-async-runtimes](https://github.com/PyO3/pyo3-async-runtimes) | 0.28.0 | Python-Rust async bridge | Bridges Python's asyncio event loop with Tokio. Provides `future_into_py()` to expose Rust futures as Python awaitables and `into_future()` to call Python coroutines from Rust. Use with `tokio-runtime` feature. This replaced the older `pyo3-asyncio` crate. | HIGH |

### Async Runtime

| Technology | Version | Purpose | Why | Confidence |
|------------|---------|---------|-----|------------|
| [Tokio](https://tokio.rs/) | 1.51.x (LTS until Mar 2027) | Async runtime | The Rust async runtime. Required by `pyo3-async-runtimes`. Use the current-thread runtime (not multi-threaded) since we run inside a Python process and must respect the GIL. Features needed: `rt`, `time` (for TTL expiration timers), `sync` (for channels/mutexes). | HIGH |

### Core Data Structures

These are custom implementations -- not off-the-shelf crates. The Redis data model has specific semantics (RESP types, score-based sorting, stream IDs with time-sequence pairs) that no existing Rust crate matches directly. Build from standard library primitives:

| Data Structure | Rust Implementation | Purpose | Why | Confidence |
|----------------|---------------------|---------|-----|------------|
| Key-value store | `HashMap<Bytes, RedisValue>` | Top-level keyspace | Standard HashMap provides O(1) key lookup. Use `bytes::Bytes` for keys to avoid excessive cloning. | HIGH |
| Strings | `Bytes` | Redis string values | Reference-counted, zero-copy byte buffers. Same type used by Tokio ecosystem. | HIGH |
| Hashes | `HashMap<Bytes, Bytes>` | Redis HSET/HGET | Direct mapping to Rust HashMap. No special requirements. | HIGH |
| Sets | `HashSet<Bytes>` | Redis SADD/SMEMBERS | Direct mapping to Rust HashSet. | HIGH |
| Sorted Sets | `BTreeMap<(f64, Bytes), ()>` + `HashMap<Bytes, f64>` | Redis ZADD/ZRANGE/ZRANGEBYSCORE | Dual-index pattern: BTreeMap for score-ordered range queries, HashMap for O(1) member-to-score lookup. This mirrors Redis's own skiplist+dict implementation but uses Rust's BTreeMap (which has better cache locality than a skip list in single-threaded use). | HIGH |
| Streams | Custom radix-tree or `BTreeMap<StreamId, Entry>` | Redis XADD/XREAD/XREADGROUP | Streams need ordered insertion by ID (timestamp-sequence pairs). Start with BTreeMap keyed by StreamId; the semantics matter more than micro-optimization. A radix tree (like Redis's `rax`) is a future optimization if memory or throughput becomes an issue. | MEDIUM |
| Consumer Groups | `HashMap<String, ConsumerGroup>` per stream | XREADGROUP/XACK/XAUTOCLAIM | Custom struct tracking: last-delivered-id, pending entries list (PEL) per consumer, consumer metadata. No existing crate covers this. | MEDIUM |

### Lua Scripting Engine

| Technology | Version | Purpose | Why | Confidence |
|------------|---------|---------|-----|------------|
| [mlua](https://github.com/mlua-rs/mlua) | 0.11.x (latest: 0.11.6) | Embedded Lua interpreter | Best Rust-Lua binding library. Supports Lua 5.4 (Redis uses 5.1, but 5.4 is backward-compatible for Prefect's scripts). Use `lua54` feature for the interpreter and `send` feature for thread safety. Async support available but not needed -- Lua scripts in Redis execute synchronously and atomically. | HIGH |

**Why mlua over alternatives:**
- `rlua` is the predecessor to mlua, now deprecated in favor of mlua by the same maintainer organization.
- `hlua` is unmaintained.
- mlua is the only actively maintained Rust Lua binding with comprehensive feature support.

**Lua version choice:** Use Lua 5.4 (feature `lua54`). Redis itself embeds Lua 5.1, but Prefect's Lua scripts use basic table/string operations and `redis.call()` that work identically across versions. Lua 5.4 gives us integers (important for sorted set scores) and better error handling. If strict Redis Lua 5.1 compatibility is ever needed, mlua supports it via the `lua51` feature -- a one-line change.

### Concurrency & Synchronization

| Technology | Version | Purpose | Why | Confidence |
|------------|---------|---------|-----|------------|
| [parking_lot](https://github.com/Amanieu/parking_lot) | 0.12.x (latest: 0.12.5) | RwLock / Mutex | 1.5x faster uncontended, up to 5x faster under contention vs std. Smaller (1 byte for Mutex). Use `RwLock` for the main keyspace -- reads are far more common than writes. | HIGH |
| [bytes](https://docs.rs/bytes/) | 1.11.x (latest: 1.11.1) | Byte buffer type | Reference-counted, zero-copy byte slices. Avoid cloning string data throughout the engine. Part of the Tokio ecosystem, so it integrates cleanly. | HIGH |

**Why NOT DashMap:** DashMap (concurrent sharded hashmap) adds complexity for marginal benefit in our case. burner-redis is single-process, embedded, and operations hold the GIL briefly. A `parking_lot::RwLock<HashMap>` is simpler, easier to reason about for atomic multi-key operations (Lua scripts, pipelines), and avoids the split-brain issue where a Lua script touching multiple keys would need to lock multiple shards.

### Serialization (Persistence)

| Technology | Version | Purpose | Why | Confidence |
|------------|---------|---------|-----|------------|
| [serde](https://serde.rs/) | 1.0.x (latest: 1.0.228) | Serialization framework | The standard. Derive macros for all data structures. | HIGH |
| [rmp-serde](https://github.com/3Hren/msgpack-rust) | 1.3.x (latest: 1.3.1) | MessagePack format for persistence | Compact binary format, ~70% the size of bincode with only ~1.5x overhead. Cross-language readable (useful for debugging persistence files). Self-describing format means forward compatibility when data structures evolve. | HIGH |

**Why NOT bincode:** Bincode 3.0 is unmaintained (RUSTSEC-2025-0141) due to maintainer harassment. The final release is a compiler error. Bincode 2.0.1 still works but receives no security patches. Avoid.

**Why NOT postcard:** Postcard (1.1.3) is the recommended bincode replacement and is excellent for embedded/no_std. But rmp-serde is better here because: (1) MessagePack is self-describing, making persistence file evolution easier; (2) MessagePack has cross-language tooling for debugging; (3) persistence is not a hot path -- the ~1.5x overhead vs postcard is irrelevant for save/load operations.

### Error Handling

| Technology | Version | Purpose | Why | Confidence |
|------------|---------|---------|-----|------------|
| [thiserror](https://github.com/dtolnay/thiserror) | 2.0.x (latest: 2.0.18) | Error type derivation | Derive `std::error::Error` implementations cleanly. Use for the Rust-side error hierarchy. | HIGH |

### Testing & Benchmarking

| Technology | Version | Purpose | Why | Confidence |
|------------|---------|---------|-----|------------|
| Built-in `#[test]` | -- | Unit tests | Rust's built-in test framework for unit and integration tests. | HIGH |
| [criterion](https://github.com/bheisler/criterion.rs) | 0.8.x (latest: 0.8.2) | Benchmarking | Statistics-driven benchmarks with HTML reports. Use for command throughput regression testing. | HIGH |
| [pytest](https://pytest.org/) | latest | Python integration tests | Test the Python API surface against `redis.asyncio.Redis` behavior. Run the same test suite against both real Redis and burner-redis to verify compatibility. | HIGH |
| [fakeredis](https://fakeredis.readthedocs.io/) | latest | Reference behavior | Use as a reference implementation (not a dependency) to validate expected Redis behavior in edge cases. | LOW |

### CI/CD & Distribution

| Technology | Version | Purpose | Why | Confidence |
|------------|---------|---------|-----|------------|
| [maturin-action](https://github.com/PyO3/maturin-action) | v1 | GitHub Actions CI | Official GitHub Action for building cross-platform wheels. Handles manylinux, macOS (x86_64 + ARM), and Windows automatically. Use `maturin generate-ci github` to scaffold the workflow. | HIGH |
| GitHub Actions | -- | CI platform | Build matrix: Linux (manylinux2014 x86_64 + aarch64), macOS (x86_64 + arm64), Windows (x86_64). | HIGH |

## Project Structure

```
burner-redis/
  Cargo.toml              # Rust workspace root
  pyproject.toml          # Python package metadata (maturin build backend)
  src/
    lib.rs                # PyO3 module entry point
    engine/
      mod.rs              # Core engine (keyspace, expiration, command dispatch)
      types.rs            # RedisValue enum, key types
      strings.rs          # String commands (GET, SET, etc.)
      hashes.rs           # Hash commands
      sets.rs             # Set commands
      sorted_sets.rs      # Sorted set commands
      streams.rs          # Stream commands + consumer groups
      expiry.rs           # TTL expiration manager (tokio timer-based)
      persistence.rs      # Save/load to disk (serde + rmp-serde)
    lua/
      mod.rs              # Lua script execution engine (mlua)
      redis_api.rs        # redis.call() / redis.pcall() bindings for Lua
      script_cache.rs     # SHA1-based script caching (EVALSHA)
    pipeline.rs           # Pipeline/batch command execution
    error.rs              # Error types (thiserror)
  python/
    burner_redis/
      __init__.py         # Python package
      _core.pyi           # Type stubs for the Rust extension
      client.py           # AsyncRedis wrapper (drop-in for redis.asyncio.Redis)
      lock.py             # AsyncLock implementation
      pipeline.py         # Pipeline wrapper
  tests/
    rust/                 # Rust unit + integration tests
    python/               # pytest integration tests
  benches/                # criterion benchmarks
```

## Key Cargo.toml Configuration

```toml
[package]
name = "burner-redis"
edition = "2024"

[lib]
name = "_burner_redis"
crate-type = ["cdylib"]

[dependencies]
pyo3 = { version = "0.28", features = ["abi3-py39", "extension-module"] }
pyo3-async-runtimes = { version = "0.28", features = ["tokio-runtime"] }
tokio = { version = "1.51", features = ["rt", "time", "sync"] }
mlua = { version = "0.11", features = ["lua54", "send"] }
parking_lot = "0.12"
bytes = "1.11"
serde = { version = "1.0", features = ["derive"] }
rmp-serde = "1.3"
thiserror = "2.0"
sha1 = "0.10"          # For EVALSHA script caching

[dev-dependencies]
criterion = { version = "0.8", features = ["html_reports"] }
```

## Key pyproject.toml Configuration

```toml
[build-system]
requires = ["maturin>=1.13,<2.0"]
build-backend = "maturin"

[project]
name = "burner-redis"
requires-python = ">=3.9"
classifiers = [
    "Programming Language :: Rust",
    "Programming Language :: Python :: Implementation :: CPython",
]

[tool.maturin]
features = ["pyo3/extension-module"]
python-source = "python"
module-name = "burner_redis._core"
```

## Alternatives Considered

| Category | Recommended | Alternative | Why Not |
|----------|-------------|-------------|---------|
| Lua engine | mlua (Lua 5.4) | rlua | rlua is deprecated; mlua is its successor by the same org |
| Lua engine | mlua (Lua 5.4) | Custom EVAL interpreter | Prefect's Lua scripts are complex (control flow, loops, multi-key). A real Lua engine is more maintainable than a custom interpreter |
| Serialization | rmp-serde (MessagePack) | bincode | Bincode is unmaintained (RUSTSEC-2025-0141) |
| Serialization | rmp-serde (MessagePack) | postcard | Postcard is excellent but not self-describing; MessagePack handles schema evolution better for persistence files |
| Serialization | rmp-serde (MessagePack) | serde_json | JSON is human-readable but 3-5x larger and slower; persistence is not user-facing |
| Sorted set impl | BTreeMap + HashMap | crossbeam-skiplist | Skip lists are for concurrent writes; we use RwLock for atomicity. BTreeMap has better cache locality in single-writer scenarios |
| Concurrency | parking_lot RwLock | DashMap | Atomic multi-key operations (Lua, pipelines) require consistent locking across keys; sharded maps make this harder |
| Concurrency | parking_lot RwLock | std::sync::RwLock | parking_lot is measurably faster (1.5-5x) and smaller |
| Async bridge | pyo3-async-runtimes | pyo3-asyncio | pyo3-asyncio is the old name/crate; pyo3-async-runtimes is the maintained successor for PyO3 0.21+ |
| Key-value store | Custom in-memory | sled / redb | These are on-disk embedded DBs. We need in-memory with optional persistence, not a storage engine |
| Build tool | maturin | setuptools-rust | maturin is purpose-built for PyO3, handles cross-compilation and manylinux, and generates CI configs |

## Architecture Decision: GIL & Threading Strategy

The Python wrapper should:

1. **Acquire the GIL only when crossing the Python-Rust boundary** -- use `Python::allow_threads()` to release the GIL during Rust-side computation.
2. **Run Tokio on a background thread** -- use `pyo3-async-runtimes` to manage a Tokio runtime that runs independently of Python's event loop, with futures bridged via `future_into_py()`.
3. **Keep the Rust engine single-threaded internally** -- use `parking_lot::RwLock` on the keyspace for safety, but design for the common case of single-writer access. This simplifies atomic operations (Lua scripts execute while holding a write lock).

This avoids the complexity of true concurrent access while still allowing Python's async code to `await` Rust operations without blocking the event loop.

## Installation (Development)

```bash
# Prerequisites
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
pip install maturin

# Development build (editable install)
maturin develop

# Production build
maturin build --release

# Generate CI workflow
maturin generate-ci github > .github/workflows/CI.yml
```

## Sources

- [PyO3 documentation (v0.28.3)](https://docs.rs/pyo3/latest/pyo3/) -- HIGH confidence
- [PyO3 user guide](https://pyo3.rs/) -- HIGH confidence
- [maturin documentation (v1.13.1)](https://www.maturin.rs/) -- HIGH confidence
- [pyo3-async-runtimes (v0.28.0)](https://docs.rs/pyo3-async-runtimes/latest/pyo3_async_runtimes/) -- HIGH confidence
- [mlua documentation (v0.11.6)](https://docs.rs/mlua/latest/mlua/) -- HIGH confidence
- [Tokio (v1.51.1 LTS)](https://tokio.rs/) -- HIGH confidence
- [parking_lot (v0.12.5)](https://docs.rs/parking_lot/) -- HIGH confidence
- [bytes (v1.11.1)](https://docs.rs/bytes/) -- HIGH confidence
- [rmp-serde (v1.3.1)](https://docs.rs/rmp-serde/) -- HIGH confidence
- [serde (v1.0.228)](https://serde.rs/) -- HIGH confidence
- [thiserror (v2.0.18)](https://docs.rs/thiserror/) -- HIGH confidence
- [criterion (v0.8.2)](https://docs.rs/criterion/) -- HIGH confidence
- [maturin-action](https://github.com/PyO3/maturin-action) -- HIGH confidence
- [Bincode unmaintained (RUSTSEC-2025-0141)](https://lib.rs/crates/bincode) -- HIGH confidence
- [Redis Streams radix tree design](https://antirez.com/news/128) -- MEDIUM confidence (design reference only)

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

# Pitfalls Research

**Domain:** Embedded Redis-compatible database (Rust + PyO3 + Lua scripting)
**Researched:** 2026-04-10
**Confidence:** HIGH (most pitfalls verified across multiple sources including official PyO3 docs, Redis docs, and community issue trackers)

## Critical Pitfalls

### Pitfall 1: GIL Deadlocks When Mixing Rust Concurrency with Python Callbacks

**What goes wrong:**
Rust code acquires a Rust mutex (or other synchronization primitive), then calls back into Python (which requires the GIL). Meanwhile, another Python thread holds the GIL and is waiting to acquire the same Rust mutex. Classic ABBA deadlock. This is the single most common cause of hangs in PyO3-based libraries.

**Why it happens:**
Developers think in terms of Rust's ownership model and forget that the GIL is an implicit global lock. Any call into Python (including seemingly innocent operations like converting a Rust value to a Python object) requires the GIL. When Rust locks are held across GIL boundaries, deadlocks become likely.

**How to avoid:**
- Release the GIL (`Python::allow_threads`) before acquiring any Rust mutex or doing any blocking work.
- Never hold a Rust lock while calling into Python.
- Design the Rust core to be completely independent of Python -- all Python interaction happens at a thin boundary layer.
- Use `Py<T>` (GIL-independent) for data stored in Rust structs, not `Bound<'py, T>`.
- Consider `PyOnceLock` instead of `std::sync::OnceLock` when the initialization touches Python.

**Warning signs:**
- Tests hang intermittently (especially under load or with multiple async tasks).
- `Python::with_gil` calls inside `tokio::spawn` blocks.
- Rust `Mutex` or `RwLock` guards held across function boundaries that eventually call Python.

**Phase to address:**
Phase 1 (Core Architecture). The Rust-Python boundary design must be established correctly from day one. Retrofitting is extremely expensive because it affects every function signature.

---

### Pitfall 2: Redis Lua Data Type Conversion Fidelity

**What goes wrong:**
The Lua scripting engine does not faithfully reproduce Redis's exact type conversion rules between Lua and Redis types, causing Prefect's Lua scripts to return subtly wrong results. This is particularly insidious because scripts may appear to work in simple tests but fail on edge cases in production.

**Why it happens:**
Redis has very specific, sometimes counterintuitive conversion rules:
- Lua numbers are always converted to integers (3.14 becomes 3).
- Lua boolean `true` becomes integer `1`; `false` becomes nil/null (not `0`).
- Lua tables truncate at the first nil value (sparse arrays lose trailing elements).
- Tables with a single `ok` field become status replies; single `err` field become error replies.
- `redis.call()` raises Lua errors on Redis errors; `redis.pcall()` returns error tables.
- RESP2 vs RESP3 boolean handling differs.

Implementing "a Lua engine that can call Redis commands" is straightforward. Implementing one with bit-perfect type conversion fidelity is where teams spend months debugging.

**How to avoid:**
- Port Redis's actual conversion code (from `scripting.c`) as the reference implementation, not the documentation alone.
- Build a comprehensive conversion test suite covering every type boundary before implementing any Prefect Lua scripts.
- Test with Prefect's actual Lua scripts against real Redis and compare outputs byte-for-byte.
- Use Lua 5.1 specifically (not 5.4) -- Redis uses Lua 5.1, and the number handling differs between versions (5.3+ distinguishes integers and floats).

**Warning signs:**
- Prefect Lua scripts pass unit tests but fail integration tests.
- Sorted set scores come back as integers instead of floats (or vice versa).
- `redis.pcall()` errors not being caught properly in Lua scripts.
- Pipeline results differ between burner-redis and real Redis.

**Phase to address:**
Phase 2 (Lua Scripting Engine). Must be addressed before integrating with Prefect's Lua scripts. Build conversion tests first, engine second.

---

### Pitfall 3: Redis Streams Consumer Group State Machine Complexity

**What goes wrong:**
Consumer groups appear simple but have a complex internal state machine involving the Pending Entries List (PEL), consumer tracking, message claiming, and acknowledgment. An incomplete implementation causes message loss, duplicate delivery, or memory leaks from unacknowledged messages piling up.

**Why it happens:**
Developers implement the happy path (XADD, XREADGROUP, XACK) and miss the recovery paths that Prefect relies on:
- XAUTOCLAIM for reclaiming messages from dead consumers.
- The PEL must be a separate data structure tracking delivery count, delivery time, and consumer assignment per message.
- XREADGROUP with `>` (new messages) vs. `0` (pending re-delivery) have completely different semantics.
- XGROUP CREATE with MKSTREAM must create the stream if it does not exist.
- Consumer auto-creation on first XREADGROUP call.
- XINFO GROUPS and XINFO CONSUMERS must expose internal state accurately.

Prefect uses consumer groups as its core messaging system, including dead letter queue patterns. Half-implemented consumer groups will cause silent message loss.

**How to avoid:**
- Implement the full consumer group state machine, including PEL management, before exposing any stream commands.
- Build an integration test that runs Prefect's actual messaging code against burner-redis.
- Track delivery count per message in the PEL (required for XAUTOCLAIM's min-idle-time filtering).
- Implement XREADGROUP blocking semantics correctly (it must wake when new messages arrive via XADD).

**Warning signs:**
- Messages "disappear" -- delivered but never acknowledged, not reclaimable.
- XPENDING shows zero pending messages when there should be entries.
- XAUTOCLAIM returns empty results when stale messages exist.
- Memory grows continuously because PEL entries are never cleaned up.

**Phase to address:**
Phase 2/3 (Data Structures -- Streams). This is the most complex data structure in the project and should not be rushed. Allocate significant testing time.

---

### Pitfall 4: redis-py API Surface Mismatch

**What goes wrong:**
The Python API layer does not match `redis.asyncio.Redis` closely enough, and Prefect code that uses the library as a drop-in replacement hits `AttributeError`, unexpected return types, or behavioral differences that cause silent data corruption.

**Why it happens:**
`redis.asyncio.Redis` has many subtle API behaviors that are easy to miss:
- Pipeline methods (`.set()`, `.get()` etc.) are NOT coroutines and must NOT be awaited -- they return the Pipeline instance for chaining. Only `.execute()` is a coroutine.
- `.execute()` returns a list where exceptions are inline as values, not raised.
- `Lock` is a complex class with `acquire(blocking, blocking_timeout, token)`, `release()`, `extend(additional_time, replace_ttl)`, `reacquire()`, and async context manager support.
- Many commands have optional arguments that change return types (e.g., `SET` with `GET` flag returns the old value instead of `OK`).
- Return types must match exactly: bytes vs str, int vs float, None vs empty list.
- Connection pooling API surface may be referenced even though there is no actual connection.

**How to avoid:**
- Study `redis.asyncio.client.py` source code, not just documentation.
- Create a compatibility test suite that imports both `redis.asyncio.Redis` and `burner_redis.Redis`, runs the same operations, and asserts identical return values.
- Start with the exact Prefect usage patterns (grep the Prefect codebase for all redis method calls) rather than implementing the full API surface.
- Implement `__aenter__`/`__aexit__` on the client class (Prefect uses `async with` patterns).

**Warning signs:**
- `TypeError` or `AttributeError` when Prefect code runs against burner-redis.
- Tests pass with simple commands but fail when Prefect exercises the full API.
- Pipeline results come back in unexpected formats.
- Lock acquire/release raises unexpected exceptions.

**Phase to address:**
Phase 1 (Python API Layer). The API contract must be defined early by studying Prefect's actual usage, but expect ongoing refinement through every subsequent phase.

---

### Pitfall 5: Async Runtime Mismatch Between Rust (Tokio) and Python (asyncio)

**What goes wrong:**
The Rust side uses tokio for async operations (timers for key expiration, blocking XREADGROUP, background persistence). The Python side uses asyncio. Bridging these incorrectly causes hangs, event loop not running errors, or tasks that silently never complete.

**Why it happens:**
- `asyncio.get_running_loop()` fails when called from a Rust/tokio thread because tokio threads are not associated with a Python event loop.
- Python's asyncio requires control of the main thread for signal handling.
- `pyo3-asyncio` (now `pyo3-async-runtimes`) bridges the gap but requires careful lifecycle management.
- Background Rust tasks (key expiration sweeps, persistence timers) need to run on tokio without blocking the Python event loop, but must be able to signal Python when needed.

**How to avoid:**
- Use `pyo3-async-runtimes` with the tokio feature for bridging.
- Keep the tokio runtime as an internal implementation detail -- Python callers see only `async def` methods.
- Store `TaskLocals` to maintain the event loop reference across async boundaries.
- For background tasks (expiration, persistence), spawn on tokio and communicate with the Python side via callbacks or shared state, not by calling Python directly from the tokio task.
- Test with multiple concurrent Python async tasks to flush out race conditions early.

**Warning signs:**
- "no running event loop" errors from Rust code.
- Async methods that hang when called from Python.
- Background tasks (like key expiration) that stop firing after the first few runs.
- Tests pass individually but deadlock when run concurrently.

**Phase to address:**
Phase 1 (Core Architecture). The async bridging strategy must be decided and proven before building features on top of it.

---

### Pitfall 6: Key Expiration Timing Semantics

**What goes wrong:**
Keys with TTLs are not expired correctly -- either they linger past their expiration time (returning stale data), or the expiration sweep causes latency spikes that block other operations. Both break Prefect's lock and lease semantics, which depend on precise TTL behavior.

**Why it happens:**
Redis uses a hybrid lazy + active expiration strategy:
- **Lazy:** Check TTL on every access, delete if expired.
- **Active:** Background sweep samples keys and deletes expired ones.

Implementing only lazy expiration means expired keys sit in memory indefinitely if never accessed. Implementing only active expiration with aggressive sweeps causes latency spikes. Getting the balance wrong either wastes memory or causes jitter.

For an embedded, in-process database, the active expiration sweep must not starve the Python event loop of CPU time.

**How to avoid:**
- Implement both lazy and active expiration from the start.
- Use a sorted structure (e.g., BTreeMap keyed by expiration timestamp) for efficient active expiration rather than random sampling -- this is simpler and more deterministic for an embedded database.
- Run the active expiration sweep on the tokio runtime as a periodic task, not on the Python thread.
- Cap the number of keys expired per sweep cycle to bound latency.
- Ensure TTL checks happen in GET, EXISTS, and any command that reads keys.

**Warning signs:**
- `GET` returns data for a key that should have expired.
- Memory usage grows continuously even with heavy TTL usage.
- Periodic latency spikes correlated with expiration sweep timing.
- Prefect locks do not expire when they should, causing deadlocks.

**Phase to address:**
Phase 2 (Core Engine Features). Must be implemented correctly before Lock/lease support, since locks depend on TTL for safety.

---

### Pitfall 7: Persistence and Crash Recovery Data Corruption

**What goes wrong:**
The flush-to-disk feature produces corrupt or incomplete files when the process crashes mid-write, or the reload-from-disk feature silently loads partial data, leading to inconsistent state that causes Prefect to behave unpredictably.

**Why it happens:**
- Writing a snapshot directly to the target file means a crash mid-write leaves a corrupt file.
- Not using fsync means data sits in OS page cache and is lost on power failure.
- Serialization format changes between versions break reload.
- Loading a snapshot does not validate checksums, so bit-rot or partial writes go undetected.

**How to avoid:**
- Write-then-rename pattern: write to a temporary file, fsync, then atomically rename to the target path.
- Include a checksum (CRC32 or xxHash) in the persistence file header and validate on load.
- Version the persistence format from day one with a magic number and format version in the header.
- On reload failure, log the error clearly and start with an empty database rather than silently using corrupt data.
- Test crash recovery by killing the process mid-persistence and verifying the previous good snapshot survives.

**Warning signs:**
- Reload after crash silently loses data.
- Persistence file grows unexpectedly or is truncated.
- No error reported when loading a corrupt file.
- Different versions of the library cannot read each other's persistence files.

**Phase to address:**
Phase 3/4 (Persistence). The write-then-rename pattern and checksum validation must be in the initial persistence implementation, not added later.

---

## Technical Debt Patterns

| Shortcut | Immediate Benefit | Long-term Cost | When Acceptable |
|----------|-------------------|----------------|-----------------|
| Skip .pyi stub generation | Faster initial development | Users get no IDE autocompletion or type checking; impossible to use with mypy/pyright | Never in a library meant for external use. Generate stubs from Phase 1. |
| Clone data on every Python-Rust boundary crossing | Avoids lifetime complexity | Excessive memory copies for large values (e.g., XRANGE on large streams) | Acceptable for MVP; optimize hot paths later with zero-copy where possible |
| Single global lock for all data structures | Simple concurrency model | Serializes all operations; becomes bottleneck under concurrent async tasks | Acceptable for Phase 1-2 if designed to be replaceable with per-key or sharded locking |
| Hardcode Lua 5.1 without sandboxing | Simpler Lua integration | Untrusted scripts could consume unbounded memory/CPU | Acceptable for now (scripts come from Prefect, not users), but document the limitation |
| Skip RESP protocol entirely (in-process only) | No protocol parsing overhead | Cannot add network mode later without major rework | Acceptable -- project explicitly scopes out network mode |
| Implement commands one-at-a-time without a command dispatch framework | Quick to get first commands working | Adding 20th command requires touching the same massive match block; error handling inconsistent | Never. Build a command dispatch trait/table from the start. |

## Integration Gotchas

| Integration | Common Mistake | Correct Approach |
|-------------|----------------|------------------|
| Prefect `redis.asyncio.Redis` | Implementing `async def set(...)` that must be awaited, when Pipeline's `.set()` must NOT be awaited | Pipeline methods return `self` synchronously; only `.execute()` is async. Use separate Pipeline class. |
| Prefect Lua scripts | Assuming scripts use only KEYS and ARGV | Prefect scripts call `redis.call()` with commands that themselves have complex return types. Test with actual Prefect scripts. |
| Prefect Lock/AsyncLock | Implementing basic acquire/release only | Prefect uses `blocking_timeout`, `token`-based ownership, `extend()`, `reacquire()`, and `raise_on_release_error` context manager semantics. |
| Prefect consumer groups | Creating consumer group on existing stream only | Prefect expects XGROUP CREATE with MKSTREAM to create the stream if it does not exist. |
| Python garbage collector | Holding Rust references to Python objects without preventing GC | Use `Py<T>` to prevent premature garbage collection of Python objects referenced from Rust. |
| maturin/PyPI wheels | Building on developer machine and uploading | Wheels must be built in manylinux containers (or with zig linker) to be PyPI-compatible. Use `maturin-action` in CI. |

## Performance Traps

| Trap | Symptoms | Prevention | When It Breaks |
|------|----------|------------|----------------|
| Acquiring/releasing GIL per Redis command | Each command pays ~1-5us GIL overhead; throughput drops under pipeline workloads | Batch GIL acquisition: take GIL once, process entire pipeline, release | Noticeable at >1000 commands/sec in pipeline |
| Full data clone on every GET/SET | Memory allocations dominate for large values | Use `PyBytes::new()` with direct buffer access; avoid intermediate String conversions | When values exceed ~1KB regularly |
| Linear scan for key expiration | Expiration sweep time grows with total key count | Use sorted expiration index (BTreeMap by timestamp) | At >10,000 keys with TTLs |
| Unbounded PEL growth in streams | Memory grows, XPENDING/XAUTOCLAIM slow down | Implement max PEL size warnings; ensure XACK properly cleans PEL | When consumers crash and messages are never acknowledged |
| Debug-mode Rust builds in development | 10-20x slower than release; misleading perf conclusions | Always benchmark with `maturin develop --release` | Immediately -- debug mode is too slow to be representative |
| Lua script compilation on every EVAL | Parsing + compilation overhead per call | Cache compiled scripts by SHA1 hash (EVALSHA pattern); compile once, execute many | When Prefect calls the same script repeatedly (which it does) |

## Security Mistakes

| Mistake | Risk | Prevention |
|---------|------|------------|
| No memory limit on Lua script execution | A script with an infinite loop or exponential allocation consumes all host memory and crashes the Prefect server | Set instruction count limits in mlua; set memory allocation limits via Lua allocator hooks |
| Persistence file contains raw key/value data without access control | Anyone with filesystem access can read all Prefect state | Document that persistence files should have restricted permissions (0600); consider optional encryption later |
| Lua sandbox escape via `debug` library | Scripts could inspect/modify Rust-side state | Disable `debug`, `os`, `io`, and `loadfile` libraries in the embedded Lua environment |
| Accepting arbitrary Lua scripts without validation | Malicious or buggy scripts block the entire engine (Lua runs atomically) | Implement script timeout; for burner-redis this is lower risk since scripts come from Prefect, but still set a timeout ceiling |

## UX Pitfalls

| Pitfall | User Impact | Better Approach |
|---------|-------------|-----------------|
| No clear error when a Redis command is used that is not implemented | User gets a cryptic Rust panic or generic error | Return a clear error: "Command SUBSCRIBE is not supported by burner-redis. See [docs] for supported commands." |
| Silent fallback to empty database when persistence file is corrupt | User loses all state with no indication | Log a clear warning and raise an error (or provide a `strict_load=True` option) |
| No way to inspect database state for debugging | Users cannot tell if data is present or what state the engine is in | Implement INFO, DBSIZE, and DEBUG-friendly commands even if not needed by Prefect |
| API differences discovered only at runtime | Prefect crashes after startup with AttributeError | Provide a compatibility check function: `burner_redis.check_compatibility()` that reports missing methods |
| Confusing error messages from Lua script failures | User sees "Lua error" with no context | Include the script source, line number, and the Redis command that failed in error messages |

## "Looks Done But Isn't" Checklist

- [ ] **SET command:** Often missing NX/XX/GET/EX/PX/EXAT/PXAT/KEEPTTL flags -- verify all flag combinations work, especially SET with GET returning the old value
- [ ] **Pipeline:** Often missing error-as-value semantics in execute() results -- verify that a failed command in a pipeline does not abort the pipeline but places the exception in the result list
- [ ] **XREADGROUP:** Often missing the distinction between `>` (new messages) and `0`/specific-ID (pending re-delivery) -- verify both paths with consumer group state
- [ ] **XAUTOCLAIM:** Often missing delivery count tracking and min-idle-time filtering -- verify that claimed messages have their idle time and delivery count updated
- [ ] **Lua EVAL:** Often missing `redis.error_reply()` and `redis.status_reply()` helper functions -- verify these are available in the Lua environment
- [ ] **Key expiration:** Often missing expiration check on write commands (SET on an expired key should not return old value) -- verify expired keys are invisible to all commands
- [ ] **Lock:** Often missing token-based ownership verification on release -- verify that only the lock owner can release it (prevents releasing someone else's lock after timeout)
- [ ] **ZADD:** Often missing NX/XX/GT/LT/CH flags -- verify all flag combinations, especially CH (changed) which modifies the return value semantics
- [ ] **Persistence:** Often missing atomic write (write-then-rename) -- verify that killing the process mid-save does not corrupt the existing save file
- [ ] **Type errors:** Often missing type checking on commands -- verify that running HGET on a string key returns a WRONGTYPE error, not a crash

## Recovery Strategies

| Pitfall | Recovery Cost | Recovery Steps |
|---------|---------------|----------------|
| GIL deadlock in production | MEDIUM | Identify the deadlock pattern via thread dump; restructure the specific Rust-Python boundary; usually a localized fix once identified |
| Lua type conversion bugs | HIGH | Requires building a comprehensive conversion test suite; may require re-examining all Lua script interactions; can cascade into data corruption |
| Incomplete consumer group state | HIGH | Requires redesigning the PEL data structure; may require a persistence format migration if PEL state was being persisted incorrectly |
| API surface mismatch | LOW | Add missing methods/flags incrementally; each fix is usually isolated to one command |
| Expiration timing bugs | MEDIUM | Fix the expiration engine; but data that should have expired and was returned to Prefect may have caused incorrect behavior already |
| Persistence corruption | HIGH | If no valid backup exists, data is lost; must implement write-then-rename retroactively and hope users have not lost data |
| Cross-platform wheel failures | LOW | Fix CI configuration; use maturin-action with proper target matrix; rebuild and republish |

## Pitfall-to-Phase Mapping

| Pitfall | Prevention Phase | Verification |
|---------|------------------|--------------|
| GIL deadlock | Phase 1 (Core Architecture) | Run concurrent async stress tests; verify no `Python::with_gil` inside Rust lock guards |
| Lua type conversion | Phase 2 (Lua Engine) | Byte-for-byte comparison test suite against real Redis for all type conversions |
| Consumer group state | Phase 2-3 (Streams) | Run Prefect's actual messaging integration tests against burner-redis |
| API surface mismatch | Phase 1 (API Layer), ongoing | Compatibility test that imports both redis.asyncio.Redis and burner_redis, runs identical operations |
| Async runtime mismatch | Phase 1 (Core Architecture) | Test async methods from multiple concurrent Python tasks; verify no event loop errors |
| Key expiration | Phase 2 (Core Engine) | Time-based tests that SET with TTL, wait, verify GET returns None; verify memory reclamation |
| Persistence corruption | Phase 3-4 (Persistence) | Crash injection test: kill process mid-save, verify recovery uses last good snapshot |
| Cross-platform wheels | Phase 4 (Distribution) | CI matrix building wheels for manylinux (x86_64, aarch64), macOS (x86_64, arm64), Windows (x86_64) |
| Command dispatch sprawl | Phase 1 (Core Architecture) | Review command dispatch design before second command is implemented; must use trait/table pattern |
| Lua sandbox escape | Phase 2 (Lua Engine) | Verify debug/os/io/loadfile libraries are disabled; test with adversarial scripts |

## Sources

- [PyO3 FAQ & Troubleshooting](https://pyo3.rs/v0.23.4/faq.html) -- GIL deadlock patterns, memory management
- [PyO3 Memory Management Guide](https://pyo3.rs/v0.22.5/memory) -- Bound API, GILPool behavior
- [PyO3 GIL Deadlock Discussion #3045](https://github.com/PyO3/pyo3/discussions/3045) -- tokio::spawn + GIL deadlocks
- [PyO3 GIL Deadlock Discussion #3089](https://github.com/PyO3/pyo3/discussions/3089) -- with_gil deadlocks in multithreaded environments
- [PyO3 Memory Issue #319](https://github.com/PyO3/pyo3/issues/319) -- Memory growth without GIL release
- [pyo3-async-runtimes](https://github.com/PyO3/pyo3-async-runtimes) -- Async bridging between Rust and Python
- [Redis Lua API Reference](https://redis.io/docs/latest/develop/programmability/lua-api/) -- Type conversion rules, redis.call/pcall semantics
- [Redis Lua Scripting Guide](https://redis.io/docs/latest/develop/programmability/eval-intro/) -- EVAL/EVALSHA, atomicity guarantees, script caching
- [Redis XREADGROUP Documentation](https://redis.io/docs/latest/commands/xreadgroup/) -- PEL, consumer group semantics
- [Redis XAUTOCLAIM Documentation](https://redis.io/docs/latest/commands/xautoclaim/) -- Message claiming, idle time tracking
- [Redis Streams Documentation](https://redis.io/docs/latest/develop/data-types/streams/) -- Consumer groups, dead letter patterns
- [Redis Key Expiration Internals](https://www.pankajtanwar.in/blog/how-redis-expires-keys-a-deep-dive-into-how-ttl-works-internally-in-redis) -- Lazy + active expiration hybrid
- [Redis Expiration Algorithm FAQ](https://redis.io/faq/doc/1fqjridk8w/what-are-the-impacts-of-the-redis-expiration-algorithm) -- Redis 6.0 radix tree approach
- [redis-py Async Pipeline Source](https://github.com/redis/redis-py/blob/master/redis/asyncio/client.py) -- Pipeline method behavior
- [redis-py Pipeline Type Discussion](https://github.com/python/typeshed/issues/8324) -- Pipeline methods are not coroutines
- [Maturin Distribution Guide](https://www.maturin.rs/distribution.html) -- manylinux compliance, cross-platform builds
- [mlua GitHub](https://github.com/mlua-rs/mlua) -- Lua 5.1/5.4 embedding in Rust, Send constraints
- [rlua FAQ](https://github.com/mlua-rs/rlua/blob/master/FAQ.md) -- Lifetime management, scope constraints, safety

---
*Pitfalls research for: Embedded Redis-compatible database (Rust + PyO3 + Lua)*
*Researched: 2026-04-10*