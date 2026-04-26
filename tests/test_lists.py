"""Tests for list commands: LPUSH, RPUSH, LPOP, RPOP, LRANGE, LLEN, LINDEX,
LINSERT, LREM, LSET, LTRIM, LMOVE, RPOPLPUSH, BRPOP, BLPOP, BLMOVE.

Covers requirements: LIST-01 through LIST-15. LIST-16 (Lua + pipeline) lives in Plan 03.
"""
import asyncio
import inspect
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


async def test_lrem_count_i64_min_no_panic(r):
    # P2 regression: count = i64::MIN previously overflowed inside
    # parse_lrem_count via `-count`. Must return 0 cleanly on a
    # missing key, not panic.
    assert await r.lrem("missing", -9223372036854775808, b"v") == 0


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


# P2: Blocking-list coroutine semantics (redis.asyncio compat)
# The Rust bindings return asyncio.Future eagerly. Python async-def wrappers
# in burner_redis/__init__.py must convert them back into coroutines so
# that (a) asyncio.create_task(r.blpop(...)) accepts them and
# (b) the blocking pop does not begin until the coroutine is awaited.

async def test_blpop_is_coroutine_function(r):
    from burner_redis import BurnerRedis
    assert inspect.iscoroutinefunction(BurnerRedis.blpop)
    assert inspect.iscoroutinefunction(BurnerRedis.brpop)
    assert inspect.iscoroutinefunction(BurnerRedis.blmove)


async def test_blpop_returns_coroutine(r):
    coro = r.blpop(["never"], timeout=0.05)
    assert inspect.iscoroutine(coro)
    # Must await (or close) — never leak an un-awaited coroutine.
    result = await coro
    assert result is None


async def test_brpop_returns_coroutine(r):
    coro = r.brpop(["never"], timeout=0.05)
    assert inspect.iscoroutine(coro)
    result = await coro
    assert result is None


async def test_blmove_returns_coroutine(r):
    coro = r.blmove("empty", "dst", timeout=0.05, src="LEFT", dest="RIGHT")
    assert inspect.iscoroutine(coro)
    result = await coro
    assert result is None


async def test_blpop_create_task_accepts_coroutine(r):
    # Before fix: raised TypeError "expected a coroutine".
    task = asyncio.create_task(r.blpop(["empty"], timeout=0.1))
    result = await task
    assert result is None


async def test_brpop_create_task_accepts_coroutine(r):
    task = asyncio.create_task(r.brpop(["empty"], timeout=0.1))
    result = await task
    assert result is None


async def test_blmove_create_task_accepts_coroutine(r):
    task = asyncio.create_task(
        r.blmove("empty", "dst", timeout=0.1, src="LEFT", dest="RIGHT")
    )
    result = await task
    assert result is None


async def test_blpop_deferred_execution_does_not_pop_before_await(r):
    # Pre-populate the list, then create the coroutine WITHOUT awaiting.
    # If the blocking pop started eagerly (the bug), the pre-populated
    # value would be consumed before our subsequent observation. With the
    # async-def wrapper, the pop runs only when we await the coroutine.
    await r.lpush("k", "v")
    coro = r.blpop(["k"], timeout=1.0)

    # Yield control briefly. Any eagerly-started Tokio future would have
    # had ample time to drain "k" by now.
    await asyncio.sleep(0.05)

    # Confirm the value is still there — the pop has not run yet.
    assert await r.lrange("k", 0, -1) == [b"v"]

    # Now actually await the coroutine; it should pop the value.
    result = await coro
    assert result == (b"k", b"v")
    assert await r.lrange("k", 0, -1) == []


async def test_brpop_deferred_execution_does_not_pop_before_await(r):
    await r.rpush("k", "v")
    coro = r.brpop(["k"], timeout=1.0)

    await asyncio.sleep(0.05)

    assert await r.lrange("k", 0, -1) == [b"v"]

    result = await coro
    assert result == (b"k", b"v")
    assert await r.lrange("k", 0, -1) == []


