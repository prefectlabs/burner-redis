"""Tests for Lock distributed locking.

Covers requirements: LOCK-01, LOCK-02.
"""
import asyncio

import pytest
from burner_redis import BurnerRedis, LockError


# --- LOCK-01: Acquire and release with token ownership ---


async def test_lock_acquire_and_release(r):
    """LOCK-01: Acquire lock, verify key exists, release, verify key gone."""
    lock = r.lock("mylock", timeout=10)
    assert await lock.acquire() is True

    # Key should exist with the token value
    stored = await r.get("mylock")
    assert stored is not None
    assert isinstance(stored, bytes)

    await lock.release()

    # Key should be gone after release
    assert await r.get("mylock") is None


async def test_lock_acquire_sets_token(r):
    """LOCK-01: After acquire, lock.token is a non-None string (UUID)."""
    lock = r.lock("mylock", timeout=10)
    await lock.acquire()

    assert lock.token is not None
    assert isinstance(lock.token, str)
    assert len(lock.token) > 0

    await lock.release()


async def test_lock_release_clears_token(r):
    """LOCK-01: After acquire then release, lock.token is None."""
    lock = r.lock("mylock", timeout=10)
    await lock.acquire()
    assert lock.token is not None

    await lock.release()
    assert lock.token is None


async def test_lock_release_without_acquire_raises(r):
    """LOCK-01: Release without acquire raises LockError."""
    lock = r.lock("mylock", timeout=10)

    with pytest.raises(LockError):
        await lock.release()


async def test_lock_release_expired_raises(r):
    """LOCK-01: Release after key expired raises LockError."""
    lock = r.lock("mylock", timeout=0.1)  # 100ms TTL
    await lock.acquire()

    await asyncio.sleep(0.2)  # Wait for expiry

    with pytest.raises(LockError):
        await lock.release()


async def test_lock_release_stolen_raises(r):
    """LOCK-01: Release after key overwritten raises LockError (token mismatch)."""
    lock = r.lock("mylock", timeout=10)
    await lock.acquire()

    # Overwrite the lock key with a different value
    await r.set("mylock", "stolen")

    with pytest.raises(LockError):
        await lock.release()


async def test_lock_double_release_raises(r):
    """LOCK-01: Second release raises LockError."""
    lock = r.lock("mylock", timeout=10)
    await lock.acquire()
    await lock.release()

    with pytest.raises(LockError):
        await lock.release()


# --- LOCK-02: Timeout, blocking, token-based ownership ---


async def test_lock_timeout_expiry(r):
    """LOCK-02: Lock with timeout expires after the TTL."""
    lock = r.lock("mylock", timeout=0.2)  # 200ms TTL
    await lock.acquire()

    # Key should exist immediately
    assert await r.get("mylock") is not None

    # Wait for expiry
    await asyncio.sleep(0.3)

    # Key should be expired
    assert await r.get("mylock") is None


async def test_lock_no_timeout(r):
    """LOCK-02: Lock with timeout=None has no TTL (stays forever until released)."""
    lock = r.lock("mylock", timeout=None)
    await lock.acquire()

    # Key should exist
    assert await r.get("mylock") is not None

    # Still exists after a short wait (no expiry)
    await asyncio.sleep(0.1)
    assert await r.get("mylock") is not None

    await lock.release()
    assert await r.get("mylock") is None


async def test_lock_blocking_acquire(r):
    """LOCK-02: Blocking acquire waits for lock to become available."""
    lock1 = r.lock("mylock", timeout=0.3)  # Short TTL
    await lock1.acquire()

    lock2 = r.lock("mylock", timeout=10, blocking=True, blocking_timeout=1.0)
    # lock2 should eventually succeed after lock1 expires
    result = await lock2.acquire()
    assert result is True

    # lock2 should own the key
    stored = await r.get("mylock")
    assert stored == lock2.token.encode()

    await lock2.release()


async def test_lock_blocking_timeout_exceeded(r):
    """LOCK-02: Blocking acquire returns False when blocking_timeout exceeded."""
    lock1 = r.lock("mylock", timeout=10)  # Long TTL -- won't expire
    await lock1.acquire()

    lock2 = r.lock("mylock", timeout=10, blocking_timeout=0.2)
    result = await lock2.acquire()
    assert result is False

    await lock1.release()


async def test_lock_nonblocking_fail(r):
    """LOCK-02: Non-blocking acquire returns False when lock held."""
    lock1 = r.lock("mylock", timeout=10)
    await lock1.acquire()

    lock2 = r.lock("mylock", timeout=10, blocking=False)
    result = await lock2.acquire()
    assert result is False

    await lock1.release()


async def test_lock_nonblocking_success(r):
    """LOCK-02: Non-blocking acquire returns True when lock available."""
    lock = r.lock("mylock", timeout=10, blocking=False)
    result = await lock.acquire()
    assert result is True

    await lock.release()


async def test_lock_token_uniqueness(r):
    """LOCK-02: Different acquisitions produce different tokens."""
    lock1 = r.lock("mylock", timeout=10)
    await lock1.acquire()
    token1 = lock1.token
    await lock1.release()

    lock2 = r.lock("mylock", timeout=10)
    await lock2.acquire()
    token2 = lock2.token

    assert token1 != token2

    await lock2.release()


async def test_lock_sleep_interval(r):
    """LOCK-02: Lock sleep attribute is configurable."""
    lock = r.lock("mylock", timeout=10, sleep=0.05)
    assert lock.sleep == 0.05


# --- Async context manager ---


async def test_lock_context_manager(r):
    """Context manager acquires on enter and releases on exit."""
    async with r.lock("mylock", timeout=10) as lock:
        # Inside context, lock should be held
        assert lock.token is not None
        assert await r.get("mylock") is not None

    # After context exit, lock should be released
    assert await r.get("mylock") is None


async def test_lock_context_manager_releases_on_exception(r):
    """Context manager releases lock even when exception occurs."""
    with pytest.raises(ValueError):
        async with r.lock("mylock", timeout=10) as lock:
            raise ValueError("test error")

    # Lock should still be released
    assert await r.get("mylock") is None


async def test_lock_context_manager_acquire_fails(r):
    """Context manager raises LockError when non-blocking acquire fails."""
    lock1 = r.lock("mylock", timeout=10)
    await lock1.acquire()

    with pytest.raises(LockError):
        async with r.lock("mylock", timeout=10, blocking=False) as lock2:
            pass  # Should not reach here

    await lock1.release()


# --- Edge cases ---


async def test_lock_different_names_independent(r):
    """Locks on different names are independent."""
    lock1 = r.lock("lock1", timeout=10)
    lock2 = r.lock("lock2", timeout=10)

    assert await lock1.acquire() is True
    assert await lock2.acquire() is True

    await lock1.release()
    await lock2.release()


# ---- LockError Hierarchy Tests (D-06) ----


def test_lock_error_hierarchy():
    """LockError is subclass of redis.exceptions.LockError when redis is installed."""
    import redis.exceptions
    assert issubclass(LockError, redis.exceptions.LockError)
