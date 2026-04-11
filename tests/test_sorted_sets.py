"""Tests for sorted set commands: ZADD, ZREM, ZRANGE, ZRANGEBYSCORE, ZRANGESTORE, ZREMRANGEBYSCORE.

Covers requirements: ZSET-01, ZSET-02, ZSET-03, ZSET-04, ZSET-05, ZSET-06.
"""
import pytest
from burner_redis import BurnerRedis


# --- ZSET-01: ZADD ---


async def test_zadd_new_members(r):
    """ZSET-01: ZADD adds new members and returns count added."""
    result = await r.zadd("z", {"a": 1.0, "b": 2.0, "c": 3.0})
    assert result == 3


async def test_zadd_update_existing(r):
    """ZSET-01: ZADD updating existing member returns 0 (not new) but score is updated."""
    await r.zadd("z", {"a": 1.0})
    result = await r.zadd("z", {"a": 5.0})
    assert result == 0  # not new

    # Verify score was updated
    members = await r.zrange("z", 0, -1, withscores=True)
    assert members == [(b"a", 5.0)]


async def test_zadd_nx_flag(r):
    """ZSET-01: ZADD with nx=True only adds new members, skips existing."""
    await r.zadd("z", {"a": 1.0})
    result = await r.zadd("z", {"a": 5.0, "b": 2.0}, nx=True)
    assert result == 1  # only b added

    # Verify a still has original score
    members = await r.zrange("z", 0, -1, withscores=True)
    assert (b"a", 1.0) in members
    assert (b"b", 2.0) in members


async def test_zadd_xx_flag(r):
    """ZSET-01: ZADD with xx=True only updates existing members."""
    await r.zadd("z", {"a": 1.0})
    result = await r.zadd("z", {"a": 5.0, "b": 2.0}, xx=True)
    assert result == 0  # default counts new only, none new since xx prevents b

    # Verify a updated, b not added
    members = await r.zrange("z", 0, -1, withscores=True)
    assert members == [(b"a", 5.0)]


async def test_zadd_gt_flag(r):
    """ZSET-01: ZADD with gt=True only updates if new score > old."""
    await r.zadd("z", {"a": 5.0})

    # Try lower score -- should not update
    await r.zadd("z", {"a": 3.0}, gt=True)
    members = await r.zrange("z", 0, -1, withscores=True)
    assert members == [(b"a", 5.0)]

    # Try higher score -- should update
    await r.zadd("z", {"a": 7.0}, gt=True)
    members = await r.zrange("z", 0, -1, withscores=True)
    assert members == [(b"a", 7.0)]


async def test_zadd_lt_flag(r):
    """ZSET-01: ZADD with lt=True only updates if new score < old."""
    await r.zadd("z", {"a": 5.0})

    # Try higher score -- should not update
    await r.zadd("z", {"a": 7.0}, lt=True)
    members = await r.zrange("z", 0, -1, withscores=True)
    assert members == [(b"a", 5.0)]

    # Try lower score -- should update
    await r.zadd("z", {"a": 3.0}, lt=True)
    members = await r.zrange("z", 0, -1, withscores=True)
    assert members == [(b"a", 3.0)]


async def test_zadd_ch_flag(r):
    """ZSET-01: ZADD with ch=True returns count of changed (new + updated)."""
    await r.zadd("z", {"a": 1.0})
    result = await r.zadd("z", {"a": 2.0, "b": 3.0}, ch=True)
    assert result == 2  # a changed + b new


async def test_zadd_bytes_input(r):
    """ZSET-01: ZADD works with bytes keys and members."""
    result = await r.zadd(b"z", {b"a": 1.0, b"b": 2.0})
    assert result == 2

    members = await r.zrange(b"z", 0, -1)
    assert b"a" in members
    assert b"b" in members


async def test_zadd_wrongtype(r):
    """ZSET-01: ZADD on a string key raises WRONGTYPE error."""
    await r.set("strkey", "value")
    with pytest.raises(Exception, match="WRONGTYPE"):
        await r.zadd("strkey", {"member": 1.0})


# --- ZSET-02: ZREM ---


async def test_zrem_existing_members(r):
    """ZSET-02: ZREM removes members and returns count removed."""
    await r.zadd("z", {"a": 1.0, "b": 2.0, "c": 3.0})
    result = await r.zrem("z", "a", "c")
    assert result == 2


async def test_zrem_nonexistent_members(r):
    """ZSET-02: ZREM returns 0 for members not in set."""
    await r.zadd("z", {"a": 1.0})
    result = await r.zrem("z", "x", "y")
    assert result == 0


async def test_zrem_missing_key(r):
    """ZSET-02: ZREM returns 0 for non-existent key."""
    result = await r.zrem("no_key", "a")
    assert result == 0


async def test_zrem_wrongtype(r):
    """ZSET-02: ZREM on a string key raises WRONGTYPE error."""
    await r.set("strkey", "value")
    with pytest.raises(Exception, match="WRONGTYPE"):
        await r.zrem("strkey", "member")


