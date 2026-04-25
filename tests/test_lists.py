"""Tests for list commands: LPUSH, RPUSH, LPOP, RPOP, LRANGE, LLEN, LINDEX,
LINSERT, LREM, LSET, LTRIM, LMOVE, RPOPLPUSH, BRPOP, BLPOP, BLMOVE.

Covers requirements: LIST-01 through LIST-15. LIST-16 (Lua + pipeline) lives in Plan 03.
"""
import asyncio
import time

import pytest


# LIST-01: LPUSH basic + multi-value order
async def test_lpush_single(r):
    n = await r.lpush("k", "a")
    assert n == 1
    assert await r.lrange("k", 0, -1) == [b"a"]


async def test_lpush_multiple_order(r):
    # redis-py: LPUSH k a b c → list is [c, b, a]
    n = await r.lpush("k", "a", "b", "c")
    assert n == 3
    assert await r.lrange("k", 0, -1) == [b"c", b"b", b"a"]


async def test_lpush_wrongtype(r):
    await r.set("s", "v")
    with pytest.raises(Exception, match="WRONGTYPE"):
        await r.lpush("s", "x")


# LIST-02: RPUSH basic + multi-value order
async def test_rpush_single(r):
    n = await r.rpush("k", "a")
    assert n == 1
    assert await r.lrange("k", 0, -1) == [b"a"]


async def test_rpush_multiple_order(r):
    n = await r.rpush("k", "a", "b", "c")
    assert n == 3
    assert await r.lrange("k", 0, -1) == [b"a", b"b", b"c"]


# LIST-03: LPOP count semantics (redis-py drop-in)
async def test_lpop_no_count(r):
    await r.rpush("k", "a", "b", "c")
    v = await r.lpop("k")
    assert v == b"a"
    assert isinstance(v, bytes)


async def test_lpop_with_count(r):
    await r.rpush("k", "a", "b", "c")
    result = await r.lpop("k", count=2)
    assert result == [b"a", b"b"]


async def test_lpop_count_zero(r):
    await r.rpush("k", "a")
    result = await r.lpop("k", count=0)
    assert result == []


async def test_lpop_missing_key(r):
    assert await r.lpop("missing") is None
    assert await r.lpop("missing", count=5) is None


async def test_lpop_deletes_empty_key(r):
    await r.rpush("k", "a")
    await r.lpop("k")
    assert await r.llen("k") == 0
    assert await r.lpop("k") is None  # truly deleted, not just empty


# LIST-04: RPOP mirror
async def test_rpop_no_count(r):
    await r.rpush("k", "a", "b", "c")
    v = await r.rpop("k")
    assert v == b"c"


async def test_rpop_with_count(r):
    await r.rpush("k", "a", "b", "c")
    result = await r.rpop("k", count=2)
    assert result == [b"c", b"b"]


async def test_rpop_missing_with_count_returns_none(r):
    assert await r.rpop("missing", count=3) is None


# LIST-05: LRANGE
@pytest.mark.parametrize(
    "start,end,expected",
    [
        (0, -1, [b"a", b"b", b"c", b"d", b"e"]),
        (0, 100, [b"a", b"b", b"c", b"d", b"e"]),
        (-100, 100, [b"a", b"b", b"c", b"d", b"e"]),
        (-3, -1, [b"c", b"d", b"e"]),
        (-3, 2, [b"c"]),
        (5, 10, []),
        (3, 2, []),
        (-10, -6, []),
    ],
)
async def test_lrange_normalization(r, start, end, expected):
    await r.rpush("k", "a", "b", "c", "d", "e")
    assert await r.lrange("k", start, end) == expected


async def test_lrange_missing_key(r):
    assert await r.lrange("missing", 0, -1) == []


# LIST-06: LLEN
async def test_llen(r):
    assert await r.llen("missing") == 0
    await r.rpush("k", "a", "b", "c")
    assert await r.llen("k") == 3


# LIST-07: LINDEX
async def test_lindex(r):
    await r.rpush("k", "a", "b", "c")
    assert await r.lindex("k", 0) == b"a"
    assert await r.lindex("k", -1) == b"c"
    assert await r.lindex("k", 100) is None
    assert await r.lindex("k", -100) is None
    assert await r.lindex("missing", 0) is None


# LIST-08: LINSERT
async def test_linsert(r):
    await r.rpush("k", "a", "c")
    # Insert BEFORE "c" → [a, b, c]
    n = await r.linsert("k", "BEFORE", "c", "b")
    assert n == 3
    assert await r.lrange("k", 0, -1) == [b"a", b"b", b"c"]
    # Pivot not found → -1
    assert await r.linsert("k", "BEFORE", "missing_pivot", "x") == -1
    # Missing key → 0
    assert await r.linsert("absent", "AFTER", "a", "b") == 0


