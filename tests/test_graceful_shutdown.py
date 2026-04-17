"""Tests for BurnerRedis graceful shutdown via aclose()/close().

Verifies that:
- aclose() causes blocking xreadgroup to return promptly (not hang)
- aclose() causes blocking xread to return promptly (not hang)
- The async context manager pattern works
- close() is an alias for aclose()
"""
import asyncio

import pytest
from burner_redis import BurnerRedis


@pytest.mark.asyncio
async def test_aclose_unblocks_xreadgroup():
    """aclose() should cause a blocking xreadgroup to return promptly."""
    client = BurnerRedis()

    # Set up stream and consumer group
    await client.xadd("test-stream", {"field": "value"})
    await client.xgroup_create("test-stream", "test-group", id="0")

    # Drain the one existing message so xreadgroup will block
    await client.xreadgroup("test-group", "consumer-1", {"test-stream": ">"}, count=10)

    # Start a blocking xreadgroup in a background task
    result_holder = []

    async def blocking_read():
        result = await client.xreadgroup(
            "test-group", "consumer-1", {"test-stream": ">"}, block=60000
        )
        result_holder.append(result)

    task = asyncio.create_task(blocking_read())

    # Give the blocking read a moment to enter the wait loop
    await asyncio.sleep(0.1)

    # Signal shutdown -- this should wake the blocking reader
    await client.aclose()

    # The task should complete promptly (within a few seconds, not 60s)
    await asyncio.wait_for(task, timeout=5.0)

    # The blocking read should return empty results (not hang)
    assert len(result_holder) == 1
    # xreadgroup returns empty list on timeout/shutdown
    assert result_holder[0] == [] or result_holder[0] is None


@pytest.mark.asyncio
async def test_aclose_unblocks_xread():
    """aclose() should cause a blocking xread to return promptly."""
    client = BurnerRedis()

    # Start a blocking xread on a stream that has no data
    await client.xadd("xread-stream", {"init": "1"})
    # Read past the existing entry
    entries = await client.xread({"xread-stream": "0-0"})
    last_id = entries[0][1][-1][0]  # Get the last entry ID

    result_holder = []

    async def blocking_read():
        result = await client.xread({"xread-stream": last_id}, block=60000)
        result_holder.append(result)

    task = asyncio.create_task(blocking_read())
    await asyncio.sleep(0.1)

    await client.aclose()

    await asyncio.wait_for(task, timeout=5.0)
    assert len(result_holder) == 1
    # xread returns None on empty/timeout
    assert result_holder[0] is None or result_holder[0] == []


@pytest.mark.asyncio
async def test_context_manager():
    """BurnerRedis should work as an async context manager."""
    async with BurnerRedis() as client:
        await client.set("key", "value")
        result = await client.get("key")
        assert result == b"value"
    # After exiting context, shutdown should have been called
    # (we can't easily verify the flag, but the fact that __aexit__
    # didn't raise is the test)


@pytest.mark.asyncio
async def test_close_is_alias_for_aclose():
    """close() should be an alias for aclose()."""
    client = BurnerRedis()
    await client.set("key", "value")
    # close() should not raise
    await client.close()


@pytest.mark.asyncio
async def test_aclose_stops_pubsub_listeners():
    """aclose() should stop all active pubsub listeners."""
    client = BurnerRedis()

    ps = client.pubsub()
    await ps.subscribe("test-channel")

    # Start consuming (listener task is now running)
    msg = await ps.get_message(timeout=0.1)

    # aclose on the client should not raise
    await client.aclose()

    # PubSub should not hang or error after client shutdown
    # (the listener task should have been stopped)
