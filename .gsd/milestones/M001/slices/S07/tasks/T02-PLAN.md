# T02: Implement the Lock class for distributed locking with token-based ownership, blocking acquisition, and automatic expiration.

**Slice:** S07 — **Milestone:** M001

## Description

Implement the Lock class for distributed locking with token-based ownership, blocking acquisition, and automatic expiration. Matches redis-py Lock semantics using SET NX PX for atomic lock acquisition.

Purpose: Provide redis-py Lock API compatibility so Prefect code using `async with client.lock("name") as lock` works as a drop-in replacement for distributed locking patterns.

Output: `python/burner_redis/lock.py` with Lock class, `LockError` exception, updated `__init__.py` with exports and factory method, and `tests/test_locking.py` with comprehensive test coverage.

## Legacy Source

---
phase: 07-pipeline-and-locking
plan: 02
type: execute
wave: 1
depends_on: []
files_modified:
  - python/burner_redis/lock.py
  - python/burner_redis/__init__.py
  - tests/test_locking.py
autonomous: true
requirements:
  - LOCK-01
  - LOCK-02

must_haves:
  truths:
    - "User can acquire a lock with a timeout and release it"
    - "Lock verifies token ownership on release and raises LockError if token mismatch"
    - "Lock supports blocking acquisition with configurable polling interval and blocking_timeout"
    - "Lock supports automatic expiration via TTL"
    - "Lock supports async context manager usage (async with client.lock('name') as lock)"
    - "LockError exception is importable from burner_redis"
  artifacts:
    - path: "python/burner_redis/lock.py"
      provides: "Lock class with acquire/release and token-based ownership"
      contains: "class Lock"
    - path: "python/burner_redis/__init__.py"
      provides: "Lock and LockError exports and BurnerRedis.lock() factory method"
      contains: "LockError"
    - path: "tests/test_locking.py"
      provides: "Comprehensive pytest suite for LOCK-01, LOCK-02"
      contains: "test_lock"
  key_links:
    - from: "python/burner_redis/lock.py"
      to: "python/burner_redis/__init__.py"
      via: "Lock calls self._client.set() with NX/PX and self._client.get()/delete()"
      pattern: "self\\._client\\.set\\|self\\._client\\.get\\|self\\._client\\.delete"
    - from: "tests/test_locking.py"
      to: "python/burner_redis/lock.py"
      via: "Tests create locks and verify acquire/release/timeout behavior"
      pattern: "client\\.lock\\("
---

<objective>
Implement the Lock class for distributed locking with token-based ownership, blocking acquisition, and automatic expiration. Matches redis-py Lock semantics using SET NX PX for atomic lock acquisition.

Purpose: Provide redis-py Lock API compatibility so Prefect code using `async with client.lock("name") as lock` works as a drop-in replacement for distributed locking patterns.

Output: `python/burner_redis/lock.py` with Lock class, `LockError` exception, updated `__init__.py` with exports and factory method, and `tests/test_locking.py` with comprehensive test coverage.
</objective>

<execution_context>
@$HOME/.claude/get-shit-done/workflows/execute-plan.md
@$HOME/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@.planning/PROJECT.md
@.planning/ROADMAP.md
@.planning/STATE.md
@python/burner_redis/__init__.py
@src/lib.rs
@tests/conftest.py
@tests/test_strings.py

<interfaces>
<!-- Key types and contracts the executor needs -->

From python/burner_redis/__init__.py (existing):
```python
from burner_redis._burner_redis import BurnerRedis

class ResponseError(Exception):
    """Redis-compatible WRONGTYPE error."""
    pass

__all__ = ["BurnerRedis", "ResponseError"]
```

From src/lib.rs (BurnerRedis methods needed by Lock):
```python
# SET with NX and PX flags -- used for atomic lock acquisition
await client.set(name, value, px=milliseconds, nx=True)  # Returns True or None

# GET -- used to check lock token ownership
await client.get(name)  # Returns bytes or None

# DELETE -- used to release lock
await client.delete(name)  # Returns count of deleted keys
```

