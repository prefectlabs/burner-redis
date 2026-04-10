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