# LIST-09: LREM
async def test_lrem_head(r):
    await r.rpush("k", "a", "b", "a", "c", "a")
    assert await r.lrem("k", 2, "a") == 2
    assert await r.lrange("k", 0, -1) == [b"b", b"c", b"a"]


async def test_lrem_tail(r):
    await r.rpush("k", "a", "b", "a", "c", "a")
    assert await r.lrem("k", -2, "a") == 2
    assert await r.lrange("k", 0, -1) == [b"a", b"b", b"c"]


async def test_lrem_all(r):
    await r.rpush("k", "a", "b", "a", "c", "a")
    assert await r.lrem("k", 0, "a") == 3
    assert await r.lrange("k", 0, -1) == [b"b", b"c"]


async def test_lrem_missing_key(r):
    assert await r.lrem("missing", 0, "v") == 0


# LIST-10: LSET
async def test_lset(r):
    await r.rpush("k", "a", "b", "c")
    assert await r.lset("k", 1, "B") is True
    assert await r.lrange("k", 0, -1) == [b"a", b"B", b"c"]


async def test_lset_out_of_range(r):
    await r.rpush("k", "a")
    with pytest.raises(Exception, match="index out of range"):
        await r.lset("k", 100, "v")


async def test_lset_missing_key(r):
    with pytest.raises(Exception):
        await r.lset("missing", 0, "v")


# LIST-11: LTRIM
async def test_ltrim_keeps_range(r):
    await r.rpush("k", "a", "b", "c", "d", "e")
    assert await r.ltrim("k", 1, 3) is True
    assert await r.lrange("k", 0, -1) == [b"b", b"c", b"d"]


async def test_ltrim_empty_result_deletes_key(r):
    await r.rpush("k", "a", "b", "c")
    assert await r.ltrim("k", 5, 10) is True
    assert await r.llen("k") == 0
    assert await r.lpop("k") is None  # truly deleted


# LIST-12: LMOVE (cross-key + same-key rotation)
async def test_lmove_cross_key(r):
    await r.rpush("src", "a", "b", "c")
    moved = await r.lmove("src", "dst", src="LEFT", dest="RIGHT")
    assert moved == b"a"
    assert await r.lrange("src", 0, -1) == [b"b", b"c"]
    assert await r.lrange("dst", 0, -1) == [b"a"]


async def test_lmove_same_key_rotation(r):
    await r.rpush("k", "a", "b", "c")
    moved = await r.lmove("k", "k", src="RIGHT", dest="LEFT")
    assert moved == b"c"
    assert await r.lrange("k", 0, -1) == [b"c", b"a", b"b"]


async def test_lmove_empty_source(r):
    assert await r.lmove("missing", "dst", src="LEFT", dest="RIGHT") is None


# LIST-13: RPOPLPUSH
async def test_rpoplpush(r):
    await r.rpush("src", "a", "b", "c")
    v = await r.rpoplpush("src", "dst")
    assert v == b"c"
    assert await r.lrange("src", 0, -1) == [b"a", b"b"]
    assert await r.lrange("dst", 0, -1) == [b"c"]


async def test_rpoplpush_empty_source(r):
    assert await r.rpoplpush("missing", "dst") is None


# LIST-14: BRPOP / BLPOP blocking
async def test_blpop_timeout_returns_none(r):
    start = time.monotonic()
    result = await r.blpop(["empty"], timeout=0.1)
    elapsed = time.monotonic() - start
    assert result is None
    # M-05: lower bound is the meaningful assertion (we did wait at least
    # the requested timeout). Upper bound widened to 2.0s so loaded CI does
    # not flake on scheduling jitter.
    assert 0.05 < elapsed < 2.0


async def test_blpop_returns_tuple_on_success(r):
    await r.rpush("k", "v")
    result = await r.blpop(["k"], timeout=1.0)
    assert result == (b"k", b"v")
    assert isinstance(result, tuple)


async def test_blpop_multi_key_scan_order(r):
    # k2 and k4 are non-empty; k1 and k3 are not.
    # BLPOP must return from k2 (first non-empty, left-to-right).
    await r.rpush("k2", "v2")
    await r.rpush("k4", "v4")
    result = await r.blpop(["k1", "k2", "k3", "k4"], timeout=0.1)
    assert result == (b"k2", b"v2")
    # k4 must still have its value
    assert await r.llen("k4") == 1


async def test_blpop_wakes_on_push(r):
    async def push_later():
        await asyncio.sleep(0.05)
        await r.lpush("k", "v")
    task = asyncio.create_task(push_later())
    result = await r.blpop(["k"], timeout=2.0)
    await task
    assert result == (b"k", b"v")