async def test_blmove_deferred_execution_does_not_move_before_await(r):
    await r.rpush("src", "v")
    coro = r.blmove("src", "dst", timeout=1.0, src="LEFT", dest="RIGHT")

    await asyncio.sleep(0.05)

    # If blmove had started eagerly, "src" would be empty and "dst" would
    # already contain the value.
    assert await r.lrange("src", 0, -1) == [b"v"]
    assert await r.lrange("dst", 0, -1) == []

    result = await coro
    assert result == b"v"
    assert await r.lrange("src", 0, -1) == []
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


# ---- P2 regression (260425-sjc): BLPOP/BRPOP single bytes-like scalar keys ----
# Before this fix, normalize_key_list only special-cased PyString and PyBytes
# before falling through to the PySequence protocol. memoryview and bytearray
# are sequences too — iterating them yielded `int`s (per-byte) and crashed
# extract_bytes with TypeError. redis-py's Encoder accepts bytes/bytearray/
# memoryview as scalar bytes-likes; we now match that.


async def test_blpop_accepts_single_bytes_scalar(r):
    """260425-sjc: bytes scalar (regression guard — already worked pre-fix)."""
    assert await r.blpop(b"empty_k", timeout=0.05) is None


async def test_blpop_accepts_single_str_scalar(r):
    """260425-sjc: str scalar (regression guard — already worked pre-fix)."""
    assert await r.blpop("empty_k", timeout=0.05) is None


async def test_blpop_accepts_single_memoryview_scalar(r):
    """260425-sjc: memoryview scalar — primary failing case before fix.
    Previously iterated as int sequence and raised TypeError."""
    assert await r.blpop(memoryview(b"empty_k"), timeout=0.05) is None


async def test_blpop_accepts_single_bytearray_scalar(r):
    """260425-sjc: bytearray scalar — companion failing case before fix."""
    assert await r.blpop(bytearray(b"empty_k"), timeout=0.05) is None


async def test_blpop_accepts_list_keys_regression(r):
    """260425-sjc: multi-key list path must still iterate (regression guard)."""
    assert await r.blpop([b"k1", b"k2"], timeout=0.05) is None


async def test_blpop_accepts_tuple_keys_regression(r):
    """260425-sjc: multi-key tuple path must still iterate (regression guard)."""
    assert await r.blpop((b"k1", b"k2"), timeout=0.05) is None


async def test_brpop_accepts_single_memoryview_scalar(r):
    """260425-sjc: BRPOP mirror — memoryview scalar must not be iterated."""
    assert await r.brpop(memoryview(b"empty_k"), timeout=0.05) is None


async def test_brpop_accepts_single_bytearray_scalar(r):
    """260425-sjc: BRPOP mirror — bytearray scalar must not be iterated."""
    assert await r.brpop(bytearray(b"empty_k"), timeout=0.05) is None


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


# ---- P2-08 regression: Lua list-grow notifications must fire even when the
# script ultimately raises. Real Redis does not roll back earlier writes
# inside a script, so any BRPOP/BLPOP waiter parked on a key that was
# pushed-to before the error must still be woken. ----


async def test_lua_eval_pushes_then_errors_wakes_blpop_waiter(r):
    """P2-08: a Lua script that LPUSHes a value and THEN raises must still wake
    a BLPOP waiter. Without the fix the waiter parks forever (timeout=0)."""

    async def lua_push_then_error():
        # Tiny delay so BLPOP is parked when the script runs.
        await asyncio.sleep(0.05)
        # LPUSH succeeds (real Redis commits it), then redis.call('FOO') raises.
        # The script's overall result is an error, but the LPUSH already grew
        # the list; BLPOP must wake on it.
        with pytest.raises(Exception):
            await r.eval(
                "redis.call('LPUSH', KEYS[1], 'x'); return redis.call('FOO')",
                1,
                "k",
            )

    task = asyncio.create_task(lua_push_then_error())
    start = time.monotonic()
    # timeout=2.0 (not 0) so the test cannot hang the suite if the fix
    # regresses; the assertion on `elapsed` proves the waiter woke promptly
    # rather than only via the safety net.
    result = await r.blpop(["k"], timeout=2.0)
    elapsed = time.monotonic() - start
    await task
    assert elapsed < 1.0, (
        f"BLPOP did not wake on Lua LPUSH-then-error in time: {elapsed}s "
        "(P2-08: had_list_mutation flag dropped on script error path)"
    )
    assert result == (b"k", b"x")


