"""Tests for value coercion, exception hierarchy, scan_iter, and setex.

Covers Phase 12 requirements: D-01, D-02, D-05, D-06, D-12.
"""
import pytest
from burner_redis import BurnerRedis, LockError


# ---- Value Coercion Tests (D-01, D-02) ----


async def test_coerce_value_int(r):
    """_coerce_value(42) returns b'42'."""
    from burner_redis import _coerce_value
    assert _coerce_value(42) == b"42"


async def test_coerce_value_float(r):
    """_coerce_value(3.14) returns b'3.14'."""
    from burner_redis import _coerce_value
    assert _coerce_value(3.14) == b"3.14"


async def test_coerce_value_bool_rejected(r):
    """_coerce_value(True) raises TypeError."""
    from burner_redis import _coerce_value
    with pytest.raises(TypeError, match="bool"):
        _coerce_value(True)


async def test_coerce_value_bool_false_rejected(r):
    """_coerce_value(False) raises TypeError."""
    from burner_redis import _coerce_value
    with pytest.raises(TypeError, match="bool"):
        _coerce_value(False)


async def test_coerce_value_bytes_passthrough(r):
    """_coerce_value(b'hello') returns b'hello'."""
    from burner_redis import _coerce_value
    assert _coerce_value(b"hello") == b"hello"


async def test_coerce_value_str_passthrough(r):
    """_coerce_value('hello') returns 'hello'."""
    from burner_redis import _coerce_value
    assert _coerce_value("hello") == "hello"


async def test_coerce_value_memoryview_passthrough(r):
    """_coerce_value(memoryview(b'hi')) returns memoryview."""
    from burner_redis import _coerce_value
    mv = memoryview(b"hi")
    assert _coerce_value(mv) is mv


async def test_set_coercion_int(r):
    """set() accepts integer values and coerces to string bytes."""
    await r.set("counter", 42)
    result = await r.get("counter")
    assert result == b"42"


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


# ---- LockError Hierarchy Tests (D-06) ----


def test_lock_error_hierarchy():
    """LockError is subclass of redis.exceptions.LockError when redis is installed."""
    import redis.exceptions
    assert issubclass(LockError, redis.exceptions.LockError)


# ---- scan_iter Tests (D-05) ----


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


# ---- setex Tests (D-12) ----


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
