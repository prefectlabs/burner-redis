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
