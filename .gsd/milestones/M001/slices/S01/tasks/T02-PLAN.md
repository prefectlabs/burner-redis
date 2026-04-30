# T02: Implement all string commands (SET with NX/XX/EX/PX, GET, DELETE, EXISTS) as async Python methods on BurnerRedis, with comprehensive Python integration tests.

**Slice:** S01 — **Milestone:** M001

## Description

Implement all string commands (SET with NX/XX/EX/PX, GET, DELETE, EXISTS) as async Python methods on BurnerRedis, with comprehensive Python integration tests.

Purpose: Deliver the full string command API surface that matches redis.asyncio.Redis method signatures, completing all Phase 1 requirements.
Output: Working async set/get/delete/exists methods passing comprehensive pytest suite covering all flags, edge cases, and redis-py compatibility.

## Legacy Source

---
phase: 01-foundation-and-string-commands
plan: 02
type: execute
wave: 2
depends_on: [01-01]
files_modified:
  - src/lib.rs
  - src/commands/strings.rs
  - tests/conftest.py
  - tests/test_strings.py
autonomous: true
requirements: [FOUND-02, FOUND-03, STR-01, STR-02, STR-03, STR-04, STR-05, STR-06]

must_haves:
  truths:
    - "User can SET a key with a string value and GET it back as bytes"
    - "SET with NX returns None when key already exists, True when key is new"
    - "SET with XX returns None when key does not exist, True when key exists"
    - "SET with EX/PX stores expiration; GET returns None after TTL elapses"
    - "SET accepts both str and bytes for keys and values"
    - "SET accepts both int and timedelta for EX/PX parameters"
    - "GET returns None for missing keys"
    - "DELETE accepts variadic keys and returns integer count of deleted keys"
    - "EXISTS accepts variadic keys and returns integer count of existing keys"
    - "All methods are async-compatible (awaitable)"
    - "Method signatures match redis.asyncio.Redis (same param names, defaults, return types)"
  artifacts:
    - path: "src/commands/strings.rs"
      provides: "String command implementations wired to Store"
      contains: "fn set"
      min_lines: 50
    - path: "src/lib.rs"
      provides: "BurnerRedis pyclass with async set/get/delete/exists methods"
      contains: "future_into_py"
    - path: "tests/conftest.py"
      provides: "Shared pytest fixtures"
      contains: "BurnerRedis"
    - path: "tests/test_strings.py"
      provides: "Comprehensive async tests for all string commands"
      contains: "test_set_and_get"
      min_lines: 80
  key_links:
    - from: "src/lib.rs"
      to: "src/commands/strings.rs"
      via: "method delegation from BurnerRedis #[pymethods] to commands::strings functions"
      pattern: "commands::strings::"
    - from: "src/lib.rs"
      to: "src/store.rs"
      via: "Arc<Store> passed to string command functions"
      pattern: "self.store.clone()"
    - from: "src/commands/strings.rs"
      to: "src/store.rs"
      via: "Store.set() and Store.get() calls"
      pattern: "store\\.(set|get|delete|exists)"
    - from: "tests/test_strings.py"
      to: "python/burner_redis/__init__.py"
      via: "import BurnerRedis"
      pattern: "from burner_redis import BurnerRedis"
---

<objective>
Implement all string commands (SET with NX/XX/EX/PX, GET, DELETE, EXISTS) as async Python methods on BurnerRedis, with comprehensive Python integration tests.

Purpose: Deliver the full string command API surface that matches redis.asyncio.Redis method signatures, completing all Phase 1 requirements.
Output: Working async set/get/delete/exists methods passing comprehensive pytest suite covering all flags, edge cases, and redis-py compatibility.
</objective>

<execution_context>
@$HOME/.claude/get-shit-done/workflows/execute-plan.md
@$HOME/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@.planning/PROJECT.md
@.planning/ROADMAP.md
@.planning/STATE.md
@.planning/phases/01-foundation-and-string-commands/01-RESEARCH.md
@.planning/phases/01-foundation-and-string-commands/01-01-SUMMARY.md

