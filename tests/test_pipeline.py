"""Tests for Pipeline batched command execution.

Covers requirements: PIPE-01, PIPE-02, PIPE-03.
"""
import pytest
from burner_redis import BurnerRedis
from burner_redis.pipeline import Pipeline


# --- PIPE-01: Pipeline creation and command queuing ---


async def test_pipeline_creation(r):
    """PIPE-01: r.pipeline() returns a Pipeline instance."""
    pipe = r.pipeline()
    assert isinstance(pipe, Pipeline)


async def test_pipeline_queue_and_execute(r):
    """PIPE-01: Pipeline buffers commands and execute() returns results list."""
    pipe = r.pipeline()
    pipe.set("k1", "v1")
    pipe.set("k2", "v2")
    results = await pipe.execute()
    assert results == [True, True]


async def test_pipeline_get_after_set(r):
    """PIPE-01: Pipeline set then get returns correct results."""
    pipe = r.pipeline()
    pipe.set("k", "v")
    pipe.get("k")
    results = await pipe.execute()
    assert results == [True, b"v"]


async def test_pipeline_mixed_commands(r):
    """PIPE-01: Pipeline with set, get, delete, exists returns correct types."""
    pipe = r.pipeline()
    pipe.set("key", "val")
    pipe.get("key")
    pipe.exists("key")
    pipe.delete("key")
    pipe.exists("key")
    results = await pipe.execute()
    assert results[0] is True        # set returns True
    assert results[1] == b"val"      # get returns bytes
    assert results[2] == 1           # exists returns count
    assert results[3] == 1           # delete returns count
    assert results[4] == 0           # exists after delete returns 0


async def test_pipeline_hash_commands(r):
    """PIPE-01: Pipeline hset, hget, hvals returns correct results."""
    pipe = r.pipeline()
    pipe.hset("myhash", key="f1", value="v1")
    pipe.hset("myhash", key="f2", value="v2")
    pipe.hget("myhash", "f1")
    pipe.hvals("myhash")
    results = await pipe.execute()
    assert results[0] == 1           # hset returns count of new fields
    assert results[1] == 1           # hset second field
    assert results[2] == b"v1"       # hget returns bytes
    assert set(results[3]) == {b"v1", b"v2"}  # hvals returns list of values


async def test_pipeline_set_commands(r):
    """PIPE-01: Pipeline sadd, sismember, smembers returns correct results."""
    pipe = r.pipeline()
    pipe.sadd("myset", "a", "b", "c")
    pipe.sismember("myset", "a")
    pipe.smembers("myset")
    results = await pipe.execute()
    assert results[0] == 3           # sadd returns count of new members
    assert results[1] is True        # sismember returns bool
    assert results[2] == {b"a", b"b", b"c"}  # smembers returns set


async def test_pipeline_sorted_set_commands(r):
    """PIPE-01: Pipeline zadd, zrange returns correct results."""
    pipe = r.pipeline()
    pipe.zadd("zset", {"a": 1.0, "b": 2.0, "c": 3.0})
    pipe.zrange("zset", 0, -1)
    results = await pipe.execute()
    assert results[0] == 3           # zadd returns count of new members
    assert results[1] == [b"a", b"b", b"c"]  # zrange returns ordered list


async def test_pipeline_stream_commands(r):
    """PIPE-01: Pipeline xadd, xlen returns correct results."""
    pipe = r.pipeline()
    pipe.xadd("stream", {"field": "value"})
    pipe.xlen("stream")
    results = await pipe.execute()
    assert isinstance(results[0], bytes)  # xadd returns stream ID as bytes
    assert results[1] == 1               # xlen returns entry count


async def test_pipeline_scripting_commands(r):
    """PIPE-01: Pipeline eval, script_load returns correct results."""
    pipe = r.pipeline()
    pipe.eval("return 42", 0)
    pipe.script_load("return 1")
    results = await pipe.execute()
    assert results[0] == 42              # eval returns Lua result
    assert isinstance(results[1], str)   # script_load returns SHA1 string


async def test_pipeline_clears_after_execute(r):
    """PIPE-01: After execute(), command buffer is cleared."""
    pipe = r.pipeline()
    pipe.set("k", "v")
    await pipe.execute()
    results = await pipe.execute()
    assert results == []


async def test_pipeline_empty_execute(r):
    """PIPE-01: Empty pipeline execute returns empty list."""
    pipe = r.pipeline()
    results = await pipe.execute()
    assert results == []