async def test_lua_eval_pushes_then_errors_finite_blpop_returns_value(r):
    """P2-08: finite-timeout BLPOP variant — must return the pushed value
    (not None) when an erroring script grew the list before raising."""

    async def lua_push_then_error():
        await asyncio.sleep(0.05)
        with pytest.raises(Exception):
            await r.eval(
                "redis.call('RPUSH', KEYS[1], 'y'); return redis.call('NOPE')",
                1,
                "k2",
            )

    task = asyncio.create_task(lua_push_then_error())
    start = time.monotonic()
    result = await r.blpop(["k2"], timeout=2.0)
    elapsed = time.monotonic() - start
    await task
    assert elapsed < 1.0, (
        f"BLPOP returned None / timed out on Lua RPUSH-then-error: {elapsed}s"
    )
    assert result == (b"k2", b"y")


# ---- P2-09 regression: same-key list moves must preserve TTL.
# `RPOPLPUSH k k` and `LMOVE k k LEFT RIGHT` are pure rotations — the key
# is never removed, so its expires_at must survive. Cross-key moves must
# NOT propagate src's TTL onto dst (that would be the opposite bug). ----


async def test_rpoplpush_same_key_preserves_ttl(r):
    """P2-09: `RPOPLPUSH k k` rotates a single-element list. The list briefly
    becomes empty mid-op but the key never goes away, so its TTL must
    survive (real Redis behavior)."""
    await r.rpush("k", "a")
    assert await r.expire("k", 60) is True
    moved = await r.rpoplpush("k", "k")
    assert moved == b"a"
    ttl = await r.ttl("k")
    # We just set 60s; allow a generous floor. Pre-fix: ttl == -1 (cleared).
    assert ttl > 50, f"TTL not preserved across same-key RPOPLPUSH: ttl={ttl}"
    # And the rotation actually happened (single element list cycles to itself).
    assert await r.lrange("k", 0, -1) == [b"a"]


async def test_lmove_same_key_preserves_ttl(r):
    """P2-09: LMOVE rotation mirror — `LMOVE k k LEFT RIGHT` on a
    single-element list must preserve TTL. Single-element ensures the
    list briefly becomes empty between pop and push, exercising the
    `data.remove(src)` branch that was clearing TTL pre-fix."""
    await r.rpush("k", "a")
    assert await r.expire("k", 60) is True
    moved = await r.lmove("k", "k", src="LEFT", dest="RIGHT")
    assert moved == b"a"
    ttl = await r.ttl("k")
    # Pre-fix: ttl == -1 (cleared by remove → or_insert_with).
    assert ttl > 50, f"TTL not preserved across same-key LMOVE: ttl={ttl}"
    assert await r.lrange("k", 0, -1) == [b"a"]


async def test_lua_lmove_same_key_preserves_ttl(r):
    """P2-09: same-key LMOVE invoked via EVAL must also preserve TTL —
    the Lua dispatch arm has its own remove/recreate code that mirrors
    the direct path's bug pre-fix. Single-element list to exercise the
    `src_empty` branch."""
    await r.rpush("k", "a")
    assert await r.expire("k", 60) is True
    moved = await r.eval(
        "return redis.call('LMOVE', KEYS[1], KEYS[1], 'LEFT', 'RIGHT')",
        1,
        "k",
    )
    assert moved == b"a"
    ttl = await r.ttl("k")
    assert ttl > 50, f"TTL not preserved across Lua same-key LMOVE: ttl={ttl}"
    assert await r.lrange("k", 0, -1) == [b"a"]


async def test_lua_rpoplpush_same_key_preserves_ttl(r):
    """P2-09: same-key RPOPLPUSH via EVAL — Lua arm must preserve TTL."""
    await r.rpush("k", "a")
    assert await r.expire("k", 60) is True
    moved = await r.eval(
        "return redis.call('RPOPLPUSH', KEYS[1], KEYS[1])",
        1,
        "k",
    )
    assert moved == b"a"
    ttl = await r.ttl("k")
    assert ttl > 50, f"TTL not preserved across Lua same-key RPOPLPUSH: ttl={ttl}"
    assert await r.lrange("k", 0, -1) == [b"a"]