<interfaces>
<!-- Key types and contracts from Plan 01 that this plan builds on. -->

From src/store.rs:
```rust
pub struct ValueEntry {
    pub data: Bytes,
    pub expires_at: Option<Instant>,
}

pub struct Store {
    // data: RwLock<HashMap<Bytes, ValueEntry>>
}

impl Store {
    pub fn new() -> Self;
    pub fn get(&self, key: &Bytes) -> Option<Bytes>;
    pub fn set(&self, key: Bytes, value: Bytes, ttl: Option<Duration>, nx: bool, xx: bool) -> bool;
    pub fn delete(&self, keys: &[Bytes]) -> i64;
    pub fn exists(&self, keys: &[Bytes]) -> i64;
}
```

From src/lib.rs:
```rust
#[pyclass]
pub struct BurnerRedis {
    store: Arc<Store>,
}

#[pymethods]
impl BurnerRedis {
    #[new]
    fn new() -> Self;
    // String command methods to be added in this plan
}
```

From python/burner_redis/__init__.py:
```python
from burner_redis._burner_redis import BurnerRedis
__all__ = ["BurnerRedis"]
```
</interfaces>
</context>

<tasks>

<task type="auto" tdd="true">
  <name>Task 1: Implement async string command methods on BurnerRedis with helper utilities</name>
  <files>src/lib.rs, src/commands/strings.rs</files>
  <read_first>
    src/lib.rs
    src/store.rs
    src/commands/mod.rs
    src/commands/strings.rs
    .planning/phases/01-foundation-and-string-commands/01-RESEARCH.md
  </read_first>
  <behavior>
    - set("key", "value") -> True (basic set returns True on success)
    - set("key", b"value") -> True (bytes input accepted)
    - set("key", "value", nx=True) when key exists -> None
    - set("key", "value", nx=True) when key absent -> True
    - set("key", "value", xx=True) when key absent -> None
    - set("key", "value", xx=True) when key exists -> True
    - set("key", "value", ex=5) -> True (stores 5-second TTL)
    - set("key", "value", px=5000) -> True (stores 5000-ms TTL)
    - set("key", "value", ex=timedelta(seconds=5)) -> True (timedelta accepted)
    - get("key") -> b"value" (returns bytes)
    - get("missing") -> None
    - get("expired_key") -> None (after TTL elapses)
    - delete("a", "b", "missing") -> 2 (returns count of existing keys deleted)
    - exists("a", "b", "missing") -> 2 (returns count of existing keys)
  </behavior>
  <action>
**src/commands/strings.rs** -- implement helper functions for PyO3 type extraction and command execution:

```rust
use bytes::Bytes;
use pyo3::prelude::*;
use pyo3::types::PyAny;
use std::time::Duration;

/// Extract a key or value from a Python object (str or bytes).
/// Matches redis-py behavior: str auto-encoded to UTF-8 bytes.
pub fn extract_bytes(obj: &Bound<'_, PyAny>) -> PyResult<Bytes> {
    if let Ok(s) = obj.extract::<&str>() {
        Ok(Bytes::from(s.as_bytes().to_vec()))
    } else if let Ok(b) = obj.extract::<&[u8]>() {
        Ok(Bytes::copy_from_slice(b))
    } else {
        Err(pyo3::exceptions::PyTypeError::new_err(
            "expected str or bytes",
        ))
    }
}

/// Extract an expiration value from a Python object.
/// Accepts int (seconds or milliseconds) or datetime.timedelta.
/// Returns Duration.
pub fn extract_expiry(obj: &Bound<'_, PyAny>, unit_millis: bool) -> PyResult<Duration> {
    // Try extracting as integer first
    if let Ok(val) = obj.extract::<u64>() {
        return Ok(if unit_millis {
            Duration::from_millis(val)
        } else {
            Duration::from_secs(val)
        });
    }
    // Try extracting as timedelta via total_seconds()
    if let Ok(total_secs) = obj.call_method0("total_seconds").and_then(|v| v.extract::<f64>()) {
        if total_secs < 0.0 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "expiration must be non-negative",
            ));
        }
        return Ok(Duration::from_secs_f64(total_secs));
    }
    Err(pyo3::exceptions::PyTypeError::new_err(
        "expected int or timedelta for expiration",
    ))
}
```

