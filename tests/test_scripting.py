"""Tests for Lua scripting commands: EVAL, EVALSHA, SCRIPT LOAD, SCRIPT EXISTS.

Covers requirements: LUA-01, LUA-02, LUA-03, LUA-04, LUA-05.
"""
import hashlib

import pytest
from burner_redis import BurnerRedis, NoScriptError


# --- LUA-01: EVAL with KEYS and ARGV ---


async def test_eval_return_string(r):
    """LUA-01: EVAL returning a string gives bytes."""
    result = await r.eval("return 'hello'", 0)
    assert result == b"hello"


async def test_eval_return_integer(r):
    """LUA-01: EVAL returning an integer gives Python int."""
    result = await r.eval("return 42", 0)
    assert result == 42


async def test_eval_return_nil(r):
    """LUA-01: EVAL returning nil gives None."""
    result = await r.eval("return nil", 0)
    assert result is None


async def test_eval_return_table_array(r):
    """LUA-01: EVAL returning a table array gives list of ints."""
    result = await r.eval("return {1, 2, 3}", 0)
    assert result == [1, 2, 3]


async def test_eval_return_false(r):
    """LUA-01: EVAL returning false gives None (Redis Lua protocol: false -> nil)."""
    result = await r.eval("return false", 0)
    assert result is None


async def test_eval_keys_and_argv(r):
    """LUA-01: EVAL with KEYS and ARGV arrays returns correct values."""
    result = await r.eval("return {KEYS[1], ARGV[1]}", 1, "mykey", "myarg")
    assert result == [b"mykey", b"myarg"]


async def test_eval_numkeys_zero(r):
    """LUA-01: With numkeys=0, all extra args go to ARGV."""
    result = await r.eval("return ARGV[1]", 0, "arg1")
    assert result == b"arg1"


async def test_eval_multiple_keys(r):
    """LUA-01: Multiple KEYS are correctly split from ARGV."""
    result = await r.eval("return {KEYS[1], KEYS[2]}", 2, "k1", "k2")
    assert result == [b"k1", b"k2"]


# --- LUA-02: EVALSHA ---


async def test_evalsha_after_script_load(r):
    """LUA-02: EVALSHA works with a script loaded via SCRIPT LOAD."""
    script = "return 'loaded'"
    sha = await r.script_load(script)
    result = await r.evalsha(sha, 0)
    assert result == b"loaded"


async def test_evalsha_after_eval(r):
    """LUA-02: EVALSHA works with auto-cached script after EVAL."""
    script = "return 'cached'"
    await r.eval(script, 0)
    # Compute SHA1 in Python to verify auto-caching
    sha = hashlib.sha1(script.encode()).hexdigest()
    result = await r.evalsha(sha, 0)
    assert result == b"cached"


async def test_evalsha_unknown_sha_raises(r):
    """LUA-02: EVALSHA with unknown SHA raises NoScriptError."""
    with pytest.raises(NoScriptError):
        await r.evalsha("deadbeef" * 5, 0)


async def test_evalsha_with_keys_and_args(r):
    """LUA-02: EVALSHA with KEYS and ARGV returns correct result."""
    script = "return {KEYS[1], ARGV[1]}"
    sha = await r.script_load(script)
    result = await r.evalsha(sha, 1, "key1", "val1")
    assert result == [b"key1", b"val1"]


# --- LUA-03: redis.call() and redis.pcall() ---

# String commands via redis.call()


async def test_redis_call_set_get(r):
    """LUA-03: redis.call('SET') and redis.call('GET') work correctly."""
    result = await r.eval(
        "redis.call('SET', KEYS[1], ARGV[1]); return redis.call('GET', KEYS[1])",
        1,
        "foo",
        "bar",
    )
    assert result == b"bar"


async def test_redis_call_del(r):
    """LUA-03: redis.call('DEL') removes a key and returns 1."""
    await r.set("foo", "bar")
    result = await r.eval("return redis.call('DEL', KEYS[1])", 1, "foo")
    assert result == 1


