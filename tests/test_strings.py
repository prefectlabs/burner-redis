"""Tests for string commands: SET, GET, DELETE, EXISTS.

Covers requirements: FOUND-01, FOUND-02, FOUND-03, STR-01 through STR-06.
"""
import asyncio
from datetime import timedelta

import pytest
import redis.exceptions
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


# ---- Value Coercion Tests (D-01, D-02) ----


async def test_set_coercion_int(r):
    """set() accepts integer values and coerces to string bytes."""
    await r.set("counter", 42)
    result = await r.get("counter")
    assert result == b"42"


async def test_set_coercion_int_with_flags(r):
    """set() coercion works with NX/PX flags (docket's exact pattern)."""
    result = await r.set("cooldown", 1, nx=True, px=5000)
    assert result is True
    val = await r.get("cooldown")
    assert val == b"1"


async def test_set_coercion_float(r):
    """set() accepts float values and coerces to string bytes."""
    await r.set("ratio", 3.14)
    result = await r.get("ratio")
    assert result == b"3.14"


async def test_set_coercion_bool_rejected(r):
    """set() rejects boolean values matching redis-py behavior."""
    with pytest.raises(TypeError, match="bool"):
        await r.set("flag", True)


async def test_set_coercion_bool_false_rejected(r):
    """set() rejects False as well as True."""
    with pytest.raises(TypeError, match="bool"):
        await r.set("flag", False)


async def test_set_coercion_bytes_passthrough(r):
    """set() passes bytes through without coercion."""
    await r.set("key", b"raw bytes")
    result = await r.get("key")
    assert result == b"raw bytes"


async def test_set_coercion_str_passthrough(r):
    """set() passes str through without coercion."""
    await r.set("key", "hello")
    result = await r.get("key")
    assert result == b"hello"


# ---- keys() Tests (D-03, D-04) ----


async def test_keys_all(r):
    """keys('*') returns all keys."""
    await r.set("a", "1")
    await r.set("b", "2")
    await r.set("c", "3")
    result = await r.keys("*")
    assert set(result) == {b"a", b"b", b"c"}


async def test_keys_pattern(r):
    """keys(pattern) filters by glob pattern."""
    await r.set("user:1", "alice")
    await r.set("user:2", "bob")
    await r.set("item:1", "widget")
    result = await r.keys("user:*")
    assert set(result) == {b"user:1", b"user:2"}


async def test_keys_no_match(r):
    """keys() returns empty list when no keys match."""
    await r.set("foo", "bar")
    result = await r.keys("nonexistent:*")
    assert result == []


async def test_keys_default_pattern(r):
    """keys() with no args defaults to '*'."""
    await r.set("x", "1")
    result = await r.keys()
    assert b"x" in result


async def test_keys_char_range(r):
    """keys() supports [a-z] character range patterns."""
    await r.set("key_a", "1")
    await r.set("key_b", "2")
    await r.set("key_1", "3")
    result = await r.keys("key_[a-z]")
    assert set(result) == {b"key_a", b"key_b"}


# ---- scan_iter() Tests (D-05) ----


async def test_scan_iter_all(r):
    """scan_iter() yields all keys."""
    await r.set("s1", "v1")
    await r.set("s2", "v2")
    keys = []
    async for key in r.scan_iter():
        keys.append(key)
    assert set(keys) == {b"s1", b"s2"}


async def test_scan_iter_pattern(r):
    """scan_iter(match=pattern) yields only matching keys."""
    await r.set("app:1", "v1")
    await r.set("app:2", "v2")
    await r.set("other", "v3")
    keys = []
    async for key in r.scan_iter(match="app:*"):
        keys.append(key)
    assert set(keys) == {b"app:1", b"app:2"}


# ---- setex() Tests (D-12) ----


async def test_setex_basic(r):
    """setex() stores a value retrievable via get()."""
    await r.setex("mykey", 60, "myvalue")
    result = await r.get("mykey")
    assert result == b"myvalue"


async def test_setex_with_ttl(r):
    """setex() sets a TTL on the key."""
    await r.setex("expiring", 30, "data")
    ttl_val = await r.ttl("expiring")
    assert 0 < ttl_val <= 30


async def test_setex_coercion(r):
    """setex() applies value coercion like set()."""
    await r.setex("num", 60, 123)
    result = await r.get("num")
    assert result == b"123"


# ---- mget() Tests (D-13) ----


async def test_mget_basic(r):
    """mget() returns values for multiple keys."""
    await r.set("k1", "v1")
    await r.set("k2", "v2")
    result = await r.mget("k1", "k2")
    assert result == [b"v1", b"v2"]


async def test_mget_missing_keys(r):
    """mget() returns None for missing keys."""
    await r.set("k1", "v1")
    result = await r.mget("k1", "missing", "k1")
    assert result == [b"v1", None, b"v1"]


async def test_mget_all_missing(r):
    """mget() returns all None for nonexistent keys."""
    result = await r.mget("a", "b", "c")
    assert result == [None, None, None]


# --- P2-FIX: GET WRONGTYPE on non-string keys ---


async def test_get_on_list_key_raises_wrongtype(r):
    """P2-FIX: GET on a list-typed key raises WRONGTYPE (matches real Redis)."""
    await r.lpush("k", "v")
    with pytest.raises(redis.exceptions.ResponseError, match="WRONGTYPE"):
        await r.get("k")


async def test_get_on_hash_key_raises_wrongtype(r):
    """P2-FIX: GET on a hash-typed key raises WRONGTYPE."""
    await r.hset("h", "f", "v")
    with pytest.raises(redis.exceptions.ResponseError, match="WRONGTYPE"):
        await r.get("h")


async def test_get_on_set_key_raises_wrongtype(r):
    """P2-FIX: GET on a set-typed key raises WRONGTYPE."""
    await r.sadd("s", "m")
    with pytest.raises(redis.exceptions.ResponseError, match="WRONGTYPE"):
        await r.get("s")


async def test_get_on_sorted_set_key_raises_wrongtype(r):
    """P2-FIX: GET on a sorted-set-typed key raises WRONGTYPE."""
    await r.zadd("z", {"m": 1.0})
    with pytest.raises(redis.exceptions.ResponseError, match="WRONGTYPE"):
        await r.get("z")


async def test_get_on_stream_key_raises_wrongtype(r):
    """P2-FIX: GET on a stream-typed key raises WRONGTYPE."""
    await r.xadd("x", {"f": "v"})
    with pytest.raises(redis.exceptions.ResponseError, match="WRONGTYPE"):
        await r.get("x")
