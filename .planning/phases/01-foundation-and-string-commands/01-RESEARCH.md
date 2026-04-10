# Phase 1: Foundation and String Commands - Research

**Researched:** 2026-04-10
**Domain:** Rust/PyO3 embedded Redis engine with Python async bindings
**Confidence:** HIGH

## Summary

This phase bootstraps a greenfield Rust+Python project: a `burner_redis` PyPI package backed by a Rust crate compiled via maturin/PyO3. The deliverable is a `BurnerRedis` Python class with async `set()`, `get()`, `delete()`, and `exists()` methods matching `redis.asyncio.Redis` signatures exactly.

The primary technical challenges are: (1) correctly bridging Rust futures to Python awaitables via `pyo3-async-runtimes` with `future_into_py`, (2) handling the `Send + 'static` constraint that prevents borrowing `&self` across async boundaries (solved by cloning data out of self before the async block), and (3) storing TTL metadata at SET time so expiration is honored on GET (passive expiration only; active sweep is Phase 4).

**Primary recommendation:** Start with the maturin project scaffold (`maturin init --bindings pyo3`), configure the Tokio current-thread runtime in the `#[pymodule]` init function, implement the in-memory store as `RwLock<HashMap<Bytes, ValueEntry>>` where `ValueEntry` holds both value and optional expiration instant, then build the async Python methods one at a time with pytest verification.

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

**API Surface Design:**
- Constructor with no required args -- `BurnerRedis()` creates a ready-to-use instance. Optional kwargs for persistence path etc in later phases.
- Method signatures exactly match `redis.asyncio.Redis` (same param names, defaults, return types) so Prefect code works unmodified.
- GET returns `None` for missing keys (matching redis-py `Optional[bytes]` behavior).
- SET returns `True` on success, `None` when NX/XX condition fails -- matches redis-py's `ResponseT`.

**Error Handling & Edge Cases:**
- Mirror redis-py exceptions -- `redis.exceptions.ResponseError` for wrong-type operations. `ConnectionError` never raised (in-process).
- Wrong-type operations raise `ResponseError("WRONGTYPE Operation against a key holding the wrong kind of value")` -- matches Redis behavior exactly.
- `delete(*keys)` and `exists(*keys)` accept variadic keys with integer return counts, matching redis-py.
- Accept both `str` and `bytes` for keys/values, auto-encode str to UTF-8 bytes -- matches redis-py's `decode_responses=False` default.

**Project Structure & Testing:**
- Single crate with modules: `src/lib.rs` (PyO3 entry), `src/store.rs` (key-value engine), `src/commands/` (command implementations).
- `tests/` directory with pytest, one test file per command group (e.g., `test_strings.py`), async tests via `pytest-asyncio`.
- Both Rust `#[test]` for the store/data layer and Python pytest for the API surface.
- Package name: `burner_redis` (underscore) for import, `burner-redis` (hyphen) for PyPI.

### Claude's Discretion
No items deferred to Claude's discretion -- all questions resolved.

### Deferred Ideas (OUT OF SCOPE)
None -- discussion stayed within phase scope.

</user_constraints>

<phase_requirements>

## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| FOUND-01 | Rust core library with PyO3 bindings compiles and is importable from Python | Maturin scaffold + PyO3 `#[pymodule]` + `abi3-py39` feature; Cargo.toml and pyproject.toml templates provided |
| FOUND-02 | Python class implements `redis.asyncio.Redis`-compatible method signatures | Exact redis-py method signatures extracted from source; `#[pyclass]` + `#[pymethods]` patterns documented |
| FOUND-03 | All command methods are async-compatible (awaitable from Python) | `pyo3-async-runtimes::tokio::future_into_py` pattern with `Send + 'static` workarounds documented |
| STR-01 | User can SET a key with a string value | Store implementation pattern with `HashMap<Bytes, ValueEntry>` |
| STR-02 | SET supports NX (only if not exists) and XX (only if exists) flags | Conditional logic documented; returns `None` on condition failure per redis-py |
| STR-03 | SET supports EX (seconds) and PX (milliseconds) expiration flags | `Instant::now() + Duration` stored in `ValueEntry.expires_at`; passive check on GET |
| STR-04 | User can GET a key's value (returns bytes or None) | Returns `None` for missing OR expired keys; `Option<PyObject>` return type |
| STR-05 | User can DELETE one or more keys | Variadic `*names` pattern via `#[pyo3(signature = (*names))]`; returns count as `i64` |
| STR-06 | User can check if a key EXISTS | Same variadic pattern as DELETE; returns count of existing (non-expired) keys |

