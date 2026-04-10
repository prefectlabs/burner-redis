"""Tests for set commands: SADD, SMEMBERS, SISMEMBER, SREM.

Covers requirements: SET-01, SET-02, SET-03, SET-04.
"""
import pytest
from burner_redis import BurnerRedis


# --- SET-01: SADD ---


async def test_sadd_new_members(r):
    """SET-01: SADD adds new members and returns count added."""
    result = await r.sadd("s", "a", "b", "c")
    assert result == 3


async def test_sadd_duplicate_members(r):
    """SET-01: SADD returns 0 when adding members that already exist."""
    await r.sadd("s", "a", "b")
    result = await r.sadd("s", "a", "b")
    assert result == 0


async def test_sadd_mixed_new_and_existing(r):
    """SET-01: SADD returns count of only NEW members added."""
    await r.sadd("s", "a", "b")
    result = await r.sadd("s", "b", "c", "d")
    assert result == 2  # only c and d are new


async def test_sadd_bytes_input(r):
    """SET-01: SADD works with bytes values."""
    result = await r.sadd(b"s", b"a", b"b")
    assert result == 2

    members = await r.smembers(b"s")
    assert members == {b"a", b"b"}


async def test_sadd_wrongtype(r):
    """SET-01: SADD on a string key raises WRONGTYPE error."""
    await r.set("strkey", "value")
    with pytest.raises(Exception, match="WRONGTYPE"):
        await r.sadd("strkey", "member")


# --- SET-02: SMEMBERS ---


async def test_smembers_existing_set(r):
    """SET-02: SMEMBERS returns set of all members as bytes."""
    await r.sadd("s", "a", "b", "c")
    members = await r.smembers("s")
    assert members == {b"a", b"b", b"c"}


async def test_smembers_empty_key(r):
    """SET-02: SMEMBERS returns empty set for non-existent key."""
    members = await r.smembers("no_key")
    assert members == set()


async def test_smembers_returns_set_type(r):
    """SET-02: SMEMBERS return type is Python set."""
    await r.sadd("s", "a")
    members = await r.smembers("s")
    assert isinstance(members, set)


async def test_smembers_wrongtype(r):
    """SET-02: SMEMBERS on a string key raises WRONGTYPE error."""
    await r.set("strkey", "value")
    with pytest.raises(Exception, match="WRONGTYPE"):
        await r.smembers("strkey")


# --- SET-03: SISMEMBER ---


async def test_sismember_exists(r):
    """SET-03: SISMEMBER returns True for an existing member."""
    await r.sadd("s", "a", "b")
    result = await r.sismember("s", "a")
    assert result is True


async def test_sismember_not_exists(r):
    """SET-03: SISMEMBER returns False for a non-existing member."""
    await r.sadd("s", "a", "b")
    result = await r.sismember("s", "z")
    assert result is False


async def test_sismember_missing_key(r):
    """SET-03: SISMEMBER returns False for a non-existent key."""
    result = await r.sismember("no_key", "a")
    assert result is False


async def test_sismember_wrongtype(r):
    """SET-03: SISMEMBER on a string key raises WRONGTYPE error."""
    await r.set("strkey", "value")
    with pytest.raises(Exception, match="WRONGTYPE"):
        await r.sismember("strkey", "member")


# --- SET-04: SREM ---


async def test_srem_existing_members(r):
    """SET-04: SREM removes members and returns count removed."""
    await r.sadd("s", "a", "b", "c")
    count = await r.srem("s", "a", "c")
    assert count == 2

    # Only b remains
    members = await r.smembers("s")
    assert members == {b"b"}


async def test_srem_nonexistent_members(r):
    """SET-04: SREM returns 0 for members not in the set."""
    await r.sadd("s", "a")
    count = await r.srem("s", "x", "y")
    assert count == 0


async def test_srem_missing_key(r):
    """SET-04: SREM on a non-existent key returns 0."""
    count = await r.srem("no_key", "a")
    assert count == 0


async def test_srem_wrongtype(r):
    """SET-04: SREM on a string key raises WRONGTYPE error."""
    await r.set("strkey", "value")
    with pytest.raises(Exception, match="WRONGTYPE"):
        await r.srem("strkey", "member")