The SET method signature in Rust:
```rust
#[pyo3(signature = (name, value, ex=None, px=None, nx=false, xx=false))]
fn set<'py>(&self, py, name, value, ex, px, nx, xx) -> PyResult<Bound<'py, PyAny>>
// Returns True on success, None when NX condition fails
```
</interfaces>
</context>

<tasks>

<task type="auto">
  <name>Task 1: Create Lock class with LockError and wire into BurnerRedis</name>
  <files>python/burner_redis/lock.py, python/burner_redis/__init__.py</files>
  <read_first>python/burner_redis/__init__.py, src/lib.rs, tests/test_strings.py</read_first>
  <action>
1. Create `python/burner_redis/lock.py` with the Lock class and LockError exception:

```python
"""Lock class for distributed locking with token-based ownership.

Provides redis-py compatible Lock API using SET NX PX for atomic
lock acquisition with UUID token-based ownership verification.
"""
import asyncio
import uuid


class LockError(Exception):
    """Raised when a lock operation fails (e.g., release without ownership)."""
    pass


class Lock:
    """Distributed lock with token-based ownership.

    Created via client.lock(name, ...). Uses SET NX PX for atomic acquisition
    and token verification for safe release.

    Args:
        client: BurnerRedis instance
        name: Lock key name
        timeout: Lock TTL in seconds (None = no expiry). Converted to milliseconds for PX.
        sleep: Polling interval in seconds for blocking acquire (default 0.1)
        blocking: Whether acquire() blocks until lock is obtained (default True)
        blocking_timeout: Maximum seconds to wait for blocking acquire (None = wait forever)
    """

    def __init__(self, client, name, timeout=None, sleep=0.1, blocking=True, blocking_timeout=None):
        self._client = client
        self.name = name
        self.timeout = timeout
        self.sleep = sleep
        self.blocking = blocking
        self.blocking_timeout = blocking_timeout
        self.token = None

    async def acquire(self, blocking=None, blocking_timeout=None):
        """Acquire the lock.

        Args:
            blocking: Override instance blocking setting
            blocking_timeout: Override instance blocking_timeout setting

        Returns:
            True if lock was acquired, False if non-blocking and lock not available.
        """
        if blocking is None:
            blocking = self.blocking
        if blocking_timeout is None:
            blocking_timeout = self.blocking_timeout

        token = str(uuid.uuid4())

        # Calculate PX (milliseconds) from timeout (seconds)
        px = int(self.timeout * 1000) if self.timeout is not None else None

        if not blocking:
            # Non-blocking: single attempt
            result = await self._client.set(self.name, token, px=px, nx=True)
            if result is True:
                self.token = token
                return True
            return False

        # Blocking: poll until acquired or timeout
        elapsed = 0.0
        while True:
            result = await self._client.set(self.name, token, px=px, nx=True)
            if result is True:
                self.token = token
                return True

            if blocking_timeout is not None:
                elapsed += self.sleep
                if elapsed >= blocking_timeout:
                    return False

            await asyncio.sleep(self.sleep)

    async def release(self):
        """Release the lock.

        Verifies token ownership before deleting. Raises LockError if
        the lock is not owned by this instance (token mismatch or expired).
        """
        if self.token is None:
            raise LockError("Cannot release an unlocked lock")

        stored = await self._client.get(self.name)
        if stored is None:
            raise LockError("Cannot release an unlocked lock")

        if stored != self.token.encode():
            raise LockError("Cannot release a lock that's no longer owned")

        await self._client.delete(self.name)
        self.token = None

    async def __aenter__(self):
        acquired = await self.acquire()
        if not acquired:
            raise LockError("Unable to acquire lock")
        return self

    async def __aexit__(self, exc_type, exc_val, exc_tb):
        await self.release()
        return False
```

