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