</phase_requirements>

## Standard Stack

### Core (Phase 1 subset)

| Library | Version | Purpose | Why Standard | Verified |
|---------|---------|---------|--------------|----------|
| pyo3 | 0.28.3 | Rust-Python bindings | De facto standard; supports `#[pyclass]`, `#[pymethods]`, abi3 | [VERIFIED: `cargo search pyo3` returned 0.28.3] |
| pyo3-async-runtimes | 0.28.0 | Async bridge (future_into_py) | Official PyO3 companion; bridges Tokio futures to Python awaitables | [VERIFIED: `cargo search pyo3-async-runtimes` returned 0.28.0] |
| tokio | 1.51.1 | Async runtime | Required by pyo3-async-runtimes; use current-thread variant | [VERIFIED: `cargo search tokio` returned 1.51.1] |
| parking_lot | 0.12.5 | RwLock for keyspace | 1.5-5x faster than std; `RwLock<HashMap>` for the store | [VERIFIED: `cargo search parking_lot` returned 0.12.5] |
| bytes | 1.11.1 | Zero-copy byte buffers | Ref-counted, Tokio-ecosystem native; avoids cloning key/value data | [VERIFIED: `cargo search bytes` returned 1.11.1] |
| thiserror | 2.0.18 | Error type derivation | Derive `Error` for Rust-side error hierarchy | [VERIFIED: `cargo search thiserror` returned 2.0.18] |
| maturin | 1.13.1 | Build tool | Standard for PyO3 projects; handles wheels and cross-compilation | [VERIFIED: pip dry-run showed 1.13.1] |

### Python Testing

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| pytest | latest | Test framework | All Python-side integration tests | [ASSUMED] |
| pytest-asyncio | latest | Async test support | All async method tests; use `asyncio_mode = "auto"` | [ASSUMED] |

**Installation:**
```bash
# Rust dependencies (Cargo.toml -- see Architecture section)
# No separate install needed; cargo fetches on build

# Python build tool
pip install maturin

# Python test dependencies
pip install pytest pytest-asyncio

# Development build
maturin develop
```

## Architecture Patterns

### Recommended Project Structure

```
burner-redis/
├── Cargo.toml              # Rust crate config
├── pyproject.toml           # Python package config (maturin backend)
├── src/
│   ├── lib.rs               # PyO3 module entry + BurnerRedis #[pyclass]
│   ├── store.rs             # In-memory key-value engine (pure Rust, no PyO3)
│   └── commands/
│       ├── mod.rs           # Command module declarations
│       └── strings.rs       # SET/GET/DELETE/EXISTS implementations
├── tests/
│   ├── conftest.py          # Shared fixtures (BurnerRedis instance)
│   └── test_strings.py      # Python async tests for string commands
└── python/
    └── burner_redis/
        └── __init__.py      # Re-export + type stubs (optional in Phase 1)
```

### Pattern 1: Store with Passive Expiration

**What:** The in-memory store holds values alongside optional expiration timestamps. On every read, check if the entry has expired; if so, remove it and return `None`.

**When to use:** Phase 1 (SET with EX/PX). Active expiration sweep comes in Phase 4.

```rust
// Source: Design pattern for Phase 1 store
use bytes::Bytes;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct ValueEntry {
    pub data: Bytes,
    pub expires_at: Option<Instant>,
}

impl ValueEntry {
    pub fn is_expired(&self) -> bool {
        self.expires_at
            .map(|exp| Instant::now() >= exp)
            .unwrap_or(false)
    }
}

pub struct Store {
    data: RwLock<HashMap<Bytes, ValueEntry>>,
}
```