async def test_zrem_verify_removal(r):
    """ZSET-02: After ZREM, member no longer appears in ZRANGE."""
    await r.zadd("z", {"a": 1.0, "b": 2.0, "c": 3.0})
    await r.zrem("z", "b")
    members = await r.zrange("z", 0, -1)
    assert b"b" not in members
    assert b"a" in members
    assert b"c" in members


# --- ZSET-03: ZRANGE ---


async def test_zrange_full_range(r):
    """ZSET-03: ZRANGE with 0, -1 returns all members in score order as list[bytes]."""
    await r.zadd("z", {"c": 3.0, "a": 1.0, "b": 2.0})
    result = await r.zrange("z", 0, -1)
    assert result == [b"a", b"b", b"c"]


async def test_zrange_subset(r):
    """ZSET-03: ZRANGE with partial range returns correct slice."""
    await r.zadd("z", {"a": 1.0, "b": 2.0, "c": 3.0, "d": 4.0})
    result = await r.zrange("z", 1, 2)
    assert result == [b"b", b"c"]


async def test_zrange_negative_indices(r):
    """ZSET-03: ZRANGE with negative indices returns last N members."""
    await r.zadd("z", {"a": 1.0, "b": 2.0, "c": 3.0, "d": 4.0})
    result = await r.zrange("z", -2, -1)
    assert result == [b"c", b"d"]


async def test_zrange_withscores(r):
    """ZSET-03: ZRANGE with withscores=True returns list of (bytes, float) tuples."""
    await r.zadd("z", {"a": 1.0, "b": 2.0, "c": 3.0})
    result = await r.zrange("z", 0, -1, withscores=True)
    assert result == [(b"a", 1.0), (b"b", 2.0), (b"c", 3.0)]


async def test_zrange_empty_key(r):
    """ZSET-03: ZRANGE returns empty list for non-existent key."""
    result = await r.zrange("no_key", 0, -1)
    assert result == []


async def test_zrange_out_of_range(r):
    """ZSET-03: ZRANGE with out-of-bounds indices returns empty or clamped result."""
    await r.zadd("z", {"a": 1.0, "b": 2.0})
    # Start beyond end
    result = await r.zrange("z", 5, 10)
    assert result == []


async def test_zrange_returns_list_type(r):
    """ZSET-03: ZRANGE return type is list."""
    await r.zadd("z", {"a": 1.0})
    result = await r.zrange("z", 0, -1)
    assert isinstance(result, list)


async def test_zrange_score_ordering(r):
    """ZSET-03: Members with different scores are returned in ascending score order."""
    await r.zadd("z", {"z_member": 1.0, "a_member": 2.0, "m_member": 3.0})
    result = await r.zrange("z", 0, -1)
    # Ordered by score, not alphabetically
    assert result == [b"z_member", b"a_member", b"m_member"]


async def test_zrange_wrongtype(r):
    """ZSET-03: ZRANGE on a string key raises WRONGTYPE error."""
    await r.set("strkey", "value")
    with pytest.raises(Exception, match="WRONGTYPE"):
        await r.zrange("strkey", 0, -1)


# --- ZSET-04: ZRANGEBYSCORE ---


async def test_zrangebyscore_range(r):
    """ZSET-04: ZRANGEBYSCORE returns members within score range."""
    await r.zadd("z", {"a": 1.0, "b": 2.0, "c": 3.0, "d": 4.0, "e": 5.0})
    result = await r.zrangebyscore("z", 2.0, 4.0)
    assert result == [b"b", b"c", b"d"]


async def test_zrangebyscore_inf(r):
    """ZSET-04: ZRANGEBYSCORE with -inf/+inf returns all members."""
    await r.zadd("z", {"a": 1.0, "b": 2.0, "c": 3.0})
    result = await r.zrangebyscore("z", "-inf", "+inf")
    assert result == [b"a", b"b", b"c"]


async def test_zrangebyscore_withscores(r):
    """ZSET-04: ZRANGEBYSCORE with withscores returns (bytes, float) tuples."""
    await r.zadd("z", {"a": 1.0, "b": 2.0, "c": 3.0})
    result = await r.zrangebyscore("z", 1.0, 2.0, withscores=True)
    assert result == [(b"a", 1.0), (b"b", 2.0)]


async def test_zrangebyscore_no_matches(r):
    """ZSET-04: ZRANGEBYSCORE with no matches returns empty list."""
    await r.zadd("z", {"a": 1.0, "b": 2.0})
    result = await r.zrangebyscore("z", 5.0, 10.0)
    assert result == []


async def test_zrangebyscore_empty_key(r):
    """ZSET-04: ZRANGEBYSCORE returns empty list for non-existent key."""
    result = await r.zrangebyscore("no_key", "-inf", "+inf")
    assert result == []