async def test_blpop_block_zero_blocks_until_data(r):
    async def push_later():
        await asyncio.sleep(0.05)
        await r.lpush("k", "v")
    task = asyncio.create_task(push_later())
    result = await asyncio.wait_for(r.blpop(["k"], timeout=0), timeout=2.0)
    await task
    assert result == (b"k", b"v")


async def test_brpop_pops_from_tail(r):
    await r.rpush("k", "a", "b", "c")
    result = await r.brpop(["k"], timeout=1.0)
    assert result == (b"k", b"c")


# M-02: Explicit slow-path wake tests. The other "wakes_on_push" tests cannot
# distinguish "first poll succeeded because the pusher race-won" from "the
# tokio::select! wake-up actually fired" — both succeed cheaply. These tests
# pin a lower bound on elapsed time so a regression that only ever serves the
# fast path would be caught.
async def test_blpop_slow_path_wake_elapsed_lower_bound(r):
    """BLPOP must really sleep until LPUSH wakes it (not poll-and-find)."""
    SLEEP = 0.15

    async def push_later():
        await asyncio.sleep(SLEEP)
        await r.lpush("k", "v")

    task = asyncio.create_task(push_later())
    start = time.monotonic()
    result = await r.blpop(["k"], timeout=2.0)
    elapsed = time.monotonic() - start
    await task
    assert result == (b"k", b"v")
    # Must have actually waited at least roughly SLEEP — the only way the
    # client can have the value before this is if the slow path itself was
    # the path that produced it.
    assert elapsed >= SLEEP * 0.8, (
        f"BLPOP returned too fast ({elapsed}s); did not exercise slow path "
        f"(expected at least ~{SLEEP}s)"
    )
    # And it must not have hit the 2.0s timeout cap.
    assert elapsed < 2.0


async def test_brpop_slow_path_wake_elapsed_lower_bound(r):
    """BRPOP must really sleep until RPUSH wakes it."""
    SLEEP = 0.15

    async def push_later():
        await asyncio.sleep(SLEEP)
        await r.rpush("k", "v")

    task = asyncio.create_task(push_later())
    start = time.monotonic()
    result = await r.brpop(["k"], timeout=2.0)
    elapsed = time.monotonic() - start
    await task
    assert result == (b"k", b"v")
    assert elapsed >= SLEEP * 0.8, (
        f"BRPOP returned too fast ({elapsed}s); did not exercise slow path "
        f"(expected at least ~{SLEEP}s)"
    )
    assert elapsed < 2.0


async def test_blpop_cancellation_is_clean(r):
    # future_into_py returns a Future directly (not a coroutine), so wrap it
    # in a coroutine for asyncio.create_task.
    async def _blpop_forever():
        return await r.blpop(["never"], timeout=0)

    task = asyncio.create_task(_blpop_forever())
    await asyncio.sleep(0.05)
    task.cancel()
    with pytest.raises(asyncio.CancelledError):
        await task
    # Verify no hanging state: an immediate subsequent call works
    await r.lpush("k", "v")
    assert await r.blpop(["k"], timeout=0.1) == (b"k", b"v")


async def test_blpop_negative_timeout_raises(r):
    with pytest.raises(ValueError, match="non-negative"):
        await r.blpop(["k"], timeout=-1.0)


# LIST-15: BLMOVE
async def test_blmove_cross_key(r):
    await r.rpush("src", "a", "b", "c")
    moved = await r.blmove("src", "dst", timeout=1.0, src="LEFT", dest="RIGHT")
    assert moved == b"a"
    assert await r.lrange("dst", 0, -1) == [b"a"]


async def test_blmove_timeout_returns_none(r):
    start = time.monotonic()
    result = await r.blmove("empty", "dst", timeout=0.1, src="LEFT", dest="RIGHT")
    elapsed = time.monotonic() - start
    assert result is None
    # M-05: widen upper bound for loaded CI. Lower bound carries the meaning.
    assert 0.05 < elapsed < 2.0


async def test_blmove_wakes_on_push(r):
    async def push_later():
        await asyncio.sleep(0.05)
        await r.lpush("src", "v")
    task = asyncio.create_task(push_later())
    result = await r.blmove("src", "dst", timeout=2.0, src="LEFT", dest="RIGHT")
    await task
    assert result == b"v"
    assert await r.lrange("dst", 0, -1) == [b"v"]


# Value coercion verification
async def test_lpush_int_coerced(r):
    # Integer should be coerced to b"42" — NOT b"b'42'" (double-coercion bug guard)
    await r.lpush("k", 42)
    assert await r.lrange("k", 0, -1) == [b"42"]


async def test_lpush_float_coerced(r):
    await r.lpush("k", 3.14)
    assert await r.lrange("k", 0, -1) == [b"3.14"]


async def test_lpush_bool_raises(r):
    # Bool must raise TypeError — redis-py compat
    with pytest.raises(Exception):
        await r.lpush("k", True)