IMPORTANT: The `timeout` parameter is in seconds (matching redis-py), but SET PX expects milliseconds. Convert via `int(timeout * 1000)`.

IMPORTANT: The `token` is stored as a string UUID. When comparing with `get()` result (which returns bytes), compare `stored != self.token.encode()`.

IMPORTANT: The blocking loop uses `asyncio.sleep` (not time.sleep) to avoid blocking the event loop.

IMPORTANT: `release()` does a check-then-delete (GET, compare, DELETE). This is not atomic, but for an in-process embedded database, there is no race condition risk since there is no network partition. This matches the simplicity of the embedded use case.

2. Update `python/burner_redis/__init__.py`:

Add imports for Lock and LockError:
```python
from burner_redis._burner_redis import BurnerRedis
from burner_redis.pipeline import Pipeline
from burner_redis.lock import Lock, LockError
```

Note: If Pipeline import is not yet present (Plan 01 may not have run yet since both are Wave 1), add only Lock and LockError. The executor should check what imports currently exist and add to them without removing existing ones.

Add Lock and LockError to `__all__`:
```python
__all__ = ["BurnerRedis", "ResponseError", "Pipeline", "Lock", "LockError"]
```

Add a `lock()` factory method to BurnerRedis via monkey-patch:
```python
def _lock(self, name, timeout=None, sleep=0.1, blocking=True, blocking_timeout=None):
    """Create a Lock for distributed locking."""
    return Lock(self, name, timeout=timeout, sleep=sleep, blocking=blocking, blocking_timeout=blocking_timeout)

BurnerRedis.lock = _lock
```

IMPORTANT: The monkey-patch must be applied AFTER the BurnerRedis import and AFTER the `try/except` block for ResponseError subclassing. Place it at the end of the module.

IMPORTANT: If Plan 01's Pipeline monkey-patch is already present, add the lock monkey-patch alongside it. If not yet present, just add the lock one. The executor should be additive, not destructive.
  </action>
  <verify>
    <automated>cd /Users/desertaxle/dev/prefectlabs/burner-redis && python -c "from burner_redis import BurnerRedis, Lock, LockError; r = BurnerRedis(); lock = r.lock('test', timeout=10); print('Lock created:', type(lock).__name__)" 2>&1</automated>
  </verify>
  <acceptance_criteria>
    - grep -q "class Lock" python/burner_redis/lock.py
    - grep -q "class LockError" python/burner_redis/lock.py
    - grep -q "async def acquire" python/burner_redis/lock.py
    - grep -q "async def release" python/burner_redis/lock.py
    - grep -q "__aenter__" python/burner_redis/lock.py
    - grep -q "__aexit__" python/burner_redis/lock.py
    - grep -q "uuid" python/burner_redis/lock.py
    - grep -q "asyncio.sleep" python/burner_redis/lock.py
    - grep -q "nx=True" python/burner_redis/lock.py
    - grep -q "self.token" python/burner_redis/lock.py
    - grep -q "LockError" python/burner_redis/__init__.py
    - grep -q "Lock" python/burner_redis/__init__.py
    - python -c "from burner_redis import Lock, LockError" succeeds
    - python -c "from burner_redis import BurnerRedis; r = BurnerRedis(); lock = r.lock('test')" succeeds
  </acceptance_criteria>
  <done>Lock class exists in python/burner_redis/lock.py with acquire()/release() using SET NX PX and UUID tokens. LockError exception for ownership violations. Blocking acquire with asyncio.sleep polling. Async context manager support. BurnerRedis.lock() factory method works via monkey-patch. Lock and LockError importable from burner_redis package.</done>
</task>

<task type="auto">
  <name>Task 2: Comprehensive pytest suite for Lock</name>
  <files>tests/test_locking.py</files>
  <read_first>tests/conftest.py, tests/test_strings.py, python/burner_redis/lock.py, python/burner_redis/__init__.py</read_first>
  <action>
Create `tests/test_locking.py` with the following test structure:

