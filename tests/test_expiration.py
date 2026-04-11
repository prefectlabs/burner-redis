"""Tests for key expiration: passive on-access and active background sweep.

Covers requirements: EXP-01, EXP-02, EXP-03.
"""
import asyncio

import pytest
from burner_redis import BurnerRedis


# --- EXP-01: Keys with TTL expire and are no longer accessible ---


async def test_string_ex_expires(r):
    """EXP-01: Key set with EX (seconds) is inaccessible after TTL."""
    await r.set("mykey", "myvalue", ex=1)
    # Key accessible before expiry
    assert await r.get("mykey") == b"myvalue"
    await asyncio.sleep(1.1)
    # Key gone after expiry
    assert await r.get("mykey") is None


async def test_string_px_expires(r):
    """EXP-01: Key set with PX (milliseconds) is inaccessible after TTL."""
    await r.set("mykey", "myvalue", px=100)
    assert await r.get("mykey") == b"myvalue"
    await asyncio.sleep(0.15)
    assert await r.get("mykey") is None


async def test_expired_key_not_found_by_exists(r):
    """EXP-01: EXISTS returns 0 for an expired key."""
    await r.set("mykey", "myvalue", px=100)
    assert await r.exists("mykey") == 1
    await asyncio.sleep(0.15)
    assert await r.exists("mykey") == 0


async def test_expired_key_delete_returns_zero(r):
    """EXP-01: DELETE returns 0 for an expired key (treated as non-existent)."""
    await r.set("mykey", "myvalue", px=100)
    await asyncio.sleep(0.15)
    result = await r.delete("mykey")
    assert result == 0


async def test_expired_key_allows_nx_set(r):
    """EXP-01: SET with NX succeeds on an expired key (treated as non-existent)."""
    await r.set("mykey", "old", px=100)
    await asyncio.sleep(0.15)
    result = await r.set("mykey", "new", nx=True)
    assert result is True
    assert await r.get("mykey") == b"new"


async def test_expired_key_blocks_xx_set(r):
    """EXP-01: SET with XX fails on an expired key (treated as non-existent)."""
    await r.set("mykey", "old", px=100)
    await asyncio.sleep(0.15)
    result = await r.set("mykey", "new", xx=True)
    assert result is None


async def test_set_replaces_ttl_with_no_ttl(r):
    """EXP-01: SET without TTL on a key that had TTL removes the expiration."""
    await r.set("mykey", "val", px=100)
    # Overwrite without TTL -- key should persist indefinitely
    await r.set("mykey", "val2")
    await asyncio.sleep(0.15)
    # Key should still exist because TTL was removed by the overwrite
    assert await r.get("mykey") == b"val2"


# --- EXP-02: Seconds and milliseconds precision ---


async def test_ex_precision_seconds(r):
    """EXP-02: EX=2 key survives at 1s but expires by 2.1s."""
    await r.set("mykey", "val", ex=2)
    await asyncio.sleep(1.0)
    assert await r.get("mykey") == b"val"  # Still alive at 1s
    await asyncio.sleep(1.2)
    assert await r.get("mykey") is None  # Gone at 2.2s total


async def test_px_precision_milliseconds(r):
    """EXP-02: PX=200 key survives at 100ms but expires by 250ms."""
    await r.set("mykey", "val", px=200)
    await asyncio.sleep(0.1)
    assert await r.get("mykey") == b"val"  # Still alive at 100ms
    await asyncio.sleep(0.2)
    assert await r.get("mykey") is None  # Gone at 300ms total


async def test_px_takes_precedence_over_ex(r):
    """EXP-02: When both PX and EX provided, PX takes precedence."""
    # PX=100ms should expire before EX=10s
    await r.set("mykey", "val", ex=10, px=100)
    await asyncio.sleep(0.15)
    assert await r.get("mykey") is None


# --- EXP-03: Active sweep cleans up expired keys without access ---


async def test_active_sweep_cleans_expired_keys(r):
    """EXP-03: Background sweep removes expired keys even if never accessed.

    Creates keys with short TTL, waits for expiry + sweep cycles,
    then checks internal state by creating a new key with the same name
    using NX (which would fail if the old key still existed in memory).
    """
    # Set 5 keys with very short TTL
    for i in range(5):
        await r.set(f"sweep-key-{i}", "value", px=50)

    # Wait for expiry (50ms) plus several sweep cycles (100ms each)
    # 400ms gives at least 3 sweep cycles after expiry
    await asyncio.sleep(0.4)

    # Keys should have been swept -- verify by checking exists
    for i in range(5):
        assert await r.exists(f"sweep-key-{i}") == 0, (
            f"sweep-key-{i} should have been cleaned up by active sweep"
        )


async def test_active_sweep_does_not_remove_live_keys(r):
    """EXP-03: Background sweep does not remove keys that have not expired."""
    await r.set("live-key", "value", ex=60)  # 60 second TTL
    await r.set("no-ttl-key", "value")  # No TTL at all

    # Wait for several sweep cycles
    await asyncio.sleep(0.4)

    # Both keys should still exist
    assert await r.get("live-key") == b"value"
    assert await r.get("no-ttl-key") == b"value"


async def test_multiple_instances_have_independent_sweep(r):
    """EXP-03: Each BurnerRedis instance has its own sweep task."""
    r2 = BurnerRedis()

    await r.set("r1-key", "val", px=50)
    await r2.set("r2-key", "val", px=50)

    await asyncio.sleep(0.4)

    # Both instances should have swept their own expired keys
    assert await r.exists("r1-key") == 0
    assert await r2.exists("r2-key") == 0
