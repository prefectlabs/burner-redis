<!-- GSD:project-start source:PROJECT.md -->
## Project

**Burner Redis**

An embedded, in-process Redis-compatible database written in Rust with Python bindings. It provides a drop-in replacement for `redis.asyncio.Redis` that runs inside the host process with no external server needed. The primary use case is backing a self-hosted Prefect server without requiring a separate Redis deployment.

**Core Value:** A self-hosted Prefect server can start, run flows, and manage state using this library instead of an external Redis server — zero infrastructure, zero configuration.

### Constraints

- **Language**: Rust core with Python bindings via PyO3/maturin
- **Python API**: Must be compatible with `redis.asyncio.Redis` interface (drop-in replacement)
- **Async**: Must support Python async/await — Prefect is async-first
- **Distribution**: PyPI package with pre-built wheels (manylinux, macOS, Windows)
- **Persistence format**: Custom (no need for RDB/AOF compatibility)
- **Lua engine**: Embedded Lua interpreter in Rust for EVAL/EVALSHA support
<!-- GSD:project-end -->

<!-- GSD:stack-start source:research/STACK.md -->
## Technology Stack

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
- `rlua` is the predecessor to mlua, now deprecated in favor of mlua by the same maintainer organization.
- `hlua` is unmaintained.
- mlua is the only actively maintained Rust Lua binding with comprehensive feature support.
### Concurrency & Synchronization
| Technology | Version | Purpose | Why | Confidence |
|------------|---------|---------|-----|------------|
| [parking_lot](https://github.com/Amanieu/parking_lot) | 0.12.x (latest: 0.12.5) | RwLock / Mutex | 1.5x faster uncontended, up to 5x faster under contention vs std. Smaller (1 byte for Mutex). Use `RwLock` for the main keyspace -- reads are far more common than writes. | HIGH |
| [bytes](https://docs.rs/bytes/) | 1.11.x (latest: 1.11.1) | Byte buffer type | Reference-counted, zero-copy byte slices. Avoid cloning string data throughout the engine. Part of the Tokio ecosystem, so it integrates cleanly. | HIGH |
### Serialization (Persistence)
| Technology | Version | Purpose | Why | Confidence |
|------------|---------|---------|-----|------------|
| [serde](https://serde.rs/) | 1.0.x (latest: 1.0.228) | Serialization framework | The standard. Derive macros for all data structures. | HIGH |
| [rmp-serde](https://github.com/3Hren/msgpack-rust) | 1.3.x (latest: 1.3.1) | MessagePack format for persistence | Compact binary format, ~70% the size of bincode with only ~1.5x overhead. Cross-language readable (useful for debugging persistence files). Self-describing format means forward compatibility when data structures evolve. | HIGH |
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
## Key Cargo.toml Configuration
## Key pyproject.toml Configuration
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
## Installation (Development)
# Prerequisites
# Development build (editable install)
# Production build
# Generate CI workflow
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
<!-- GSD:stack-end -->

<!-- GSD:conventions-start source:CONVENTIONS.md -->
## Conventions

Conventions not yet established. Will populate as patterns emerge during development.
<!-- GSD:conventions-end -->

<!-- GSD:architecture-start source:ARCHITECTURE.md -->
## Architecture

Architecture not yet mapped. Follow existing patterns found in the codebase.
<!-- GSD:architecture-end -->

<!-- GSD:skills-start source:skills/ -->
## Project Skills

No project skills found. Add skills to any of: `.claude/skills/`, `.agents/skills/`, `.cursor/skills/`, or `.github/skills/` with a `SKILL.md` index file.
<!-- GSD:skills-end -->

<!-- GSD:workflow-start source:GSD defaults -->
## GSD Workflow Enforcement

Before using Edit, Write, or other file-changing tools, start work through a GSD command so planning artifacts and execution context stay in sync.

Use these entry points:
- `/gsd-quick` for small fixes, doc updates, and ad-hoc tasks
- `/gsd-debug` for investigation and bug fixing
- `/gsd-execute-phase` for planned phase work

Do not make direct repo edits outside a GSD workflow unless the user explicitly asks to bypass it.
<!-- GSD:workflow-end -->



<!-- GSD:profile-start -->
## Developer Profile

> Profile not yet configured. Run `/gsd-profile-user` to generate your developer profile.
> This section is managed by `generate-claude-profile` -- do not edit manually.
<!-- GSD:profile-end -->