**src/lib.rs** -- add async methods to the `#[pymethods]` impl block on BurnerRedis. Do NOT remove the existing `#[new]` method or module init. Add these methods inside the existing `#[pymethods] impl BurnerRedis` block:

```rust
// At the top of lib.rs, add these imports (merge with existing):
use pyo3::types::PyAnyMethods;
use std::time::Duration;
use commands::strings::{extract_bytes, extract_expiry};

// Inside #[pymethods] impl BurnerRedis { ... }:

/// SET command matching redis.asyncio.Redis.set() signature.
/// Returns True on success, None when NX/XX condition fails.
#[pyo3(signature = (name, value, ex=None, px=None, nx=false, xx=false))]
fn set<'py>(
    &self,
    py: Python<'py>,
    name: &Bound<'py, PyAny>,
    value: &Bound<'py, PyAny>,
    ex: Option<&Bound<'py, PyAny>>,
    px: Option<&Bound<'py, PyAny>>,
    nx: bool,
    xx: bool,
) -> PyResult<Bound<'py, PyAny>> {
    let store = self.store.clone();
    let key = extract_bytes(name)?;
    let val = extract_bytes(value)?;

    // Determine TTL: px takes precedence over ex (matches Redis behavior)
    let ttl = if let Some(px_val) = px {
        Some(extract_expiry(px_val, true)?)
    } else if let Some(ex_val) = ex {
        Some(extract_expiry(ex_val, false)?)
    } else {
        None
    };

    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let success = store.set(key, val, ttl, nx, xx);
        if success {
            Ok(Some(true))   // Python: True
        } else {
            Ok(None)         // Python: None (NX/XX condition failed)
        }
    })
}

/// GET command matching redis.asyncio.Redis.get() signature.
/// Returns bytes or None.
fn get<'py>(
    &self,
    py: Python<'py>,
    name: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyAny>> {
    let store = self.store.clone();
    let key = extract_bytes(name)?;

    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        Ok(store.get(&key).map(|b| b.to_vec()))
        // Option<Vec<u8>> -> Python bytes or None
    })
}

/// DELETE command matching redis.asyncio.Redis.delete() signature.
/// Accepts variadic keys, returns count of deleted keys.
#[pyo3(signature = (*names))]
fn delete<'py>(
    &self,
    py: Python<'py>,
    names: &Bound<'py, pyo3::types::PyTuple>,
) -> PyResult<Bound<'py, PyAny>> {
    let store = self.store.clone();
    let keys: Vec<Bytes> = names
        .iter()
        .map(|obj| extract_bytes(&obj))
        .collect::<PyResult<Vec<_>>>()?;

    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        Ok(store.delete(&keys))
    })
}

/// EXISTS command matching redis.asyncio.Redis.exists() signature.
/// Accepts variadic keys, returns count of existing keys.
#[pyo3(signature = (*names))]
fn exists<'py>(
    &self,
    py: Python<'py>,
    names: &Bound<'py, pyo3::types::PyTuple>,
) -> PyResult<Bound<'py, PyAny>> {
    let store = self.store.clone();
    let keys: Vec<Bytes> = names
        .iter()
        .map(|obj| extract_bytes(&obj))
        .collect::<PyResult<Vec<_>>>()?;

    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        Ok(store.exists(&keys))
    })
}
```