async def test_lset_int_coerced(r):
    await r.rpush("k", "a")
    await r.lset("k", 0, 42)
    assert await r.lindex("k", 0) == b"42"


async def test_linsert_int_coerced(r):
    await r.rpush("k", "a")
    await r.linsert("k", "AFTER", "a", 42)
    assert await r.lrange("k", 0, -1) == [b"a", b"42"]


# ---- LIST-16: Lua integration ----

async def test_lua_lpush_rpush_lrange(r):
    """LIST-16: Lua can dispatch LPUSH/LRANGE correctly."""
    result = await r.eval(
        "redis.call('LPUSH', KEYS[1], 'a', 'b', 'c'); "
        "return redis.call('LRANGE', KEYS[1], 0, -1)",
        1,
        "k",
    )
    assert result == [b"c", b"b", b"a"]


async def test_lua_rpush_order(r):
    """LIST-16: Lua RPUSH preserves order."""
    result = await r.eval(
        "redis.call('RPUSH', KEYS[1], 'a', 'b', 'c'); "
        "return redis.call('LRANGE', KEYS[1], 0, -1)",
        1,
        "k",
    )
    assert result == [b"a", b"b", b"c"]


async def test_lua_lpop_count(r):
    """LIST-16: Lua LPOP with count returns array."""
    await r.rpush("k", "a", "b", "c")
    result = await r.eval(
        "return redis.call('LPOP', KEYS[1], 2)",
        1,
        "k",
    )
    assert result == [b"a", b"b"]


async def test_lua_rpop_no_count(r):
    await r.rpush("k", "a", "b", "c")
    result = await r.eval("return redis.call('RPOP', KEYS[1])", 1, "k")
    assert result == b"c"


async def test_lua_llen(r):
    await r.rpush("k", "a", "b", "c")
    result = await r.eval("return redis.call('LLEN', KEYS[1])", 1, "k")
    assert result == 3


async def test_lua_lindex(r):
    await r.rpush("k", "a", "b", "c")
    result = await r.eval("return redis.call('LINDEX', KEYS[1], 1)", 1, "k")
    assert result == b"b"


async def test_lua_linsert(r):
    await r.rpush("k", "a", "c")
    result = await r.eval(
        "return redis.call('LINSERT', KEYS[1], 'AFTER', 'a', 'b')",
        1,
        "k",
    )
    assert result == 3
    assert await r.lrange("k", 0, -1) == [b"a", b"b", b"c"]


async def test_lua_lrem(r):
    await r.rpush("k", "a", "b", "a", "c", "a")
    result = await r.eval(
        "return redis.call('LREM', KEYS[1], 0, 'a')",
        1,
        "k",
    )
    assert result == 3
    assert await r.lrange("k", 0, -1) == [b"b", b"c"]


async def test_lua_lset(r):
    await r.rpush("k", "a", "b", "c")
    result = await r.eval(
        "return redis.call('LSET', KEYS[1], 1, 'B')",
        1,
        "k",
    )
    # Status "OK" is mapped through Lua back to Python
    assert result in (b"OK", "OK")
    assert await r.lrange("k", 0, -1) == [b"a", b"B", b"c"]


async def test_lua_lset_out_of_range_pcall(r):
    await r.rpush("k", "a")
    result = await r.eval(
        "local ok = redis.pcall('LSET', KEYS[1], 5, 'x'); "
        "if ok.err then return ok.err else return 'nope' end",
        1,
        "k",
    )
    assert b"index out of range" in result


async def test_lua_ltrim(r):
    await r.rpush("k", "a", "b", "c", "d")
    await r.eval("redis.call('LTRIM', KEYS[1], 1, 2)", 1, "k")
    assert await r.lrange("k", 0, -1) == [b"b", b"c"]


async def test_lua_lmove(r):
    await r.rpush("src", "a", "b", "c")
    result = await r.eval(
        "return redis.call('LMOVE', KEYS[1], KEYS[2], 'LEFT', 'RIGHT')",
        2, "src", "dst",
    )
    assert result == b"a"
    assert await r.lrange("dst", 0, -1) == [b"a"]
    assert await r.lrange("src", 0, -1) == [b"b", b"c"]


async def test_lua_rpoplpush(r):
    await r.rpush("src", "a", "b", "c")
    result = await r.eval(
        "return redis.call('RPOPLPUSH', KEYS[1], KEYS[2])",
        2, "src", "dst",
    )
    assert result == b"c"
    assert await r.lrange("dst", 0, -1) == [b"c"]
    assert await r.lrange("src", 0, -1) == [b"a", b"b"]


