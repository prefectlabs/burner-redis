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
    assert 0.05 < elapsed < 0.5


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
    assert 0.05 < elapsed < 0.5


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