async def test_rpoplpush_diff_keys_does_not_carry_src_ttl(r):
    """P2-09 negative control: cross-key RPOPLPUSH must NOT propagate src's
    TTL onto dst. Guards against an over-eager fix that special-cased TTL
    on the destination."""
    await r.rpush("src", "a")
    assert await r.expire("src", 60) is True
    moved = await r.rpoplpush("src", "dst")
    assert moved == b"a"
    # src is now empty → key removed, TTL gone.
    assert await r.ttl("src") == -2  # -2 = key does not exist
    # dst is a freshly created list with no TTL.
    assert await r.ttl("dst") == -1  # -1 = no expire
    assert await r.lrange("dst", 0, -1) == [b"a"]


# ---- P2-10 regression: Lua LMOVE/RPOPLPUSH must check src for a poppable
# element BEFORE inspecting dst's type. Missing/empty src is a no-op move
# and must return nil regardless of dst's type — matches the direct path
# after round-1 P2-02 and real Redis. ----


async def test_lua_lmove_missing_src_with_string_dst_returns_nil(r):
    """P2-10: Lua LMOVE with missing src and string dst must return nil
    (not WRONGTYPE). The Lua arm must not inspect dst's type when there
    is no element to move — same semantics as `Store::lmove_atomic`."""
    await r.set("string_dst", "x")
    result = await r.eval(
        "return redis.call('LMOVE', KEYS[1], KEYS[2], 'LEFT', 'RIGHT')",
        2,
        "missing",
        "string_dst",
    )
    assert result is None
    # dst untouched.
    assert await r.get("string_dst") == b"x"


async def test_lua_rpoplpush_missing_src_with_string_dst_returns_nil(r):
    """P2-10: Lua RPOPLPUSH mirror — missing src + string dst returns nil."""
    await r.set("string_dst", "x")
    result = await r.eval(
        "return redis.call('RPOPLPUSH', KEYS[1], KEYS[2])",
        2,
        "missing",
        "string_dst",
    )
    assert result is None
    assert await r.get("string_dst") == b"x"


async def test_lua_lmove_empty_src_with_string_dst_returns_nil(r):
    """P2-10: empty-list src variant — even if src exists as a list with
    zero elements (unusual; D-03 normally removes empties, but be
    defensive), the move is a no-op and dst's type must not be inspected."""
    # Build then drain a list so src is an empty-list state. After LPOP of
    # the only element, D-03 removes the key entirely, so this collapses
    # to the missing-src case via a different path. We exercise that path
    # explicitly here.
    await r.rpush("empty_src", "tmp")
    await r.lpop("empty_src")
    assert await r.exists("empty_src") == 0
    await r.set("string_dst", "x")
    result = await r.eval(
        "return redis.call('LMOVE', KEYS[1], KEYS[2], 'LEFT', 'RIGHT')",
        2,
        "empty_src",
        "string_dst",
    )
    assert result is None
    assert await r.get("string_dst") == b"x"


async def test_lua_lmove_nonempty_src_with_string_dst_still_wrongtype(r):
    """P2-10 atomicity guard: when src DOES have a poppable element, the
    dst type-check must STILL fire BEFORE the pop — mirrors the round-1
    P2-02 atomicity test for the direct path. The data write lock spans
    the whole Lua arm, so the dst type cannot change between check and
    push; we just need the check to fire before any mutation so a string
    dst aborts the move without consuming src."""
    await r.rpush("src", "a")
    await r.set("string_dst", "x")
    with pytest.raises(Exception, match="WRONGTYPE"):
        await r.eval(
            "return redis.call('LMOVE', KEYS[1], KEYS[2], 'LEFT', 'RIGHT')",
            2,
            "src",
            "string_dst",
        )
    # src untouched (no element popped).
    assert await r.lrange("src", 0, -1) == [b"a"]
    # dst untouched.
    assert await r.get("string_dst") == b"x"