async def test_lua_blpop_rejected(r):
    """LIST-16: Lua BLPOP must raise the exact real-Redis wording.

    Real Redis returns: "This Redis command is not allowed from script"
    (singular "script", no colon, no command name) — M-01.
    """
    with pytest.raises(Exception, match="not allowed from script"):
        await r.eval("return redis.call('BLPOP', KEYS[1], 0)", 1, "k")


async def test_lua_brpop_rejected(r):
    with pytest.raises(Exception, match="not allowed from script"):
        await r.eval("return redis.call('BRPOP', KEYS[1], 0)", 1, "k")


async def test_lua_blmove_rejected(r):
    with pytest.raises(Exception, match="not allowed from script"):
        await r.eval(
            "return redis.call('BLMOVE', KEYS[1], KEYS[2], 'LEFT', 'RIGHT', 0)",
            2, "src", "dst",
        )


async def test_lua_blocking_error_does_not_include_command_name(r):
    """M-01 regression: error must NOT include the command name or a colon.

    Guards against the previous wording "...not allowed from scripts: BLPOP".
    The Lua stack traceback (appended by mlua) is allowed to contain colons,
    so we check just the first line of the message.
    """
    with pytest.raises(Exception) as excinfo:
        await r.eval("return redis.call('BLPOP', KEYS[1], 0)", 1, "k")
    msg = str(excinfo.value)
    # The first line is the actual reject message we control. Subsequent
    # lines are mlua's stack traceback (which legitimately contains colons,
    # source paths, etc.).
    first_line = msg.splitlines()[0]
    assert "BLPOP" not in first_line, (
        f"command name leaked into error: {first_line!r}"
    )
    # No trailing colon-with-suffix after "from script"
    assert "from script:" not in first_line and "from scripts" not in first_line, (
        f"old plural/colon wording leaked: {first_line!r}"
    )
    assert "not allowed from script" in first_line


async def test_brpop_wakes_on_lua_lpush(r):
    """LIST-16 regression: BRPOP must wake when LPUSH is issued from inside a Lua script.
    This is the Phase-11-style race fix guarded by the had_list_mutation flag.
    """
    async def lua_push_later():
        await asyncio.sleep(0.05)
        await r.eval("redis.call('LPUSH', KEYS[1], 'v'); return 1", 1, "k")

    task = asyncio.create_task(lua_push_later())
    start = time.monotonic()
    result = await r.brpop(["k"], timeout=2.0)
    elapsed = time.monotonic() - start
    await task
    assert elapsed < 1.0, f"BRPOP did not wake promptly on Lua LPUSH: {elapsed}s"
    assert result == (b"k", b"v")


async def test_blpop_wakes_on_lua_rpush(r):
    """Mirror for RPUSH — also marked had_list_mutation."""
    async def lua_push_later():
        await asyncio.sleep(0.05)
        await r.eval("redis.call('RPUSH', KEYS[1], 'v'); return 1", 1, "k")

    task = asyncio.create_task(lua_push_later())
    result = await asyncio.wait_for(r.blpop(["k"], timeout=2.0), timeout=3.0)
    await task
    assert result == (b"k", b"v")


# ---- LIST-16: Pipeline integration ----

async def test_pipeline_list_commands_non_blocking(r):
    """All non-blocking list commands in a pipeline — verify results + fast-path timing."""
    pipe = r.pipeline()
    pipe.lpush("k", "a", "b", "c")
    pipe.llen("k")
    pipe.lrange("k", 0, -1)
    pipe.lindex("k", 0)
    pipe.lpop("k")
    start = time.monotonic()
    results = await pipe.execute()
    elapsed = time.monotonic() - start
    assert elapsed < 0.1, f"fast path too slow: {elapsed}s"
    assert results[0] == 3  # lpush count
    assert results[1] == 3  # llen
    assert results[2] == [b"c", b"b", b"a"]  # lrange
    assert results[3] == b"c"  # lindex 0
    assert results[4] == b"c"  # lpop


async def test_pipeline_with_blocking_command(r):
    """Pipeline mixing blocking + non-blocking commands respects per-command timeout."""
    pipe = r.pipeline()
    pipe.set("x", "1")
    pipe.blpop(["missing"], timeout=0.1)
    pipe.set("y", "2")
    start = time.monotonic()
    results = await pipe.execute()
    elapsed = time.monotonic() - start
    assert elapsed >= 0.05, f"blocking pipeline did not block: {elapsed}s"
    assert results[0] is True  # set
    assert results[1] is None  # blpop timeout
    assert results[2] is True  # set after blpop