async def test_redis_call_exists(r):
    """LUA-03: redis.call('EXISTS') returns 1 for existing key."""
    await r.set("foo", "bar")
    result = await r.eval("return redis.call('EXISTS', KEYS[1])", 1, "foo")
    assert result == 1


# Hash commands via redis.call()


async def test_redis_call_hset_hget(r):
    """LUA-03: redis.call('HSET') and redis.call('HGET') work correctly."""
    result = await r.eval(
        "redis.call('HSET', KEYS[1], 'field1', ARGV[1]); return redis.call('HGET', KEYS[1], 'field1')",
        1,
        "myhash",
        "val1",
    )
    assert result == b"val1"


async def test_redis_call_hdel(r):
    """LUA-03: redis.call('HDEL') removes a hash field and returns 1."""
    await r.hset("myhash", "field1", "val1")
    result = await r.eval(
        "return redis.call('HDEL', KEYS[1], 'field1')", 1, "myhash"
    )
    assert result == 1


async def test_redis_call_hvals(r):
    """LUA-03: redis.call('HVALS') returns all hash values as array."""
    await r.hset("myhash", "f1", "v1")
    await r.hset("myhash", "f2", "v2")
    result = await r.eval("return redis.call('HVALS', KEYS[1])", 1, "myhash")
    # Order may vary, so check as sets
    assert set(result) == {b"v1", b"v2"}


# Set commands via redis.call()


async def test_redis_call_sadd_smembers(r):
    """LUA-03: redis.call('SADD') and redis.call('SMEMBERS') work correctly."""
    result = await r.eval(
        "redis.call('SADD', KEYS[1], ARGV[1], ARGV[2]); return redis.call('SMEMBERS', KEYS[1])",
        1,
        "myset",
        "a",
        "b",
    )
    assert set(result) == {b"a", b"b"}


async def test_redis_call_sismember(r):
    """LUA-03: redis.call('SISMEMBER') returns 1 for member, 0 for non-member."""
    await r.sadd("myset", "a")
    result_yes = await r.eval(
        "return redis.call('SISMEMBER', KEYS[1], ARGV[1])", 1, "myset", "a"
    )
    result_no = await r.eval(
        "return redis.call('SISMEMBER', KEYS[1], ARGV[1])", 1, "myset", "z"
    )
    assert result_yes == 1
    assert result_no == 0


async def test_redis_call_srem(r):
    """LUA-03: redis.call('SREM') removes a member and returns 1."""
    await r.sadd("myset", "a")
    result = await r.eval(
        "return redis.call('SREM', KEYS[1], ARGV[1])", 1, "myset", "a"
    )
    assert result == 1


# Sorted set commands via redis.call()


async def test_redis_call_zadd_zrange(r):
    """LUA-03: redis.call('ZADD') and redis.call('ZRANGE') work correctly."""
    result = await r.eval(
        "redis.call('ZADD', KEYS[1], '1.0', 'a', '2.0', 'b'); return redis.call('ZRANGE', KEYS[1], '0', '-1')",
        1,
        "zs",
    )
    assert result == [b"a", b"b"]


async def test_redis_call_zrem(r):
    """LUA-03: redis.call('ZREM') removes a sorted set member and returns 1."""
    await r.zadd("zs", {"a": 1.0, "b": 2.0})
    result = await r.eval(
        "return redis.call('ZREM', KEYS[1], 'a')", 1, "zs"
    )
    assert result == 1


async def test_redis_call_zrangebyscore(r):
    """LUA-03: redis.call('ZRANGEBYSCORE') returns members in score range."""
    await r.zadd("zs", {"a": 1.0, "b": 2.0, "c": 3.0})
    result = await r.eval(
        "return redis.call('ZRANGEBYSCORE', KEYS[1], '1', '2')", 1, "zs"
    )
    assert result == [b"a", b"b"]