```python
"""Tests for Lock distributed locking.

Covers requirements: LOCK-01, LOCK-02.
"""
import asyncio
import pytest
from burner_redis import BurnerRedis, LockError
```

All tests are async, use the `r` fixture from conftest.py.

**LOCK-01 (Acquire and release with token ownership):**

- `test_lock_acquire_and_release`: Create lock via `r.lock("mylock", timeout=10)`. `assert await lock.acquire() is True`. Verify key exists via `await r.get("mylock")` returns bytes (the token). `await lock.release()`. Verify key gone: `await r.get("mylock") is None`.
- `test_lock_acquire_sets_token`: After acquire, `lock.token` is a non-None string (UUID).
- `test_lock_release_clears_token`: After acquire then release, `lock.token is None`.
- `test_lock_release_without_acquire_raises`: Create lock, do NOT acquire, call `release()`. Raises `LockError`.
- `test_lock_release_expired_raises`: Create lock with very short timeout (0.1s = 100ms). Acquire, then `await asyncio.sleep(0.2)`, then release. Should raise `LockError` because the key expired.
- `test_lock_release_stolen_raises`: Acquire lock. Manually overwrite the key value via `await r.set("mylock", "stolen")`. Release should raise `LockError` (token mismatch).
- `test_lock_double_release_raises`: Acquire and release. Second release raises `LockError`.

**LOCK-02 (Timeout, blocking, token-based ownership):**

- `test_lock_timeout_expiry`: Create lock with `timeout=0.2` (200ms). Acquire. Verify key exists. `await asyncio.sleep(0.3)`. Verify key expired: `await r.get("mylock") is None`.
- `test_lock_no_timeout`: Create lock with `timeout=None`. Acquire. Key exists. Lock has no TTL (stays forever until released).
- `test_lock_blocking_acquire`: Acquire lock1 with short timeout (0.3s). Start lock2 acquire (blocking=True, blocking_timeout=1.0). lock2 should eventually succeed after lock1 expires. Verify lock2 owns the key.
- `test_lock_blocking_timeout_exceeded`: Acquire lock1 with `timeout=10` (long). Create lock2 with `blocking_timeout=0.2`. `result = await lock2.acquire()`. Assert `result is False` (timed out without acquiring).
- `test_lock_nonblocking_fail`: Acquire lock1. Create lock2 with `blocking=False`. `result = await lock2.acquire()`. Assert `result is False` (immediate fail).
- `test_lock_nonblocking_success`: No prior lock held. Create lock with `blocking=False`. `result = await lock.acquire()`. Assert `result is True`.
- `test_lock_token_uniqueness`: Acquire lock1, note token. Release. Acquire lock2 on same name. lock2.token should differ from lock1's original token.
- `test_lock_sleep_interval`: Create lock with `sleep=0.05`. Verify the attribute is set (testing interface, not timing).

**Async context manager:**

- `test_lock_context_manager`: `async with r.lock("mylock", timeout=10) as lock:` -- inside, verify `lock.token is not None` and key exists. After the block, verify key is gone (released).
- `test_lock_context_manager_releases_on_exception`:
  ```python
  with pytest.raises(ValueError):
      async with r.lock("mylock", timeout=10) as lock:
          raise ValueError("test error")
  # Lock should still be released
  assert await r.get("mylock") is None
  ```
- `test_lock_context_manager_acquire_fails`: Create lock1 and acquire it (long timeout). Try `async with r.lock("mylock", timeout=10, blocking=False) as lock2:`. Should raise `LockError` because acquire returns False in non-blocking mode.

**Edge cases:**

- `test_lock_different_names_independent`: Acquire lock on "lock1". Acquire separate lock on "lock2". Both succeed. Release both.

IMPORTANT: All test functions must be `async def` and use the `r` fixture.

IMPORTANT: For timing-sensitive tests (blocking acquire, timeout expiry), use generous margins. For timeout=0.2s, sleep 0.3s to verify expiry. Do NOT use margins under 50ms.