async def test_pipeline_blocking_wakes_on_existing_data(r):
    """Pipeline BLPOP with pre-existing data returns immediately."""
    await r.rpush("k", "pre-existing")  # guarantees first poll succeeds

    pipe = r.pipeline()
    pipe.set("x", "1")
    pipe.blpop(["k"], timeout=2.0)
    pipe.set("y", "2")

    start = time.monotonic()
    results = await pipe.execute()
    elapsed = time.monotonic() - start

    assert elapsed < 1.0, f"BLPOP in pipeline blocked unnecessarily: {elapsed}s"
    assert results[0] is True
    assert results[1] == (b"k", b"pre-existing")
    assert results[2] is True


async def test_pipeline_non_blocking_fast_path_timing(r):
    """Regression guard for quick task 260415-an2: non-blocking pipelines must stay sync-fast."""
    pipe = r.pipeline()
    for _ in range(50):
        pipe.lpush("k", "v")
    pipe.llen("k")
    start = time.monotonic()
    results = await pipe.execute()
    elapsed = time.monotonic() - start
    assert elapsed < 0.1, f"50-cmd non-blocking pipeline too slow: {elapsed}s (fast path may have regressed)"
    assert results[-1] == 50


async def test_pipeline_lrem_ltrim_lset(r):
    """Pipeline coverage for in-place list mutations."""
    pipe = r.pipeline()
    pipe.rpush("k", "a", "b", "a", "c")
    pipe.lrem("k", 0, "a")
    pipe.lset("k", 0, "B")
    pipe.ltrim("k", 0, 0)
    pipe.lrange("k", 0, -1)
    results = await pipe.execute()
    assert results[0] == 4  # rpush count
    assert results[1] == 2  # lrem removed 2 a's
    assert results[2] is True  # lset
    assert results[3] is True  # ltrim
    assert results[4] == [b"B"]


async def test_pipeline_lmove_rpoplpush(r):
    """Pipeline coverage for cross-key atomic moves."""
    pipe = r.pipeline()
    pipe.rpush("src", "a", "b", "c")
    pipe.lmove("src", "dst", src="LEFT", dest="RIGHT")
    pipe.rpoplpush("src", "dst")
    pipe.lrange("dst", 0, -1)
    pipe.lrange("src", 0, -1)
    results = await pipe.execute()
    assert results[0] == 3  # rpush count
    assert results[1] == b"a"  # lmove popped "a" from head of src
    assert results[2] == b"c"  # rpoplpush popped "c" from tail of src, pushed to head of dst
    assert results[3] == [b"c", b"a"]  # dst: c (head), a (tail)
    assert results[4] == [b"b"]  # src: remaining


async def test_pipeline_linsert(r):
    """Pipeline coverage for LINSERT (variadic position arg)."""
    pipe = r.pipeline()
    pipe.rpush("k", "a", "c")
    pipe.linsert("k", "AFTER", "a", "b")
    pipe.lrange("k", 0, -1)
    results = await pipe.execute()
    assert results[0] == 2
    assert results[1] == 3  # new length
    assert results[2] == [b"a", b"b", b"c"]


# ---- H-01 regression: pipeline value coercion (drop-in parity with client) ----

async def test_pipeline_lpush_int_coerced(r):
    """Pipeline lpush must coerce ints to bytes — same rule as r.lpush."""
    pipe = r.pipeline()
    pipe.lpush("k", 42)
    pipe.lrange("k", 0, -1)
    results = await pipe.execute()
    assert results[0] == 1
    assert results[1] == [b"42"]


async def test_pipeline_rpush_float_coerced(r):
    """Pipeline rpush must coerce floats to bytes — same rule as r.rpush."""
    pipe = r.pipeline()
    pipe.rpush("k", 3.14)
    pipe.lrange("k", 0, -1)
    results = await pipe.execute()
    assert results[0] == 1
    assert results[1] == [b"3.14"]


async def test_pipeline_lpush_bool_raises(r):
    """Pipeline lpush must reject booleans — same rule as r.lpush.

    Coercion happens at buffer time, so we expect TypeError on the .lpush()
    call itself (before .execute()).
    """
    pipe = r.pipeline()
    with pytest.raises(TypeError):
        pipe.lpush("k", True)


async def test_pipeline_lset_int_coerced(r):
    """Pipeline lset must coerce ints — mirror of r.lset."""
    await r.rpush("k", "a")
    pipe = r.pipeline()
    pipe.lset("k", 0, 42)
    pipe.lindex("k", 0)
    results = await pipe.execute()
    assert results[1] == b"42"


async def test_pipeline_linsert_int_coerced(r):
    """Pipeline linsert must coerce inserted value but leave refvalue alone."""
    await r.rpush("k", "a")
    pipe = r.pipeline()
    pipe.linsert("k", "AFTER", "a", 42)
    pipe.lrange("k", 0, -1)
    results = await pipe.execute()
    assert results[0] == 2
    assert results[1] == [b"a", b"42"]


