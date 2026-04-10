"""Tests for hash commands: HSET, HGET, HDEL, HVALS.

Covers requirements: HASH-01, HASH-02, HASH-03, HASH-04.
"""
import pytest
from burner_redis import BurnerRedis


# --- HASH-01: HSET ---


async def test_hset_single_field(r):
    """HASH-01: HSET with a single key/value pair returns 1 and value is retrievable."""
    result = await r.hset("h", "field1", "value1")
    assert result == 1

    value = await r.hget("h", "field1")
    assert value == b"value1"


async def test_hset_mapping(r):
    """HASH-01: HSET with mapping dict sets multiple fields, returns count of new fields."""
    result = await r.hset("h", mapping={"f1": "v1", "f2": "v2"})
    assert result == 2


async def test_hset_key_value_and_mapping(r):
    """HASH-01: HSET with both key/value and mapping combines all fields."""
    result = await r.hset("h", "f1", "v1", mapping={"f2": "v2", "f3": "v3"})
    assert result == 3

    assert await r.hget("h", "f1") == b"v1"
    assert await r.hget("h", "f2") == b"v2"
    assert await r.hget("h", "f3") == b"v3"


async def test_hset_update_existing_field(r):
    """HASH-01: HSET returns 0 when updating an existing field (not new)."""
    await r.hset("h", "field1", "value1")
    result = await r.hset("h", "field1", "value_updated")
    assert result == 0

    # Value is updated
    value = await r.hget("h", "field1")
    assert value == b"value_updated"


async def test_hset_bytes_input(r):
    """HASH-01: HSET works with bytes keys and values."""
    result = await r.hset(b"h", b"field1", b"value1")
    assert result == 1

    value = await r.hget(b"h", b"field1")
    assert value == b"value1"


async def test_hset_wrongtype(r):
    """HASH-01: HSET on a string key raises WRONGTYPE error."""
    await r.set("strkey", "value")
    with pytest.raises(Exception, match="WRONGTYPE"):
        await r.hset("strkey", "field", "value")


# --- HASH-02: HGET ---


async def test_hget_existing_field(r):
    """HASH-02: HGET returns bytes value for an existing field."""
    await r.hset("h", "field1", "value1")
    value = await r.hget("h", "field1")
    assert value == b"value1"
    assert isinstance(value, bytes)


async def test_hget_missing_field(r):
    """HASH-02: HGET returns None for a field that doesn't exist."""
    await r.hset("h", "field1", "value1")
    value = await r.hget("h", "field_missing")
    assert value is None


async def test_hget_missing_key(r):
    """HASH-02: HGET returns None for a key that doesn't exist."""
    value = await r.hget("no_such_key", "field1")
    assert value is None


async def test_hget_wrongtype(r):
    """HASH-02: HGET on a string key raises WRONGTYPE error."""
    await r.set("strkey", "value")
    with pytest.raises(Exception, match="WRONGTYPE"):
        await r.hget("strkey", "field")


# --- HASH-03: HDEL ---


async def test_hdel_existing_fields(r):
    """HASH-03: HDEL removes specified fields and returns count deleted."""
    await r.hset("h", mapping={"f1": "v1", "f2": "v2", "f3": "v3"})
    count = await r.hdel("h", "f1", "f3")
    assert count == 2

    # f2 still exists
    assert await r.hget("h", "f2") == b"v2"
    # f1 and f3 are gone
    assert await r.hget("h", "f1") is None
    assert await r.hget("h", "f3") is None


async def test_hdel_missing_fields(r):
    """HASH-03: HDEL returns 0 when deleting non-existent fields."""
    await r.hset("h", "f1", "v1")
    count = await r.hdel("h", "f_missing1", "f_missing2")
    assert count == 0


async def test_hdel_missing_key(r):
    """HASH-03: HDEL on a non-existent key returns 0."""
    count = await r.hdel("no_key", "f1")
    assert count == 0


async def test_hdel_wrongtype(r):
    """HASH-03: HDEL on a string key raises WRONGTYPE error."""
    await r.set("strkey", "value")
    with pytest.raises(Exception, match="WRONGTYPE"):
        await r.hdel("strkey", "field")


# --- HASH-04: HVALS ---


async def test_hvals_existing_hash(r):
    """HASH-04: HVALS returns all values as a list of bytes."""
    await r.hset("h", mapping={"f1": "v1", "f2": "v2", "f3": "v3"})
    vals = await r.hvals("h")
    assert sorted(vals) == [b"v1", b"v2", b"v3"]


async def test_hvals_empty_key(r):
    """HASH-04: HVALS returns empty list for non-existent key."""
    vals = await r.hvals("no_key")
    assert vals == []


async def test_hvals_returns_list_type(r):
    """HASH-04: HVALS return type is list."""
    await r.hset("h", "f1", "v1")
    vals = await r.hvals("h")
    assert isinstance(vals, list)


async def test_hvals_wrongtype(r):
    """HASH-04: HVALS on a string key raises WRONGTYPE error."""
    await r.set("strkey", "value")
    with pytest.raises(Exception, match="WRONGTYPE"):
        await r.hvals("strkey")