IMPORTANT: The `r` fixture creates a fresh BurnerRedis instance per test, so tests are isolated.
  </action>
  <verify>
    <automated>cd /Users/desertaxle/dev/prefectlabs/burner-redis && python -m pytest tests/test_locking.py -x -v 2>&1 | tail -40</automated>
  </verify>
  <acceptance_criteria>
    - grep -q "LOCK-01" tests/test_locking.py
    - grep -q "LOCK-02" tests/test_locking.py
    - grep -q "test_lock_acquire_and_release" tests/test_locking.py
    - grep -q "test_lock_context_manager" tests/test_locking.py
    - grep -q "test_lock_blocking_acquire" tests/test_locking.py
    - grep -q "test_lock_blocking_timeout_exceeded" tests/test_locking.py
    - grep -q "test_lock_nonblocking_fail" tests/test_locking.py
    - grep -q "test_lock_timeout_expiry" tests/test_locking.py
    - grep -q "test_lock_release_without_acquire_raises" tests/test_locking.py
    - grep -q "test_lock_release_stolen_raises" tests/test_locking.py
    - grep -q "test_lock_token_uniqueness" tests/test_locking.py
    - grep -q "LockError" tests/test_locking.py
    - grep -q "asyncio.sleep" tests/test_locking.py
    - python -m pytest tests/test_locking.py -x passes
    - python -m pytest tests/ -x passes (full regression)
  </acceptance_criteria>
  <done>Comprehensive pytest suite in tests/test_locking.py covers both LOCK requirements. Tests validate: acquire/release with token verification, LockError on ownership violations (LOCK-01), timeout-based expiry, blocking acquisition with polling and blocking_timeout, non-blocking immediate fail/success, token uniqueness across acquisitions (LOCK-02). Async context manager acquire-on-enter and release-on-exit (including exception paths) tested. Full regression suite passes.</done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|----------|-------------|
| Python user -> Lock | User controls lock name, timeout, and sleep parameters |

## STRIDE Threat Register

| Threat ID | Category | Component | Disposition | Mitigation Plan |
|-----------|----------|-----------|-------------|-----------------|
| T-07-03 | Tampering | Lock token verification | mitigate | Release compares stored value against local token; raises LockError on mismatch. Prevents releasing locks owned by other callers. |
| T-07-04 | Tampering | Lock release race condition | accept | GET-then-DELETE is non-atomic, but acceptable for in-process embedded database with no network partitions or concurrent processes. |
| T-07-05 | Denial of Service | Blocking acquire infinite loop | mitigate | blocking_timeout parameter limits maximum wait time; caller controls polling interval via sleep parameter |
</threat_model>

<verification>
1. `python -c "from burner_redis import Lock, LockError"` imports successfully
2. `r.lock("name", timeout=10)` returns a Lock instance
3. `python -m pytest tests/test_locking.py -v` all lock tests pass
4. `python -m pytest tests/ -v` full regression suite passes
5. Lock acquire uses SET NX PX for atomic acquisition
6. Lock release verifies token ownership before deleting
7. Blocking acquire polls with asyncio.sleep and respects blocking_timeout
8. Async context manager acquires on enter, releases on exit
</verification>

<success_criteria>
- Lock class uses SET NX PX for atomic acquisition with UUID tokens
- release() verifies token ownership and raises LockError on mismatch
- Blocking acquire polls with configurable sleep interval and blocking_timeout
- Non-blocking acquire returns False immediately if lock held
- Lock timeout (TTL) causes automatic key expiration
- Async context manager (async with client.lock(...) as lock) works correctly
- LockError exception is importable and raised appropriately
- Tests cover all 2 LOCK requirements with multiple cases each
- Full test suite (all prior phases + locking) passes with zero regressions
</success_criteria>

<output>
After completion, create `.planning/phases/07-pipeline-and-locking/07-02-SUMMARY.md`
</output>