**Key insight:** Use `std::time::Instant` (not `tokio::time::Instant`) for expiration timestamps. `std::time::Instant` is `Send + Sync + 'static` and works outside of a Tokio runtime context, which matters because the store is accessed from both sync Rust tests and async Python methods. [ASSUMED]

### Pattern 2: Async Method via future_into_py with Clone

**What:** Since `future_into_py` requires `Send + 'static`, you cannot borrow `&self` across the async boundary. Clone the data you need (or wrap shared state in `Arc`) before entering the async block.

**When to use:** Every `#[pymethods]` async method on `BurnerRedis`.

```rust
// Source: PyO3/pyo3-async-runtimes README + issue #50 on pyo3-asyncio
// Adapted for 0.28 API (Bound<'_, PyAny> return type)
use pyo3::prelude::*;
use std::sync::Arc;

#[pyclass]
struct BurnerRedis {
    store: Arc<Store>,  // Arc allows cloning into async blocks
}

#[pymethods]
impl BurnerRedis {
    #[new]
    fn new() -> Self {
        BurnerRedis {
            store: Arc::new(Store::new()),
        }
    }

    fn get<'py>(&self, py: Python<'py>, name: &[u8]) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();  // Clone Arc, not the store
        let key = Bytes::copy_from_slice(name);
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            Ok(store.get(&key))  // Returns Option<Bytes>
        })
    }
}
```

**Critical constraint:** The future passed to `future_into_py` must be `Send + 'static`. The `Arc<Store>` clone satisfies `'static`; the `parking_lot::RwLock` inside `Store` satisfies `Send`. [VERIFIED: pyo3-async-runtimes docs -- `F: Future<Output = PyResult<T>> + Send + 'static`]

### Pattern 3: Key/Value Type Acceptance (str and bytes)

**What:** Redis-py accepts both `str` and `bytes` for keys and values. PyO3 can extract either via `&[u8]` (for bytes) or by accepting `PyObject` and manually checking type.

**When to use:** All methods that accept key or value parameters.

```rust
// Source: PyO3 docs on extracting bytes
// Accept both str and bytes by extracting as bytes
fn extract_key(obj: &Bound<'_, PyAny>) -> PyResult<Bytes> {
    if let Ok(s) = obj.extract::<&str>() {
        Ok(Bytes::from(s.as_bytes().to_vec()))
    } else if let Ok(b) = obj.extract::<&[u8]>() {
        Ok(Bytes::copy_from_slice(b))
    } else {
        Err(PyTypeError::new_err("expected str or bytes"))
    }
}
```

### Pattern 4: Tokio Current-Thread Runtime Initialization

**What:** `pyo3-async-runtimes` defaults to a **multi-thread** Tokio runtime. CLAUDE.md specifies current-thread. Must call `init()` before any `future_into_py` call.

**When to use:** Module initialization in `lib.rs`.

```rust
// Source: pyo3-async-runtimes source code (tokio.rs lines 159-179)
// CRITICAL: Default is multi_thread(). Must override to current_thread().
#[pymodule]
fn _burner_redis(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Initialize Tokio with current-thread runtime
    pyo3_async_runtimes::tokio::init(
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
    );

    m.add_class::<BurnerRedis>()?;
    Ok(())
}
```

**Why this matters:** The default `multi_thread()` builder spawns OS threads that compete with Python's GIL. Current-thread avoids this overhead for an in-process database. [VERIFIED: pyo3-async-runtimes source -- `static TOKIO_BUILDER: Lazy<Mutex<Builder>> = Lazy::new(|| Mutex::new(multi_thread()));`]

### Pattern 5: redis-py SET Method Signature Match

**What:** The `set()` method in redis-py has many parameters. Phase 1 must match the subset we support.

**When to use:** Implementing the `set` method on `BurnerRedis`.

```rust
// Source: redis-py core.py lines 4120-4136 (GitHub redis/redis-py master)
// Phase 1 subset of parameters:
#[pymethods]
impl BurnerRedis {
    #[pyo3(signature = (name, value, ex=None, px=None, nx=false, xx=false))]
    fn set<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,   // KeyT = str | bytes | memoryview
        value: &Bound<'py, PyAny>,  // EncodableT = str | bytes | int | float
        ex: Option<u64>,            // ExpiryT = int | timedelta (simplify to int for Phase 1)
        px: Option<u64>,            // ExpiryT = int | timedelta
        nx: bool,                   // Only set if not exists
        xx: bool,                   // Only set if exists
    ) -> PyResult<Bound<'py, PyAny>> {
        // Clone Arc<Store>, extract key/value as Bytes, then future_into_py
        // Returns: True on success, None when NX/XX fails
        todo!()
    }
}
```