async def test_zrangebyscore_float_bounds(r):
    """ZSET-04: ZRANGEBYSCORE accepts float values for min/max."""
    await r.zadd("z", {"a": 1.5, "b": 2.5, "c": 3.5})
    result = await r.zrangebyscore("z", 1.5, 2.5)
    assert result == [b"a", b"b"]


async def test_zrangebyscore_wrongtype(r):
    """ZSET-04: ZRANGEBYSCORE on a string key raises WRONGTYPE error."""
    await r.set("strkey", "value")
    with pytest.raises(Exception, match="WRONGTYPE"):
        await r.zrangebyscore("strkey", "-inf", "+inf")


# --- ZSET-05: ZRANGESTORE ---


async def test_zrangestore_basic(r):
    """ZSET-05: ZRANGESTORE copies score range to new key, returns count stored."""
    await r.zadd("src", {"a": 1.0, "b": 2.0, "c": 3.0, "d": 4.0})
    result = await r.zrangestore("dst", "src", 2.0, 3.0)
    assert result == 2


async def test_zrangestore_verify_dest(r):
    """ZSET-05: Verify destination key contains correct members via zrange."""
    await r.zadd("src", {"a": 1.0, "b": 2.0, "c": 3.0, "d": 4.0})
    await r.zrangestore("dst", "src", 2.0, 3.0)
    members = await r.zrange("dst", 0, -1, withscores=True)
    assert members == [(b"b", 2.0), (b"c", 3.0)]


async def test_zrangestore_empty_range(r):
    """ZSET-05: ZRANGESTORE with empty range returns 0."""
    await r.zadd("src", {"a": 1.0, "b": 2.0})
    result = await r.zrangestore("dst", "src", 5.0, 10.0)
    assert result == 0


async def test_zrangestore_missing_src(r):
    """ZSET-05: ZRANGESTORE with missing source returns 0."""
    result = await r.zrangestore("dst", "no_key", 0.0, 10.0)
    assert result == 0


async def test_zrangestore_overwrites_dest(r):
    """ZSET-05: ZRANGESTORE replaces existing destination key."""
    await r.zadd("src", {"a": 1.0, "b": 2.0, "c": 3.0})
    await r.zadd("dst", {"x": 10.0, "y": 20.0})

    await r.zrangestore("dst", "src", 1.0, 2.0)
    members = await r.zrange("dst", 0, -1, withscores=True)
    assert members == [(b"a", 1.0), (b"b", 2.0)]


async def test_zrangestore_wrongtype(r):
    """ZSET-05: ZRANGESTORE raises WRONGTYPE if source is wrong type."""
    await r.set("strkey", "value")
    with pytest.raises(Exception, match="WRONGTYPE"):
        await r.zrangestore("dst", "strkey", 0.0, 10.0)


# --- ZSET-06: ZREMRANGEBYSCORE ---


async def test_zremrangebyscore_basic(r):
    """ZSET-06: ZREMRANGEBYSCORE removes members in score range, returns count."""
    await r.zadd("z", {"a": 1.0, "b": 2.0, "c": 3.0, "d": 4.0, "e": 5.0})
    result = await r.zremrangebyscore("z", 2.0, 4.0)
    assert result == 3


async def test_zremrangebyscore_verify_remaining(r):
    """ZSET-06: Verify remaining members after ZREMRANGEBYSCORE via zrange."""
    await r.zadd("z", {"a": 1.0, "b": 2.0, "c": 3.0, "d": 4.0, "e": 5.0})
    await r.zremrangebyscore("z", 2.0, 4.0)
    members = await r.zrange("z", 0, -1, withscores=True)
    assert members == [(b"a", 1.0), (b"e", 5.0)]


async def test_zremrangebyscore_no_matches(r):
    """ZSET-06: ZREMRANGEBYSCORE with no matches returns 0."""
    await r.zadd("z", {"a": 1.0, "b": 2.0})
    result = await r.zremrangebyscore("z", 5.0, 10.0)
    assert result == 0


async def test_zremrangebyscore_all_members(r):
    """ZSET-06: ZREMRANGEBYSCORE removing full range removes all members."""
    await r.zadd("z", {"a": 1.0, "b": 2.0, "c": 3.0})
    result = await r.zremrangebyscore("z", "-inf", "+inf")
    assert result == 3

    # Verify empty
    members = await r.zrange("z", 0, -1)
    assert members == []


async def test_zremrangebyscore_missing_key(r):
    """ZSET-06: ZREMRANGEBYSCORE on missing key returns 0."""
    result = await r.zremrangebyscore("no_key", 0.0, 10.0)
    assert result == 0


async def test_zremrangebyscore_wrongtype(r):
    """ZSET-06: ZREMRANGEBYSCORE on a string key raises WRONGTYPE error."""
    await r.set("strkey", "value")
    with pytest.raises(Exception, match="WRONGTYPE"):
        await r.zremrangebyscore("strkey", 0.0, 10.0)