Key implementation details:
- `set` returns `Option<bool>`: `Some(true)` on success, `None` on NX/XX failure. PyO3 maps `None` to Python `None` and `Some(true)` to Python `True`.
- `get` returns `Option<Vec<u8>>`: PyO3 maps `Some(vec)` to Python `bytes`, `None` to Python `None`.
- `delete` and `exists` use `#[pyo3(signature = (*names))]` for variadic arguments, receiving a `PyTuple`.
- `ex` and `px` parameters accept `Option<&Bound<PyAny>>` so they can handle both `int` and `timedelta`.
- The `extract_bytes` function accepts both `str` and `bytes` Python objects.
- The `extract_expiry` function accepts both `int` and `timedelta` Python objects.
- All methods clone `Arc<Store>` before the async block (never borrow `&self` across await).
- The async block inside `future_into_py` accesses the Store synchronously (no await on lock).

After implementing, run `cargo test` to verify Rust compilation still passes, then `maturin develop` to rebuild the extension.
  </action>
  <verify>
    <automated>cd /Users/desertaxle/dev/prefectlabs/burner-redis && cargo test 2>&1 | tail -5 && maturin develop 2>&1 | tail -3</automated>
  </verify>
  <acceptance_criteria>
    - src/commands/strings.rs contains `pub fn extract_bytes(obj: &Bound<'_, PyAny>) -> PyResult<Bytes>`
    - src/commands/strings.rs contains `pub fn extract_expiry(obj: &Bound<'_, PyAny>, unit_millis: bool) -> PyResult<Duration>`
    - src/commands/strings.rs handles both `str` and `bytes` extraction (contains `extract::<&str>` and `extract::<&[u8]>`)
    - src/commands/strings.rs handles both `int` and `timedelta` expiry extraction (contains `extract::<u64>` and `call_method0("total_seconds")`)
    - src/lib.rs contains `#[pyo3(signature = (name, value, ex=None, px=None, nx=false, xx=false))]` on set method
    - src/lib.rs set method returns `PyResult<Bound<'py, PyAny>>` and uses `future_into_py`
    - src/lib.rs set method returns `Ok(Some(true))` on success and `Ok(None)` on NX/XX failure
    - src/lib.rs get method returns `Option<Vec<u8>>` from the async block (maps to bytes or None)
    - src/lib.rs delete method has `#[pyo3(signature = (*names))]` and returns `i64`
    - src/lib.rs exists method has `#[pyo3(signature = (*names))]` and returns `i64`
    - All four methods (set, get, delete, exists) use `self.store.clone()` before the async block
    - All four methods use `pyo3_async_runtimes::tokio::future_into_py`
    - `cargo test` exits 0
    - `maturin develop` exits 0
  </acceptance_criteria>
  <done>All four string command methods (set, get, delete, exists) are implemented on BurnerRedis with correct signatures matching redis.asyncio.Redis, all using future_into_py for async compatibility. Helper utilities handle str/bytes and int/timedelta extraction. Rust tests pass and extension builds.</done>
</task>

<task type="auto" tdd="true">
  <name>Task 2: Create comprehensive Python integration tests for all string commands</name>
  <files>tests/conftest.py, tests/test_strings.py</files>
  <read_first>
    src/lib.rs
    src/commands/strings.rs
    python/burner_redis/__init__.py
    .planning/phases/01-foundation-and-string-commands/01-RESEARCH.md
  </read_first>
  <behavior>
    - FOUND-01: `from burner_redis import BurnerRedis` succeeds
    - FOUND-02: set/get/delete/exists method names and parameter names match redis.asyncio.Redis
    - FOUND-03: all methods are awaitable (async)
    - STR-01: set("key", "value") stores and get("key") retrieves b"value"
    - STR-01: set(b"key", b"value") works with bytes inputs
    - STR-02: set("key", "val", nx=True) returns None when key exists, True when absent
    - STR-02: set("key", "val", xx=True) returns None when key absent, True when exists
    - STR-03: set("key", "val", ex=1) expires after 1 second
    - STR-03: set("key", "val", px=100) expires after 100 milliseconds
    - STR-03: set("key", "val", ex=timedelta(seconds=1)) accepts timedelta
    - STR-04: get("missing") returns None
    - STR-04: get returns bytes type
    - STR-05: delete("a", "b", "missing") returns 2 (count of deleted)
    - STR-06: exists("a", "b", "missing") returns 2 (count of existing)
  </behavior>
  <action>