**redis-py exact signatures (Phase 1 relevant):**
```python
# Source: redis-py core.py (GitHub redis/redis-py master, lines 4120-4230)
# [VERIFIED: fetched from GitHub API]

# SET - returns bool | None (True=success, None=NX/XX fail)
async def set(self, name, value, ex=None, px=None, nx=False, xx=False,
              keepttl=False, get=False, exat=None, pxat=None) -> bool | None

# GET - returns bytes | None
async def get(self, name) -> bytes | None

# DELETE - returns int (count of deleted keys)
async def delete(self, *names) -> int

# EXISTS - returns int (count of existing keys)
async def exists(self, *names) -> int
```

### Anti-Patterns to Avoid

- **Borrowing `&self` in async blocks:** `future_into_py` requires `'static`; `&self` is tied to GIL borrow lifetime. Always clone `Arc<Store>` before the async block. [VERIFIED: pyo3-async-runtimes docs]
- **Using multi-thread Tokio runtime:** Competes with Python GIL; causes unnecessary context switching. Use `current_thread`. [CITED: CLAUDE.md Architecture Decision]
- **Storing `tokio::time::Instant` in the data store:** Not `Send` across runtime boundaries in some configurations. Use `std::time::Instant`. [ASSUMED]
- **Returning Python objects from inside the async block directly:** Must return Rust types that implement `IntoPyObject`. Return `Option<Vec<u8>>` or `bool`, not `PyObject`. [VERIFIED: pyo3-async-runtimes -- `T: for<'py> IntoPyObject<'py> + Send + 'static`]
- **Hand-rolling `ResponseError` exception class:** Import from `redis.exceptions` at runtime via PyO3's `py.import()`. This ensures isinstance checks work. [ASSUMED -- needs validation]

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Python async bridge | Custom coroutine wrapping | `pyo3_async_runtimes::tokio::future_into_py` | Handles event loop integration, cancellation, contextvars |
| Build system | Makefile + setuptools | `maturin` | Handles manylinux, abi3, cross-compilation, wheel building |
| Byte buffer management | `Vec<u8>` everywhere | `bytes::Bytes` | Reference-counted, zero-copy slicing, Tokio ecosystem integration |
| Synchronization primitives | `std::sync::RwLock` | `parking_lot::RwLock` | 1.5-5x faster, 1 byte Mutex, no poisoning |
| Error type boilerplate | Manual `impl Error` | `thiserror::Error` derive | Clean error hierarchy with automatic From impls |

## Common Pitfalls

### Pitfall 1: pyo3-async-runtimes Defaults to Multi-Thread Runtime

**What goes wrong:** Calling `future_into_py` without initializing the runtime uses the default multi-threaded Tokio runtime, which spawns worker threads that contend with Python's GIL.
**Why it happens:** `pyo3-async-runtimes` source code initializes with `Builder::new_multi_thread()` by default.
**How to avoid:** Call `pyo3_async_runtimes::tokio::init(Builder::new_current_thread().enable_all())` in the `#[pymodule]` init function, before any `future_into_py` call.
**Warning signs:** Multiple threads visible in profiler; unexplained GIL contention. [VERIFIED: pyo3-async-runtimes source code on GitHub]

### Pitfall 2: Lifetime Error on &self in Async Methods