# ---- 260425-ftl: bytes-token compatibility (P3) ----
# Verifies linsert/lmove/blmove + their dispatch_pipeline_command arms accept
# pre-encoded bytes for option tokens (where, src, dest), not just str.
# Real Redis + redis-py accept either; redis-py's Encoder pre-encodes str→bytes
# before dispatch, so a Pipeline / execute_command consumer that pre-encodes
# tokens previously hit a TypeError at the PyO3 boundary.


async def test_linsert_bytes_where_before(r):
    await r.rpush("k", "a", "c")
    n = await r.linsert("k", b"BEFORE", "c", "b")
    assert n == 3
    assert await r.lrange("k", 0, -1) == [b"a", b"b", b"c"]


async def test_linsert_bytes_where_after(r):
    await r.rpush("k", "a", "c")
    n = await r.linsert("k", b"AFTER", "a", "b")
    assert n == 3
    assert await r.lrange("k", 0, -1) == [b"a", b"b", b"c"]


async def test_linsert_bytes_where_lowercase(r):
    # Case-insensitive parity with the str path (parse_linsert_where is case-insensitive).
    await r.rpush("k", "a", "c")
    n = await r.linsert("k", b"before", "c", "b")
    assert n == 3
    assert await r.lrange("k", 0, -1) == [b"a", b"b", b"c"]


async def test_linsert_bytes_where_unknown_token(r):
    await r.rpush("k", "a")
    with pytest.raises(Exception, match="syntax"):
        await r.linsert("k", b"SIDEWAYS", "a", "b")


async def test_linsert_bytes_where_invalid_utf8(r):
    await r.rpush("k", "a")
    # Invalid UTF-8 bytes must surface as a syntax error (same path as unknown
    # token), NOT a TypeError leak from the helper. extract_token_str feeds the
    # lossy decode into parse_linsert_where which then emits StoreError::Syntax
    # → ResponseError, matching real-Redis unknown-token semantics.
    with pytest.raises(Exception, match="syntax"):
        await r.linsert("k", b"\xff", "a", "b")


async def test_lmove_bytes_tokens_cross_key(r):
    await r.rpush("src", "a", "b", "c")
    moved = await r.lmove("src", "dst", src=b"LEFT", dest=b"RIGHT")
    assert moved == b"a"
    assert await r.lrange("src", 0, -1) == [b"b", b"c"]
    assert await r.lrange("dst", 0, -1) == [b"a"]


async def test_lmove_bytes_tokens_same_key_rotation(r):
    await r.rpush("k", "a", "b", "c")
    moved = await r.lmove("k", "k", src=b"RIGHT", dest=b"LEFT")
    assert moved == b"c"
    assert await r.lrange("k", 0, -1) == [b"c", b"a", b"b"]


async def test_lmove_bytes_tokens_lowercase(r):
    await r.rpush("src", "a", "b", "c")
    moved = await r.lmove("src", "dst", src=b"left", dest=b"right")
    assert moved == b"a"


async def test_blmove_bytes_tokens(r):
    await r.rpush("src", "a", "b", "c")
    moved = await r.blmove("src", "dst", timeout=1.0, src=b"LEFT", dest=b"RIGHT")
    assert moved == b"a"
    assert await r.lrange("dst", 0, -1) == [b"a"]


# Pipeline dispatch path — covers the dispatch_pipeline_command linsert/lmove arms.


async def test_pipeline_linsert_bytes_where(r):
    await r.rpush("k", "a", "c")
    pipe = r.pipeline()
    pipe.linsert("k", b"BEFORE", "c", "b")
    pipe.lrange("k", 0, -1)
    results = await pipe.execute()
    assert results[0] == 3
    assert results[1] == [b"a", b"b", b"c"]


async def test_pipeline_lmove_bytes_tokens(r):
    await r.rpush("src", "a", "b", "c")
    pipe = r.pipeline()
    pipe.lmove("src", "dst", src=b"LEFT", dest=b"RIGHT")
    pipe.lrange("dst", 0, -1)
    results = await pipe.execute()
    assert results[0] == b"a"
    assert results[1] == [b"a"]