async def test_pipeline_set_int_coerced(r):
    """Pipeline set must coerce ints — mirror of monkey-patched _coerced_set."""
    pipe = r.pipeline()
    pipe.set("k", 42)
    pipe.get("k")
    results = await pipe.execute()
    assert results[0] is True
    assert results[1] == b"42"


async def test_pipeline_set_bool_raises(r):
    """Pipeline set must reject booleans (same rule as r.set)."""
    pipe = r.pipeline()
    with pytest.raises(TypeError):
        pipe.set("k", True)


# ---- P2-01 regression: blocking pipelines must execute all commands before raising ----


async def test_pipeline_blocking_continues_on_error_then_raises_first(r):
    """P2-01: Slow path (pipeline contains blocking commands) must mirror the
    fast path — capture per-command errors and raise only the first one AFTER
    all commands have been attempted. Previously the slow path raised on the
    first error, leaving subsequent commands un-executed (e.g. `set('after')`
    was never run).
    """
    # Pre-populate "k" so blpop succeeds quickly.
    await r.rpush("k", "v")
    pipe = r.pipeline()
    pipe.blpop(["k"], timeout=1)            # succeeds
    pipe.lset("missing", 0, "x")            # raises ResponseError ("no such key")
    pipe.set("after", "1")                  # MUST execute despite prior error

    # Default raise_on_error=True: should raise the captured ResponseError after
    # all three commands have been attempted.
    with pytest.raises(Exception, match="no such key"):
        await pipe.execute()

    # Critical assertion: the third command DID execute, even though the second
    # failed — matches redis-py / fast-path semantics.
    assert await r.get("after") == b"1"


# ---- P2-07 regression: LREM value must be coerced (redis-py parity) ----


async def test_lrem_int_value_coerced(r):
    """P2-07: r.lrem('k', 0, 42) previously raised TypeError because the
    PyO3 binding only accepted str/bytes. redis-py encodes ints/floats for
    `value` like LPUSH/LSET. Now we coerce at the Python wrapper."""
    await r.rpush("k", 42, "a", 42, "b")
    n = await r.lrem("k", 0, 42)  # remove all matches of int 42
    assert n == 2
    assert await r.lrange("k", 0, -1) == [b"a", b"b"]


async def test_lrem_float_value_coerced(r):
    await r.rpush("k", 3.14, "a", 3.14)
    n = await r.lrem("k", 1, 3.14)  # remove first match
    assert n == 1
    assert await r.lrange("k", 0, -1) == [b"a", b"3.14"]


async def test_lrem_bool_value_raises(r):
    """P2-07: bool is rejected (matches _coerce_value contract)."""
    await r.rpush("k", "a")
    with pytest.raises(TypeError):
        await r.lrem("k", 0, True)


async def test_pipeline_lrem_int_value_coerced(r):
    """P2-07: pipeline LREM also coerces value (mirror of client)."""
    await r.rpush("k", 42, "a", 42)
    pipe = r.pipeline()
    pipe.lrem("k", 0, 42)
    pipe.lrange("k", 0, -1)
    results = await pipe.execute()
    assert results[0] == 2
    assert results[1] == [b"a"]


# ---- P2-06 regression: LINSERT pivot must be coerced (redis-py parity) ----


async def test_linsert_int_pivot_matches_bytes_pivot(r):
    """P2-06: redis-py encodes every command argument including the
    LINSERT pivot, so numeric pivots are legal. Previously raised
    TypeError because the wrapper forwarded refvalue raw."""
    await r.rpush("k", 42)
    n = await r.linsert("k", "AFTER", 42, "x")
    assert n == 2
    assert await r.lrange("k", 0, -1) == [b"42", b"x"]


async def test_linsert_float_pivot_coerced(r):
    """P2-06: float pivots also encode to bytes."""
    await r.rpush("k", 3.14)
    n = await r.linsert("k", "BEFORE", 3.14, "y")
    assert n == 2
    assert await r.lrange("k", 0, -1) == [b"y", b"3.14"]


async def test_pipeline_linsert_int_pivot_coerced(r):
    """P2-06: pipeline LINSERT pivot also coerced (mirror of client)."""
    await r.rpush("k", 7)
    pipe = r.pipeline()
    pipe.linsert("k", "AFTER", 7, "z")
    pipe.lrange("k", 0, -1)
    results = await pipe.execute()
    assert results[0] == 2
    assert results[1] == [b"7", b"z"]


# ---- P2-05 regression: LPUSH/RPUSH must reject calls with no values ----


async def test_lpush_no_values_raises_and_does_not_create_key(r):
    """P2-05: lpush('k') previously created an empty list and returned 0.
    Real Redis rejects with wrong-arity and leaves the key absent."""
    with pytest.raises(Exception, match="wrong number of arguments"):
        await r.lpush("k")
    # Key must remain absent (no empty list created).
    assert await r.exists("k") == 0