# --- PIPE-02: Results in command order ---


async def test_pipeline_result_order(r):
    """PIPE-02: Results are returned in the exact order commands were queued."""
    pipe = r.pipeline()
    pipe.set("x", "1")
    pipe.set("y", "2")
    pipe.get("x")
    pipe.get("y")
    pipe.exists("x", "y")
    results = await pipe.execute()
    assert results[0] is True      # set x
    assert results[1] is True      # set y
    assert results[2] == b"1"      # get x
    assert results[3] == b"2"      # get y
    assert results[4] == 2         # exists count


async def test_pipeline_preserves_none(r):
    """PIPE-02: Pipeline correctly returns None for missing keys."""
    pipe = r.pipeline()
    pipe.get("nonexistent")
    results = await pipe.execute()
    assert results == [None]


# --- PIPE-03: Async context manager ---


async def test_pipeline_context_manager(r):
    """PIPE-03: Async context manager auto-executes commands on clean exit."""
    async with r.pipeline() as pipe:
        pipe.set("k", "v")
        pipe.set("k2", "v2")
    # Commands should have been executed when exiting the block
    assert await r.get("k") == b"v"
    assert await r.get("k2") == b"v2"


async def test_pipeline_context_manager_with_explicit_execute(r):
    """PIPE-03: Explicit execute inside context manager clears buffer."""
    async with r.pipeline() as pipe:
        pipe.set("k", "v")
        results = await pipe.execute()
        assert results == [True]
    # After the block, the implicit __aexit__ execute runs on empty buffer
    assert await r.get("k") == b"v"


async def test_pipeline_context_manager_exception(r):
    """PIPE-03: Commands are NOT executed when exception occurs in context."""
    with pytest.raises(ValueError):
        async with r.pipeline() as pipe:
            pipe.set("should_not_exist", "val")
            raise ValueError("test error")
    # The key should not have been set since __aexit__ skips on exception
    assert await r.get("should_not_exist") is None


# --- Method chaining ---


async def test_pipeline_method_chaining(r):
    """Each pipeline command method returns self for chaining."""
    pipe = r.pipeline()
    result = pipe.set("k1", "v1").set("k2", "v2").get("k1")
    assert result is pipe
    results = await pipe.execute()
    assert results == [True, True, b"v1"]


# --- Error handling ---


async def test_pipeline_wrongtype_error(r):
    """Pipeline propagates WRONGTYPE errors from individual commands."""
    await r.set("str_key", "value")
    pipe = r.pipeline()
    pipe.hset("str_key", key="field", value="val")
    with pytest.raises(Exception, match="WRONGTYPE"):
        await pipe.execute()


# ---- Pipeline Stubs for Phase 12 Commands (D-09) ----


async def test_pipeline_keys(r):
    """Pipeline keys() command works."""
    await r.set("pk1", "v1")
    await r.set("pk2", "v2")
    pipe = r.pipeline()
    pipe.keys("pk*")
    results = await pipe.execute()
    assert set(results[0]) == {b"pk1", b"pk2"}


async def test_pipeline_ttl(r):
    """Pipeline ttl() command works."""
    await r.set("pt", "v", ex=60)
    pipe = r.pipeline()
    pipe.ttl("pt")
    pipe.ttl("nonexistent")
    results = await pipe.execute()
    assert 0 < results[0] <= 60
    assert results[1] == -2


async def test_pipeline_mget(r):
    """Pipeline mget() command works."""
    await r.set("pm1", "a")
    await r.set("pm2", "b")
    pipe = r.pipeline()
    pipe.mget("pm1", "pm2", "pm3")
    results = await pipe.execute()
    assert results[0] == [b"a", b"b", None]


async def test_pipeline_setex(r):
    """Pipeline setex() command works."""
    pipe = r.pipeline()
    pipe.setex("pse", 60, "val")
    await pipe.execute()
    result = await r.get("pse")
    assert result == b"val"


async def test_pipeline_xpending(r):
    """Pipeline xpending() summary command works."""
    await r.xadd("pstream", {"f": "v"})
    await r.xgroup_create("pstream", "pgroup", id="0")
    await r.xreadgroup("pgroup", "pc1", {"pstream": ">"}, count=1)
    pipe = r.pipeline()
    pipe.xpending("pstream", "pgroup")
    results = await pipe.execute()
    assert results[0]["pending"] == 1