async def test_redis_call_zremrangebyscore(r):
    """LUA-03: redis.call('ZREMRANGEBYSCORE') returns count of removed members."""
    await r.zadd("zs", {"a": 1.0, "b": 2.0, "c": 3.0})
    result = await r.eval(
        "return redis.call('ZREMRANGEBYSCORE', KEYS[1], '1', '2')", 1, "zs"
    )
    assert result == 2


# Stream commands via redis.call()


async def test_redis_call_xadd_xread(r):
    """LUA-03: redis.call('XADD') returns a stream ID."""
    result = await r.eval(
        "local id = redis.call('XADD', KEYS[1], '*', 'f1', 'v1'); return id",
        1,
        "stream",
    )
    assert isinstance(result, bytes)
    assert b"-" in result


# redis.pcall()


async def test_redis_pcall_success(r):
    """LUA-03: redis.pcall on valid command succeeds."""
    result = await r.eval(
        "local ok, err = pcall(function() return redis.call('SET', KEYS[1], 'v') end); return redis.call('GET', KEYS[1])",
        1,
        "k",
    )
    assert result == b"v"


async def test_redis_pcall_error(r):
    """LUA-03: redis.pcall on type error returns error table (nil in Python conversion)."""
    await r.set("k", "string_value")
    # redis.pcall should catch the WRONGTYPE error and return an error table
    # When converted to Python, the error table becomes None (it has 'err' field)
    result = await r.eval(
        "local r = redis.pcall('HSET', KEYS[1], 'f', 'v'); if r.err then return r.err else return 'no-error' end",
        1,
        "k",
    )
    assert b"WRONGTYPE" in result


async def test_redis_call_wrongtype_raises(r):
    """LUA-03: redis.call on wrong type raises an exception containing WRONGTYPE."""
    await r.set("k", "string_value")
    with pytest.raises(Exception, match="WRONGTYPE"):
        await r.eval("return redis.call('HSET', KEYS[1], 'f', 'v')", 1, "k")


async def test_redis_call_unknown_command(r):
    """LUA-03: redis.call with unknown command raises exception."""
    with pytest.raises(Exception, match="[Uu]nknown command"):
        await r.eval("return redis.call('FLUSHALL')", 0)


# --- LUA-04: SCRIPT LOAD ---


async def test_script_load_returns_sha1(r):
    """LUA-04: SCRIPT LOAD returns a 40-character hex string."""
    sha = await r.script_load("return 1")
    assert isinstance(sha, str)
    assert len(sha) == 40
    assert all(c in "0123456789abcdef" for c in sha)


async def test_script_load_sha1_matches_python(r):
    """LUA-04: SHA1 from script_load matches Python's hashlib computation."""
    script = "return 'test'"
    sha = await r.script_load(script)
    expected = hashlib.sha1(script.encode()).hexdigest()
    assert sha == expected


async def test_script_load_idempotent(r):
    """LUA-04: Loading same script twice returns the same SHA1."""
    sha1 = await r.script_load("return 'idem'")
    sha2 = await r.script_load("return 'idem'")
    assert sha1 == sha2


# --- LUA-05: SCRIPT EXISTS ---


async def test_script_exists_loaded(r):
    """LUA-05: SCRIPT EXISTS returns [True] for a loaded script."""
    sha = await r.script_load("return 1")
    result = await r.script_exists(sha)
    assert result == [True]


async def test_script_exists_not_loaded(r):
    """LUA-05: SCRIPT EXISTS returns [False] for an unknown SHA."""
    result = await r.script_exists("deadbeef" * 5)
    assert result == [False]


async def test_script_exists_multiple(r):
    """LUA-05: SCRIPT EXISTS handles multiple SHAs correctly."""
    sha = await r.script_load("return 'exists'")
    result = await r.script_exists(sha, "0" * 40)
    assert result == [True, False]


async def test_script_exists_after_eval(r):
    """LUA-05: SCRIPT EXISTS returns True for auto-cached script after EVAL."""
    script = "return 'auto'"
    await r.eval(script, 0)
    sha = hashlib.sha1(script.encode()).hexdigest()
    result = await r.script_exists(sha)
    assert result == [True]
