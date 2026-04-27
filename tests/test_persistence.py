"""Tests for persistence: save/restore, atexit, corrupt file handling.

Covers requirements: PERS-01, PERS-02, PERS-03, PERS-04.
"""
import asyncio
import os
import tempfile

import pytest
from burner_redis import BurnerRedis


@pytest.fixture
def tmp_path():
    """Create a temporary file path for persistence tests."""
    with tempfile.TemporaryDirectory() as d:
        yield os.path.join(d, "test.dat")


@pytest.mark.asyncio
async def test_save_and_restore(tmp_path):
    """PERS-01, PERS-03: Manual save persists data, new instance restores it."""
    client = BurnerRedis(persistence_path=tmp_path)
    await client.set("key1", "value1")
    await client.hset("hash1", mapping={"f1": "v1"})
    await client.save()

    # New instance should restore data
    client2 = BurnerRedis(persistence_path=tmp_path)
    result = await client2.get("key1")
    assert result == b"value1"
    result = await client2.hget("hash1", "f1")
    assert result == b"v1"


@pytest.mark.asyncio
async def test_list_persistence(tmp_path):
    """PERS-01..04 / LIST-01..16 / Phase 15 ISSUE-3 regression: ValueData::List round-trips
    through save/restore with order preserved.
    """
    client = BurnerRedis(persistence_path=tmp_path)
    await client.rpush("list1", "a", "b", "c")
    await client.save()

    # New instance with the same persistence_path should restore the list.
    client2 = BurnerRedis(persistence_path=tmp_path)
    result = await client2.lrange("list1", 0, -1)
    assert result == [b"a", b"b", b"c"]

    # Verify length is also correct.
    length = await client2.llen("list1")
    assert length == 3


@pytest.mark.asyncio
async def test_save_with_explicit_path(tmp_path):
    """PERS-01: save() with explicit path argument."""
    client = BurnerRedis()
    await client.set("x", "y")
    await client.save(path=tmp_path)
    assert os.path.exists(tmp_path)


@pytest.mark.asyncio
async def test_restore_missing_file(tmp_path):
    """PERS-03: Missing file starts empty without error."""
    client = BurnerRedis(persistence_path=tmp_path)
    result = await client.get("anything")
    assert result is None


@pytest.mark.asyncio
async def test_restore_corrupt_file(tmp_path):
    """PERS-03: Corrupt file starts empty with warning."""
    with open(tmp_path, "wb") as f:
        f.write(b"not valid msgpack data")
    client = BurnerRedis(persistence_path=tmp_path)
    result = await client.get("anything")
    assert result is None


@pytest.mark.asyncio
async def test_crash_safe_no_tmp_file(tmp_path):
    """PERS-04: No .tmp file remains after successful save."""
    client = BurnerRedis(persistence_path=tmp_path)
    await client.set("k", "v")
    await client.save()
    assert not os.path.exists(tmp_path + ".tmp")
    assert os.path.exists(tmp_path)


@pytest.mark.asyncio
async def test_expired_keys_not_persisted(tmp_path):
    """Keys that are expired at save time should not be restored."""
    client = BurnerRedis(persistence_path=tmp_path)
    await client.set("ephemeral", "gone", px=1)  # 1ms TTL
    await client.set("permanent", "stays")
    await asyncio.sleep(0.01)  # Wait for expiry
    await client.save()

    client2 = BurnerRedis(persistence_path=tmp_path)
    assert await client2.get("ephemeral") is None
    assert await client2.get("permanent") == b"stays"


@pytest.mark.asyncio
async def test_sorted_set_persistence(tmp_path):
    """Sorted sets survive save/restore."""
    client = BurnerRedis(persistence_path=tmp_path)
    await client.zadd("zs", {"a": 1.0, "b": 2.0, "c": 3.0})
    await client.save()

    client2 = BurnerRedis(persistence_path=tmp_path)
    result = await client2.zrange("zs", 0, -1)
    assert result == [b"a", b"b", b"c"]


@pytest.mark.asyncio
async def test_stream_persistence(tmp_path):
    """Streams survive save/restore."""
    client = BurnerRedis(persistence_path=tmp_path)
    await client.xadd("stream", {"field": "value"})
    await client.save()

    client2 = BurnerRedis(persistence_path=tmp_path)
    length = await client2.xlen("stream")
    assert length == 1


@pytest.mark.asyncio
async def test_script_cache_persistence(tmp_path):
    """PERS-01: Script cache persists across restarts."""
    client = BurnerRedis(persistence_path=tmp_path)
    sha = await client.script_load("return 42")
    await client.save()

    client2 = BurnerRedis(persistence_path=tmp_path)
    exists = await client2.script_exists(sha)
    assert exists == [True]


@pytest.mark.asyncio
async def test_atexit_persistence(tmp_path):
    """PERS-02: atexit handler saves on shutdown."""
    # This test verifies the _save_sync method works (atexit calls it)
    client = BurnerRedis(persistence_path=tmp_path)
    await client.set("atexit_key", "atexit_value")
    # Simulate atexit by calling _save_sync directly
    client._save_sync()
    assert os.path.exists(tmp_path)

    client2 = BurnerRedis(persistence_path=tmp_path)
    result = await client2.get("atexit_key")
    assert result == b"atexit_value"


@pytest.mark.asyncio
async def test_persistence_path_property(tmp_path):
    """persistence_path property returns configured path or None."""
    client_with = BurnerRedis(persistence_path=tmp_path)
    assert client_with.persistence_path == tmp_path

    client_without = BurnerRedis()
    assert client_without.persistence_path is None


@pytest.mark.asyncio
async def test_set_persistence(tmp_path):
    """Sets survive save/restore."""
    client = BurnerRedis(persistence_path=tmp_path)
    await client.sadd("myset", "a", "b", "c")
    await client.save()

    client2 = BurnerRedis(persistence_path=tmp_path)
    members = await client2.smembers("myset")
    assert members == {b"a", b"b", b"c"}