Create two test files:

**tests/conftest.py**:
```python
import pytest
from burner_redis import BurnerRedis


@pytest.fixture
def r():
    """Create a fresh BurnerRedis instance for each test."""
    return BurnerRedis()
```

**tests/test_strings.py** -- comprehensive tests covering every requirement:
```python
"""Tests for string commands: SET, GET, DELETE, EXISTS.

Covers requirements: FOUND-01, FOUND-02, FOUND-03, STR-01 through STR-06.
"""
import asyncio
from datetime import timedelta

import pytest
from burner_redis import BurnerRedis


# --- FOUND-01: Import and instantiation ---

def test_import():
    """FOUND-01: BurnerRedis is importable and instantiable."""
    r = BurnerRedis()
    assert r is not None


# --- STR-01: Basic SET and GET ---

async def test_set_and_get(r):
    """STR-01: SET stores a value, GET retrieves it as bytes."""
    result = await r.set("key", "value")
    assert result is True

    value = await r.get("key")
    assert value == b"value"


async def test_set_and_get_bytes_input(r):
    """STR-01: SET and GET work with bytes keys and values."""
    result = await r.set(b"key", b"value")
    assert result is True

    value = await r.get(b"key")
    assert value == b"value"


async def test_set_overwrites_existing(r):
    """STR-01: SET overwrites an existing key."""
    await r.set("key", "value1")
    await r.set("key", "value2")

    value = await r.get("key")
    assert value == b"value2"


async def test_get_returns_bytes(r):
    """STR-04: GET always returns bytes type."""
    await r.set("key", "hello")
    value = await r.get("key")
    assert isinstance(value, bytes)


# --- STR-02: NX and XX flags ---

async def test_set_nx_new_key(r):
    """STR-02: SET NX succeeds when key does not exist."""
    result = await r.set("key", "value", nx=True)
    assert result is True

    value = await r.get("key")
    assert value == b"value"


async def test_set_nx_existing_key(r):
    """STR-02: SET NX returns None when key already exists."""
    await r.set("key", "original")
    result = await r.set("key", "new", nx=True)
    assert result is None

    # Original value unchanged
    value = await r.get("key")
    assert value == b"original"


async def test_set_xx_existing_key(r):
    """STR-02: SET XX succeeds when key exists."""
    await r.set("key", "original")
    result = await r.set("key", "updated", xx=True)
    assert result is True

    value = await r.get("key")
    assert value == b"updated"


async def test_set_xx_missing_key(r):
    """STR-02: SET XX returns None when key does not exist."""
    result = await r.set("key", "value", xx=True)
    assert result is None

    value = await r.get("key")
    assert value is None


# --- STR-03: EX and PX expiration ---

async def test_set_with_ex(r):
    """STR-03: SET with EX sets expiration in seconds."""
    await r.set("key", "value", ex=1)
    assert await r.get("key") == b"value"

    await asyncio.sleep(1.1)
    assert await r.get("key") is None


async def test_set_with_px(r):
    """STR-03: SET with PX sets expiration in milliseconds."""
    await r.set("key", "value", px=200)
    assert await r.get("key") == b"value"

    await asyncio.sleep(0.3)
    assert await r.get("key") is None


async def test_set_with_ex_timedelta(r):
    """STR-03: SET with EX accepts timedelta."""
    await r.set("key", "value", ex=timedelta(seconds=1))
    assert await r.get("key") == b"value"

    await asyncio.sleep(1.1)
    assert await r.get("key") is None


async def test_set_with_px_timedelta(r):
    """STR-03: SET with PX accepts timedelta."""
    await r.set("key", "value", px=timedelta(milliseconds=200))
    assert await r.get("key") == b"value"

    await asyncio.sleep(0.3)
    assert await r.get("key") is None


async def test_set_nx_with_ex(r):
    """STR-02 + STR-03: NX and EX combined."""
    result = await r.set("key", "value", nx=True, ex=60)
    assert result is True

    # Key exists now, NX should fail
    result = await r.set("key", "other", nx=True, ex=60)
    assert result is None


# --- STR-04: GET edge cases ---

async def test_get_missing_key(r):
    """STR-04: GET returns None for a key that was never set."""
    value = await r.get("nonexistent")
    assert value is None


async def test_get_expired_key(r):
    """STR-04: GET returns None for an expired key."""
    await r.set("key", "value", px=50)
    await asyncio.sleep(0.1)
    assert await r.get("key") is None


# --- STR-05: DELETE ---

async def test_delete_single_key(r):
    """STR-05: DELETE removes a single key."""
    await r.set("key", "value")
    count = await r.delete("key")
    assert count == 1

    assert await r.get("key") is None


async def test_delete_multiple_keys(r):
    """STR-05: DELETE returns count of existing keys deleted."""
    await r.set("a", "1")
    await r.set("b", "2")

    count = await r.delete("a", "b", "nonexistent")
    assert count == 2


async def test_delete_nonexistent_key(r):
    """STR-05: DELETE of nonexistent key returns 0."""
    count = await r.delete("nonexistent")
    assert count == 0


# --- STR-06: EXISTS ---

async def test_exists_single_key(r):
    """STR-06: EXISTS returns 1 for an existing key."""
    await r.set("key", "value")
    count = await r.exists("key")
    assert count == 1


async def test_exists_multiple_keys(r):
    """STR-06: EXISTS returns count of existing keys."""
    await r.set("a", "1")
    await r.set("b", "2")

    count = await r.exists("a", "b", "nonexistent")
    assert count == 2


async def test_exists_nonexistent_key(r):
    """STR-06: EXISTS returns 0 for nonexistent key."""
    count = await r.exists("nonexistent")
    assert count == 0


async def test_exists_expired_key(r):
    """STR-06: EXISTS returns 0 for expired key."""
    await r.set("key", "value", px=50)
    await asyncio.sleep(0.1)

    count = await r.exists("key")
    assert count == 0


# --- FOUND-03: Async compatibility ---

async def test_methods_are_awaitable(r):
    """FOUND-03: All command methods are async-compatible."""
    # This test verifies that all methods can be awaited
    set_result = await r.set("key", "value")
    get_result = await r.get("key")
    exists_result = await r.exists("key")
    delete_result = await r.delete("key")

    assert set_result is True
    assert get_result == b"value"
    assert exists_result == 1
    assert delete_result == 1
```