**What goes wrong:** Compiler error: `&self` does not satisfy `'static` when used inside `future_into_py`.
**Why it happens:** `future_into_py` requires the future to be `Send + 'static`, but `&self` is borrowed from PyO3's runtime borrow checker with a limited lifetime.
**How to avoid:** Wrap store in `Arc<Store>`, clone the Arc before the async block. Never reference `self` inside the async closure.
**Warning signs:** Lifetime errors mentioning `'py` or `'_` in async contexts. [VERIFIED: pyo3-asyncio issue #50, pyo3-async-runtimes docs]

### Pitfall 3: SET Return Type Mismatch

**What goes wrong:** Redis-py's `set()` returns `True` on success and `None` when NX/XX condition fails. If you return `bool` (True/False), NX/XX failure returns `False` instead of `None`, breaking Prefect code that checks `result is None`.
**Why it happens:** Python `True`/`None` is different from Rust `bool`. Need `Option<bool>`.
**How to avoid:** Return `Option<bool>` from the Rust side: `Some(true)` for success, `None` for NX/XX failure. PyO3 converts `None` variant to Python `None`.
**Warning signs:** Prefect tests fail on SET NX when key exists. [VERIFIED: redis-py source -- return type is `bool | str | bytes | None`]

### Pitfall 4: Not Handling timedelta for EX/PX

**What goes wrong:** redis-py's `ExpiryT = Union[int, timedelta]`. Users may pass `timedelta(seconds=30)` instead of `30`.
**Why it happens:** The type annotation in redis-py allows both.
**How to avoid:** Accept `PyAny` for ex/px parameters and extract either `i64` or `timedelta` (via `total_seconds()`).
**Warning signs:** TypeError when Prefect passes timedelta objects to SET. [VERIFIED: redis-py typing.py -- `ExpiryT = Union[int, timedelta]`]

### Pitfall 5: maturin Module Name vs Package Name

**What goes wrong:** Python import fails: `ModuleNotFoundError: No module named 'burner_redis'`.
**Why it happens:** Mismatch between the Rust `#[pymodule]` name, the crate `lib.name`, and the Python package structure.
**How to avoid:** Use `_burner_redis` as the Rust module name (the native extension), and create a `python/burner_redis/__init__.py` that re-exports from `_burner_redis`. Or set `[tool.maturin] module-name = "burner_redis._burner_redis"` in pyproject.toml.
**Warning signs:** ImportError after `maturin develop`. [ASSUMED -- common maturin pitfall]

### Pitfall 6: parking_lot::RwLock is Not Async-Aware

**What goes wrong:** Holding a `parking_lot::RwLock` read/write guard across an `.await` point blocks the Tokio runtime thread.
**Why it happens:** `parking_lot::RwLock` is a synchronous lock; holding it across await points prevents other futures from running.
**How to avoid:** For the current-thread runtime, acquire the lock, do the operation, and drop the guard **before** any await. Since our store operations are synchronous (HashMap lookups), this is natural -- just don't hold the guard across async boundaries.
**Warning signs:** Deadlocks or hangs during concurrent Python async calls. [ASSUMED -- general Tokio best practice]

## Code Examples

### Complete Cargo.toml for Phase 1

```toml
# Source: CLAUDE.md stack specification + crate version verification
[package]
name = "burner-redis"
version = "0.1.0"
edition = "2024"

[lib]
name = "_burner_redis"
crate-type = ["cdylib"]

[dependencies]
pyo3 = { version = "0.28.3", features = ["extension-module", "abi3-py39"] }
pyo3-async-runtimes = { version = "0.28.0", features = ["tokio-runtime"] }
tokio = { version = "1.51", features = ["rt", "time", "sync"] }
parking_lot = "0.12.5"
bytes = "1.11"
thiserror = "2.0"

[dev-dependencies]
# Rust tests only; Python tests use pytest
```

### Complete pyproject.toml for Phase 1

```toml
# Source: maturin documentation + CLAUDE.md conventions
[build-system]
requires = ["maturin>=1.0,<2.0"]
build-backend = "maturin"

[project]
name = "burner-redis"
version = "0.1.0"
requires-python = ">=3.9"
description = "An embedded, in-process Redis-compatible database"

[project.optional-dependencies]
dev = ["pytest", "pytest-asyncio", "maturin"]

[tool.maturin]
features = ["pyo3/extension-module", "pyo3/abi3-py39"]
module-name = "burner_redis._burner_redis"

[tool.pytest.ini_options]
asyncio_mode = "auto"
```

### Module Entry Point (lib.rs)

```rust
// Source: PyO3 class guide + pyo3-async-runtimes README
use pyo3::prelude::*;

mod store;
mod commands;

use std::sync::Arc;
use store::Store;

#[pyclass]
pub struct BurnerRedis {
    store: Arc<Store>,
}

#[pymethods]
impl BurnerRedis {
    #[new]
    fn new() -> Self {
        BurnerRedis {
            store: Arc::new(Store::new()),
        }
    }

    // String commands implemented in commands/strings.rs
    // Each method follows the future_into_py + Arc::clone pattern
}

#[pymodule]
fn _burner_redis(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // CRITICAL: Must init before any future_into_py call
    pyo3_async_runtimes::tokio::init(
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
    );

    m.add_class::<BurnerRedis>()?;
    Ok(())
}
```

### Python Test Example (test_strings.py)

```python
# Source: pytest-asyncio docs + redis-py behavior specification
import pytest
from burner_redis import BurnerRedis

@pytest.fixture
async def r():
    return BurnerRedis()

async def test_set_and_get(r):
    result = await r.set("key", "value")
    assert result is True

    value = await r.get("key")
    assert value == b"value"

async def test_get_missing_key(r):
    value = await r.get("nonexistent")
    assert value is None

async def test_set_nx_existing_key(r):
    await r.set("key", "value")
    result = await r.set("key", "other", nx=True)
    assert result is None  # NX fails when key exists

    value = await r.get("key")
    assert value == b"value"  # Original value unchanged

async def test_set_xx_missing_key(r):
    result = await r.set("key", "value", xx=True)
    assert result is None  # XX fails when key doesn't exist

async def test_set_with_ex(r):
    import asyncio
    await r.set("key", "value", ex=1)
    assert await r.get("key") == b"value"
    await asyncio.sleep(1.1)
    assert await r.get("key") is None  # Expired

async def test_delete(r):
    await r.set("a", "1")
    await r.set("b", "2")
    count = await r.delete("a", "b", "nonexistent")
    assert count == 2  # Only 2 keys existed

async def test_exists(r):
    await r.set("a", "1")
    await r.set("b", "2")
    count = await r.exists("a", "b", "nonexistent")
    assert count == 2  # Only 2 keys exist
```

### Python __init__.py

```python
# python/burner_redis/__init__.py
from burner_redis._burner_redis import BurnerRedis

__all__ = ["BurnerRedis"]
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| pyo3-asyncio crate | pyo3-async-runtimes | PyO3 0.21+ (2024) | New crate name, same maintainers. Old crate is unmaintained. |
| PyO3 `Python::with_gil` | PyO3 `Python::attach` | PyO3 0.28 (2025) | GIL-agnostic naming. `with_gil` still works but deprecated. |
| `&PyAny` return type | `Bound<'_, PyAny>` return type | PyO3 0.21+ | Bound API is now the standard; unbound refs deprecated |
| `PyCell<T>` borrow pattern | `Bound<T>` borrow pattern | PyO3 0.22+ | `Bound::borrow()` / `borrow_mut()` replaces PyCell |

**Deprecated/outdated:**
- `pyo3-asyncio` (crate): Replaced by `pyo3-async-runtimes`. Do not use.
- `Python::with_gil()`: Renamed to `Python::attach()` in PyO3 0.28. Both work but `attach` is preferred.
- `&PyModule` in `#[pymodule]`: Use `&Bound<'_, PyModule>` in PyO3 0.28+.

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | Use `std::time::Instant` instead of `tokio::time::Instant` for expiration timestamps | Architecture Patterns - Pattern 1 | LOW -- both work; std is safer for cross-runtime use |
| A2 | Import `redis.exceptions.ResponseError` at runtime via `py.import("redis")` for isinstance compatibility | Anti-Patterns | MEDIUM -- if redis is not installed, need to define our own exception class that matches the name |
| A3 | pytest and pytest-asyncio latest versions work with Python 3.9-3.14 | Standard Stack | LOW -- these are mature packages with wide Python support |
| A4 | `parking_lot::RwLock` guard holding across non-await sync code is safe in current-thread Tokio | Pitfall 6 | LOW -- current-thread runtime is cooperative; sync code does not yield |
| A5 | maturin `module-name` config handles the `_burner_redis` -> `burner_redis` package mapping | Pitfall 5 | MEDIUM -- may need manual python/ directory structure instead |

## Open Questions

1. **Should we depend on the `redis` Python package for exception types?**
   - What we know: redis-py defines `ResponseError`, `DataError`, etc. Prefect code may catch these by type.
   - What's unclear: Is `redis` always installed alongside `burner-redis`? Should we make it an optional dependency?
   - Recommendation: Define our own exception classes that subclass from `redis.exceptions` if available, with fallback to standalone classes. This avoids a hard dependency while maintaining isinstance compatibility.

2. **ExpiryT handling: int only or also timedelta?**
   - What we know: redis-py accepts `Union[int, timedelta]` for `ex` and `px`.
   - What's unclear: Does Prefect pass timedelta objects or always int?
   - Recommendation: Support both from the start. Extract via `PyAny` and handle both `int` and `timedelta` types. The cost is minimal and prevents future breakage.

3. **Should operations be truly async or sync-wrapped-in-async?**
   - What we know: In-memory HashMap operations are synchronous and fast (< 1 microsecond). `future_into_py` adds overhead for true async.
   - What's unclear: Whether the overhead of `future_into_py` is acceptable for every operation.
   - Recommendation: Use `future_into_py` for all methods. The overhead is negligible compared to Python/Rust boundary crossing, and it maintains the async contract. This also prepares for Phase 4 (TTL timers) and Phase 8 (persistence) which genuinely need async.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Rust (rustc) | Core compilation | Yes | 1.94.1 | -- |
| Cargo | Build system | Yes | 1.94.1 | -- |
| Python 3 | Binding target | Yes | 3.14.3 | -- |
| maturin | Build tool | No | -- | `pip install maturin` (1.13.1 available) |
| pytest | Testing | No | -- | `pip install pytest` |
| pytest-asyncio | Async testing | No | -- | `pip install pytest-asyncio` |

**Missing dependencies with no fallback:**
- None -- all missing deps are installable via pip.

**Missing dependencies with fallback:**
- maturin, pytest, pytest-asyncio: Install via pip. Add to `[project.optional-dependencies]` in pyproject.toml.

**Note:** Python 3.14.3 is available on this machine (higher than the 3.9 minimum target). PyO3 0.28.3 has verified support for Python 3.14. [VERIFIED: PyO3 GitHub -- "first release tested against Python 3.14.0 final"]

## Validation Architecture

### Test Framework

| Property | Value |
|----------|-------|
| Framework (Rust) | Built-in `#[test]` |
| Framework (Python) | pytest + pytest-asyncio |
| Config file | `pyproject.toml` `[tool.pytest.ini_options]` (Wave 0 creation) |
| Quick run command | `maturin develop && python -m pytest tests/ -x` |
| Full suite command | `cargo test && maturin develop && python -m pytest tests/ -v` |

### Phase Requirements to Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| FOUND-01 | Import `from burner_redis import BurnerRedis` succeeds | smoke | `python -c "from burner_redis import BurnerRedis"` | No -- Wave 0 |
| FOUND-02 | Methods match redis.asyncio.Redis signatures | unit (Python) | `pytest tests/test_strings.py::test_set_and_get -x` | No -- Wave 0 |
| FOUND-03 | Methods are awaitable | unit (Python) | `pytest tests/test_strings.py -x` (all tests are async) | No -- Wave 0 |
| STR-01 | SET key with value | unit (Python) | `pytest tests/test_strings.py::test_set_and_get -x` | No -- Wave 0 |
| STR-02 | SET with NX/XX flags | unit (Python) | `pytest tests/test_strings.py::test_set_nx -x` | No -- Wave 0 |
| STR-03 | SET with EX/PX expiration | unit (Python) + Rust unit | `pytest tests/test_strings.py::test_set_with_ex -x` | No -- Wave 0 |
| STR-04 | GET returns bytes or None | unit (Python) | `pytest tests/test_strings.py::test_get_missing_key -x` | No -- Wave 0 |
| STR-05 | DELETE returns count | unit (Python) | `pytest tests/test_strings.py::test_delete -x` | No -- Wave 0 |
| STR-06 | EXISTS returns count | unit (Python) | `pytest tests/test_strings.py::test_exists -x` | No -- Wave 0 |

### Sampling Rate

- **Per task commit:** `maturin develop && python -m pytest tests/ -x` (fast fail)
- **Per wave merge:** `cargo test && maturin develop && python -m pytest tests/ -v`
- **Phase gate:** Full suite green before `/gsd-verify-work`

### Wave 0 Gaps

- [ ] `tests/conftest.py` -- shared BurnerRedis fixture
- [ ] `tests/test_strings.py` -- covers STR-01 through STR-06, FOUND-01 through FOUND-03
- [ ] `pyproject.toml` `[tool.pytest.ini_options]` with `asyncio_mode = "auto"`
- [ ] Rust unit tests in `src/store.rs` for pure Rust store operations

## Security Domain

Security enforcement is enabled by default (not explicitly disabled in config).

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | No | N/A -- in-process, no auth boundary |
| V3 Session Management | No | N/A -- no sessions |
| V4 Access Control | No | N/A -- in-process |
| V5 Input Validation | Yes | Validate key/value types at PyO3 boundary; reject unexpected types with TypeError |
| V6 Cryptography | No | N/A -- no crypto in Phase 1 |

### Known Threat Patterns for Rust+PyO3

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Memory safety via unsafe | Tampering | No `unsafe` blocks needed in Phase 1; PyO3 handles FFI boundary |
| Panic across FFI boundary | Denial of Service | PyO3 catches Rust panics and converts to Python exceptions |
| Unbounded memory growth | Denial of Service | Future phases add maxmemory; Phase 1 is unbounded (acceptable for dev) |

## Sources

### Primary (HIGH confidence)
- [PyO3 0.28.3](https://docs.rs/pyo3/0.28.3/pyo3/) -- `#[pyclass]`, `#[pymethods]`, `Bound<>` API
- [pyo3-async-runtimes 0.28.0](https://docs.rs/pyo3-async-runtimes/0.28.0/) -- `future_into_py` signature and constraints
- [pyo3-async-runtimes source (GitHub)](https://github.com/PyO3/pyo3-async-runtimes/blob/main/src/tokio.rs) -- Default multi-thread runtime, `init()` function
- [pyo3-asyncio issue #50](https://github.com/awestlake87/pyo3-asyncio/issues/50) -- Pattern for async methods on `#[pyclass]` structs
- [redis-py core.py (GitHub master)](https://github.com/redis/redis-py/blob/master/redis/commands/core.py) -- Exact method signatures for set/get/delete/exists
- [redis-py typing.py (GitHub master)](https://github.com/redis/redis-py/blob/master/redis/typing.py) -- KeyT, EncodableT, ExpiryT type aliases
- [redis-py exceptions.py (GitHub master)](https://github.com/redis/redis-py/blob/master/redis/exceptions.py) -- ResponseError, DataError hierarchy
- [maturin user guide](https://www.maturin.rs/) -- Project configuration, module-name, abi3
- crates.io `cargo search` -- Version verification for all Rust dependencies

### Secondary (MEDIUM confidence)
- [PyO3 user guide - Python classes](https://pyo3.rs/main/class) -- `#[pyclass]` restrictions (Send+Sync), `#[new]` pattern
- [pytest-asyncio docs](https://pytest-asyncio.readthedocs.io/) -- `asyncio_mode = "auto"` configuration
- [PyO3 GitHub releases](https://github.com/pyo3/pyo3/releases) -- Python 3.14 compatibility confirmation

### Tertiary (LOW confidence)
- None -- all claims verified or cited.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- All versions verified against crate registry; all libraries are CLAUDE.md-specified
- Architecture: HIGH -- Patterns verified from official docs and source code; async bridge pattern confirmed from multiple sources
- Pitfalls: HIGH -- Runtime default verified from source code; lifetime issue confirmed from GitHub issues
- redis-py compatibility: HIGH -- Exact signatures extracted from redis-py source on GitHub

**Research date:** 2026-04-10
**Valid until:** 2026-05-10 (stable ecosystem; PyO3 0.28 is current major)
