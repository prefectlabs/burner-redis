# Phase 1: Foundation and String Commands - Context

**Gathered:** 2026-04-10
**Status:** Ready for planning

<domain>
## Phase Boundary

Deliver a Python package (`burner_redis`) backed by a Rust core via PyO3/maturin that can execute string commands (SET with NX/XX/EX/PX flags, GET, DELETE, EXISTS) through an async API matching `redis.asyncio.Redis` method signatures.

</domain>

<decisions>
## Implementation Decisions

### API Surface Design
- Constructor with no required args — `BurnerRedis()` creates a ready-to-use instance. Optional kwargs for persistence path etc in later phases.
- Method signatures exactly match `redis.asyncio.Redis` (same param names, defaults, return types) so Prefect code works unmodified.
- GET returns `None` for missing keys (matching redis-py `Optional[bytes]` behavior).
- SET returns `True` on success, `None` when NX/XX condition fails — matches redis-py's `ResponseT`.

### Error Handling & Edge Cases
- Mirror redis-py exceptions — `redis.exceptions.ResponseError` for wrong-type operations. `ConnectionError` never raised (in-process).
- Wrong-type operations raise `ResponseError("WRONGTYPE Operation against a key holding the wrong kind of value")` — matches Redis behavior exactly.
- `delete(*keys)` and `exists(*keys)` accept variadic keys with integer return counts, matching redis-py.
- Accept both `str` and `bytes` for keys/values, auto-encode str to UTF-8 bytes — matches redis-py's `decode_responses=False` default.

### Project Structure & Testing
- Single crate with modules: `src/lib.rs` (PyO3 entry), `src/store.rs` (key-value engine), `src/commands/` (command implementations).
- `tests/` directory with pytest, one test file per command group (e.g., `test_strings.py`), async tests via `pytest-asyncio`.
- Both Rust `#[test]` for the store/data layer and Python pytest for the API surface.
- Package name: `burner_redis` (underscore) for import, `burner-redis` (hyphen) for PyPI.

### Claude's Discretion
No items deferred to Claude's discretion — all questions resolved.

</decisions>

<code_context>
## Existing Code Insights

### Reusable Assets
- No existing code — greenfield project. CLAUDE.md contains full technology stack specification.

### Established Patterns
- Technology stack defined in CLAUDE.md: PyO3 0.28.x, maturin 1.13.x, pyo3-async-runtimes 0.28.0, Tokio 1.51.x, parking_lot 0.12.x, bytes 1.11.x, thiserror 2.0.x
- Architecture decision: Use current-thread Tokio runtime (not multi-threaded) since we run inside a Python process.
- Key type: `bytes::Bytes` for keys and values throughout.
- Top-level keyspace: `HashMap<Bytes, RedisValue>` with `parking_lot::RwLock`.

### Integration Points
- PyO3 `#[pyclass]` / `#[pymethods]` for the `BurnerRedis` class
- `pyo3-async-runtimes` `future_into_py()` to expose Rust futures as Python awaitables
- maturin build system with `abi3-py39` feature for single-wheel-per-platform builds

</code_context>

<specifics>
## Specific Ideas

No specific requirements — open to standard approaches following the technology stack in CLAUDE.md.

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope.

</deferred>