After creating test files, run the full test suite:
1. `maturin develop` (rebuild extension with latest code)
2. `python -m pytest tests/ -v` (run all tests with verbose output)

If any test fails, diagnose and fix the Rust implementation (src/lib.rs or src/commands/strings.rs), then rebuild and re-test. Common issues:
- If `set` returns `False` instead of `None` for NX/XX failure: check `Option<bool>` return type
- If `get` returns `str` instead of `bytes`: check `Vec<u8>` return type in async block
- If `delete`/`exists` get TypeError: check `#[pyo3(signature = (*names))]` and PyTuple extraction
  </action>
  <verify>
    <automated>cd /Users/desertaxle/dev/prefectlabs/burner-redis && cargo test 2>&1 | tail -5 && maturin develop 2>&1 | tail -3 && python -m pytest tests/ -v 2>&1 | tail -30</automated>
  </verify>
  <acceptance_criteria>
    - tests/conftest.py contains `@pytest.fixture` and `def r()` returning `BurnerRedis()`
    - tests/test_strings.py contains `def test_import()` (FOUND-01)
    - tests/test_strings.py contains `async def test_set_and_get(r)` (STR-01)
    - tests/test_strings.py contains `async def test_set_and_get_bytes_input(r)` (STR-01 bytes)
    - tests/test_strings.py contains `async def test_set_nx_existing_key(r)` with `assert result is None` (STR-02 NX)
    - tests/test_strings.py contains `async def test_set_xx_missing_key(r)` with `assert result is None` (STR-02 XX)
    - tests/test_strings.py contains `async def test_set_with_ex(r)` with `asyncio.sleep` (STR-03 EX)
    - tests/test_strings.py contains `async def test_set_with_px(r)` (STR-03 PX)
    - tests/test_strings.py contains `async def test_set_with_ex_timedelta(r)` (STR-03 timedelta)
    - tests/test_strings.py contains `async def test_get_missing_key(r)` with `assert value is None` (STR-04)
    - tests/test_strings.py contains `async def test_delete_multiple_keys(r)` with `assert count == 2` (STR-05)
    - tests/test_strings.py contains `async def test_exists_multiple_keys(r)` with `assert count == 2` (STR-06)
    - tests/test_strings.py contains `async def test_methods_are_awaitable(r)` (FOUND-03)
    - `python -m pytest tests/ -v` exits 0 with all tests passing
    - `cargo test` exits 0 with all Rust tests passing
  </acceptance_criteria>
  <done>All Python integration tests pass covering every requirement: FOUND-01 (import), FOUND-02 (signature match), FOUND-03 (async), STR-01 (set/get), STR-02 (NX/XX), STR-03 (EX/PX/timedelta), STR-04 (get None), STR-05 (delete count), STR-06 (exists count). Full test suite green via `cargo test && maturin develop && pytest tests/ -v`.</done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|----------|-------------|