async def test_rpush_no_values_raises_and_does_not_create_key(r):
    with pytest.raises(Exception, match="wrong number of arguments"):
        await r.rpush("k")
    assert await r.exists("k") == 0


async def test_pipeline_lpush_no_values_raises_at_execute(r):
    """P2-05: pipeline must mirror the client — empty values raises on
    execute (the dispatch hits the same Store::lpush guard)."""
    pipe = r.pipeline()
    pipe.lpush("k")
    with pytest.raises(Exception, match="wrong number of arguments"):
        await pipe.execute()
    assert await r.exists("k") == 0


async def test_lua_lpush_no_values_returns_error(r):
    """P2-05: Lua dispatch must also reject — keep parity with direct call."""
    # The Lua dispatch arm already does `args.len() < 2`, so this exercises
    # the existing code path (regression guard against future drift).
    with pytest.raises(Exception, match="wrong number of arguments"):
        await r.eval("return redis.call('LPUSH', KEYS[1])", 1, "k")
    assert await r.exists("k") == 0


# ---- P2-04 regression: BLPOP/BRPOP must reject empty key lists ----


async def test_blpop_empty_keys_raises_wrong_arity(r):
    """P2-04: blpop([], timeout=0) previously hung forever; finite timeouts
    returned None. Real Redis treats no-keys as a wrong-arity error."""
    with pytest.raises(Exception, match="wrong number of arguments"):
        await r.blpop([], timeout=0.1)


async def test_brpop_empty_keys_raises_wrong_arity(r):
    """P2-04: BRPOP mirror — empty keys list must error, not block."""
    with pytest.raises(Exception, match="wrong number of arguments"):
        await r.brpop([], timeout=0.1)


async def test_blpop_empty_tuple_raises_wrong_arity(r):
    """P2-04: tuple form is also rejected (normalize_key_list accepts both)."""
    with pytest.raises(Exception, match="wrong number of arguments"):
        await r.blpop((), timeout=0.1)


# ---- P2-03 regression: sub-millisecond blocking timeouts must expire ----


async def test_blpop_sub_millisecond_timeout_expires(r):
    """P2-03: A positive timeout below 1ms previously truncated to 0
    (block forever). Now it rounds up to >=1ms and must return None."""
    start = time.monotonic()
    result = await r.blpop(["empty"], timeout=0.0005)
    elapsed = time.monotonic() - start
    assert result is None
    # Generous upper bound — we just need to confirm it didn't block forever.
    assert elapsed < 1.0


async def test_brpop_sub_millisecond_timeout_expires(r):
    """P2-03: BRPOP mirror — must expire instead of blocking forever."""
    start = time.monotonic()
    result = await r.brpop(["empty"], timeout=0.0005)
    elapsed = time.monotonic() - start
    assert result is None
    assert elapsed < 1.0


# ---- P2-02 regression: LMOVE/RPOPLPUSH return nil before dst type check ----


async def test_rpoplpush_missing_src_with_string_dst_returns_none(r):
    """P2-02: When source is missing, the move is a no-op and must return None
    without ever inspecting dst's type. Previously raised WRONGTYPE."""
    await r.set("string_dst", "x")
    assert await r.rpoplpush("missing", "string_dst") is None
    # dst untouched
    assert await r.get("string_dst") == b"x"


async def test_lmove_missing_src_with_string_dst_returns_none(r):
    """P2-02: LMOVE mirror — missing src + string dst = None, no WRONGTYPE."""
    await r.set("string_dst", "x")
    assert await r.lmove("missing", "string_dst", src="LEFT", dest="RIGHT") is None
    assert await r.get("string_dst") == b"x"


async def test_lmove_nonempty_src_with_string_dst_still_wrongtype(r):
    """P2-02: When src DOES have a poppable element, the dst type-check still
    fires BEFORE pop — this is the atomicity guarantee we must preserve."""
    await r.rpush("src", "a")
    await r.set("string_dst", "x")
    with pytest.raises(Exception, match="WRONGTYPE"):
        await r.lmove("src", "string_dst", src="LEFT", dest="RIGHT")
    # src untouched (no element lost)
    assert await r.lrange("src", 0, -1) == [b"a"]


async def test_pipeline_blocking_no_raise_returns_exceptions_inline(r):
    """P2-01: With raise_on_error=False the slow path returns Exception objects
    inline at failed positions, matching the fast path."""
    await r.rpush("k", "v")
    pipe = r.pipeline()
    pipe.blpop(["k"], timeout=1)
    pipe.lset("missing", 0, "x")
    pipe.set("after", "2")
    results = await pipe.execute(raise_on_error=False)
    assert results[0] == (b"k", b"v")
    assert isinstance(results[1], Exception)
    assert results[2] is True
    assert await r.get("after") == b"2"