| Python -> Rust FFI (set params) | User-provided keys, values, ex/px cross from Python into Rust |

## STRIDE Threat Register

| Threat ID | Category | Component | Disposition | Mitigation Plan |
|-----------|----------|-----------|-------------|-----------------|
| T-01-05 | Tampering | src/commands/strings.rs extract_bytes | mitigate | Validate input type is str or bytes; reject other types with PyTypeError |
| T-01-06 | Tampering | src/commands/strings.rs extract_expiry | mitigate | Validate expiry is int or timedelta; reject negative timedelta with PyValueError |
| T-01-07 | Denial of Service | src/lib.rs set method | accept | No size limit on values in Phase 1. Unbounded growth acceptable for dev. Future phases add limits. |
</threat_model>

<verification>
1. `cargo test` -- all Rust unit tests pass (store + any new tests)
2. `maturin develop` -- extension builds without errors
3. `python -m pytest tests/ -v` -- all Python integration tests pass
4. `python -c "from burner_redis import BurnerRedis; import asyncio; r = BurnerRedis(); print(asyncio.run(r.set('k','v'))); print(asyncio.run(r.get('k')))"` -- quick smoke test shows True and b'v'
</verification>

<success_criteria>
- All 25+ Python tests pass covering FOUND-01, FOUND-02, FOUND-03, STR-01 through STR-06
- SET returns True on success, None on NX/XX condition failure (not False)
- GET returns bytes on hit, None on miss or expired
- DELETE and EXISTS accept variadic keys and return integer counts
- Both str and bytes accepted as key/value inputs
- Both int and timedelta accepted for EX/PX parameters
- All methods are async (awaitable from Python)
</success_criteria>

<output>
After completion, create `.planning/phases/01-foundation-and-string-commands/01-02-SUMMARY.md`
</output>
