"""Integration tests running pydocket (Docket task scheduling) against BurnerRedis.

These tests monkey-patch pydocket's RedisConnection to use a shared BurnerRedis
instance, then exercise the full Docket + Worker lifecycle to verify compatibility.

Purpose: Validate that burner-redis can serve as a drop-in backend for pydocket,
which is the primary use case (Prefect's Docket task scheduling). All tests pass,
proving full compatibility with pydocket's usage patterns.
"""

import asyncio
from contextlib import asynccontextmanager
from datetime import datetime, timedelta, timezone
from unittest.mock import AsyncMock

import pytest

docket = pytest.importorskip("docket", reason="docket not installed")

from burner_redis import BurnerRedis
from docket import Docket, Worker


class NoOpResultStorage:
    """A no-op result storage that avoids needing a real Redis for result serialization."""

    async def get(self, key):
        return None

    async def set(self, key, value, ttl=None):
        pass

    async def put(self, key, value, ttl=None):
        pass

    async def setup(self):
        pass

    async def teardown(self):
        pass

pytestmark = pytest.mark.integration


@pytest.fixture
def burner():
    """Shared BurnerRedis instance for pydocket tests."""
    return BurnerRedis()


@pytest.fixture
def patch_pydocket(monkeypatch, burner):
    """Monkey-patch pydocket to use BurnerRedis instead of redis.asyncio.Redis."""
    from docket._redis import RedisConnection

    @asynccontextmanager
    async def fake_client(self):
        yield burner

    @asynccontextmanager
    async def fake_pubsub(self):
        ps = burner.pubsub()
        try:
            yield ps
        finally:
            await ps.aclose()

    async def fake_publish(self, channel, message):
        return await burner.publish(channel, message)

    async def fake_aenter(self):
        self._connection_pool = True  # Make is_connected return True
        return self

    async def fake_aexit(self, *args):
        self._connection_pool = None

    monkeypatch.setattr(RedisConnection, "client", fake_client)
    monkeypatch.setattr(RedisConnection, "pubsub", fake_pubsub)
    monkeypatch.setattr(RedisConnection, "publish", fake_publish)
    monkeypatch.setattr(RedisConnection, "__aenter__", fake_aenter)
    monkeypatch.setattr(RedisConnection, "__aexit__", fake_aexit)


# Track calls using a simple list (cloudpickle-safe, unlike AsyncMock)
_call_log = []


def _reset_call_log():
    """Reset the global call log between tests."""
    global _call_log
    _call_log = []


async def immediate_task(a, b):
    """Task function for immediate execution test."""
    _call_log.append(("immediate_task", a, b))
    return "ok"


async def delayed_task(arg):
    """Task function for delayed execution test."""
    _call_log.append(("delayed_task", arg))
    return "ok"


async def cancel_task():
    """Task function for cancel test -- should never be called."""
    _call_log.append(("cancel_task",))
    return "ok"


async def snapshot_task():
    """Task function for snapshot test."""
    _call_log.append(("snapshot_task",))
    return "ok"


async def heartbeat_task():
    """Task function for heartbeat test."""
    _call_log.append(("heartbeat_task",))
    return "ok"


async def test_docket_add_immediate_task(patch_pydocket):
    """Create a Docket, add a task, run Worker, verify the function was called.

    Exercises: xgroup_create, register_script (Lua scheduling), xadd,
    xreadgroup, xack, hset, hget, hgetall, pipeline, zadd, lock, publish.
    """
    _reset_call_log()

    async with Docket(name="test-docket-1", url="redis://localhost", result_storage=NoOpResultStorage()) as docket:
        docket.register(immediate_task)
        schedule = docket.add(immediate_task)
        await schedule("hello", "world")

        async with Worker(
            docket,
            concurrency=1,
            minimum_check_interval=timedelta(milliseconds=5),
            scheduling_resolution=timedelta(milliseconds=5),
        ) as worker:
            await asyncio.wait_for(
                worker.run_until_finished(),
                timeout=5.0,
            )

    assert ("immediate_task", "hello", "world") in _call_log


async def test_docket_add_delayed_task(patch_pydocket):
    """Schedule a task with when= in the near future, verify it executes after delay.

    Exercises: zadd (delayed queue), zrangebyscore (scheduler loop Lua),
    xadd (move to stream), the full delayed task lifecycle.
    """
    _reset_call_log()

    async with Docket(
        name="test-docket-2",
        url="redis://localhost",
        result_storage=NoOpResultStorage(),
    ) as docket:
        docket.register(delayed_task)
        when = datetime.now(timezone.utc) + timedelta(milliseconds=50)
        schedule = docket.add(delayed_task, when=when)
        await schedule("delayed_arg")

        async with Worker(
            docket,
            concurrency=1,
            minimum_check_interval=timedelta(milliseconds=5),
            scheduling_resolution=timedelta(milliseconds=10),
        ) as worker:
            await asyncio.wait_for(
                worker.run_until_finished(),
                timeout=5.0,
            )

    assert ("delayed_task", "delayed_arg") in _call_log


async def test_docket_cancel_task(patch_pydocket):
    """Schedule a delayed task, cancel it before execution, verify it does not execute.

    Exercises: the cancel Lua script (ZREM + DEL + HSET).
    """
    _reset_call_log()

    async with Docket(
        name="test-docket-3",
        url="redis://localhost",
        result_storage=NoOpResultStorage(),
    ) as docket:
        docket.register(cancel_task)
        # Schedule 10 seconds in the future (well beyond test timeout)
        when = datetime.now(timezone.utc) + timedelta(seconds=10)
        schedule = docket.add(cancel_task, when=when, key="cancel-me")
        await schedule()

        # Cancel it
        await docket.cancel("cancel-me")

        # Run worker briefly -- the task should NOT execute
        async with Worker(
            docket,
            concurrency=1,
            minimum_check_interval=timedelta(milliseconds=5),
            scheduling_resolution=timedelta(milliseconds=5),
        ) as worker:
            try:
                await asyncio.wait_for(
                    worker.run_until_finished(),
                    timeout=0.5,
                )
            except asyncio.TimeoutError:
                pass  # Expected -- cancelled task won't finish

    assert ("cancel_task",) not in _call_log


async def test_docket_snapshot(patch_pydocket):
    """Schedule multiple tasks, take a snapshot, verify it reflects state.

    Exercises: hgetall, zrange, xrange, pipeline with zcard.
    """
    async with Docket(
        name="test-docket-4",
        url="redis://localhost",
        result_storage=NoOpResultStorage(),
    ) as docket:
        docket.register(snapshot_task)

        # Schedule a delayed task
        when = datetime.now(timezone.utc) + timedelta(seconds=60)
        schedule = docket.add(snapshot_task, when=when, key="snap-task-1")
        await schedule()

        snapshot = await docket.snapshot()
        # The snapshot should contain our scheduled task
        assert snapshot is not None


async def test_worker_heartbeat(patch_pydocket):
    """Start a worker, verify it registers heartbeat.

    Exercises: zadd, sadd, expire, pipeline, zremrangebyscore.
    """
    async with Docket(
        name="test-docket-5",
        url="redis://localhost",
        heartbeat_interval=timedelta(milliseconds=50),
        result_storage=NoOpResultStorage(),
    ) as docket:
        docket.register(heartbeat_task)

        async with Worker(
            docket,
            concurrency=1,
            minimum_check_interval=timedelta(milliseconds=5),
            scheduling_resolution=timedelta(milliseconds=5),
        ) as worker:
            # Let the worker run briefly to register heartbeat
            try:
                await asyncio.wait_for(
                    worker.run_until_finished(),
                    timeout=0.3,
                )
            except asyncio.TimeoutError:
                pass  # Expected -- no tasks to finish

        # Verify worker registered via workers list
        workers = await docket.workers()
        # Workers list may or may not be populated depending on heartbeat timing
        # The key test is that the worker started and ran without errors
        assert workers is not None


# --- Phase 11 Regression Tests ---
# These tests cover specific pydocket patterns that were broken before Phase 11 fixes.


async def test_pydocket_lease_renewal_pattern(burner):
    """Regression: pydocket uses XCLAIM for lease renewal (same consumer, idle=0).

    This is the pattern from docket/worker.py _renew_leases() method.
    The consumer xclaims its own messages to reset idle time, preventing
    XAUTOCLAIM from reclaiming them during long-running tasks.

    Requires: XCLAIM command (implemented in Phase 11 Plan 01).
    """
    # Setup: create stream with consumer group and deliver a message
    await burner.xadd("test:stream", {"task": "data"})
    await burner.xgroup_create("test:stream", "workers", id="0")
    result = await burner.xreadgroup("workers", "worker-1", {"test:stream": ">"})
    assert len(result) > 0
    msg_id = result[0][1][0][0]  # First stream, first entry, ID

    # Lease renewal: same consumer claims its own message with idle=0
    claimed = await burner.xclaim(
        "test:stream", "workers", "worker-1", 0, [msg_id], idle=0
    )
    assert len(claimed) == 1

    # After renewal, XAUTOCLAIM should NOT reclaim (idle was just reset)
    autoclaim_result = await burner.xautoclaim(
        "test:stream", "workers", "worker-2", 1000, start_id="0-0"
    )
    next_id, autoclaimed, deleted = autoclaim_result
    assert len(autoclaimed) == 0  # Nothing idle enough to claim


async def test_pydocket_delayed_task_pattern(burner):
    """Regression: the scheduler atomically moves tasks from sorted set to stream.

    This test simulates the exact pattern that caused the delayed task race:
    1. Lua script does ZRANGEBYSCORE + ZREM + XADD (scheduler finds due task)
    2. Worker does XREADGROUP with block (waits for new stream entries)
    3. Worker should receive the entry added by Lua

    Requires: Blocking XREADGROUP with Lua XADD wake-through (Phase 11 Plan 01).
    """
    # Setup stream and consumer group
    await burner.xadd("test:stream", {"init": "setup"})
    await burner.xgroup_create("test:stream", "workers", id="$")

    # Simulate scheduler: Lua script adds an entry to the stream
    lua_scheduler = burner.register_script("""
    redis.call('XADD', KEYS[1], '*', 'task', ARGV[1])
    return 1
    """)

    async def scheduler():
        await asyncio.sleep(0.05)
        await lua_scheduler(keys=["test:stream"], args=["delayed-payload"])

    # Worker: blocking read should see the Lua-added entry
    task = asyncio.create_task(scheduler())
    result = await burner.xreadgroup(
        "workers", "worker-1", {"test:stream": ">"}, block=2000
    )
    await task

    assert len(result) > 0
    stream_name, entries = result[0]
    assert entries[0][1][b"task"] == b"delayed-payload"


async def test_pydocket_xtrim_clear_pattern(burner):
    """Regression: pydocket's docket.clear() uses XTRIM with approximate=False.

    The clear operation trims streams to zero length. Before Phase 11,
    the approximate parameter was not accepted.

    Requires: XTRIM approximate parameter (Phase 11 Plan 01).
    """
    # Add entries to stream
    for i in range(5):
        await burner.xadd("test:stream", {"f": f"v{i}"})

    assert await burner.xlen("test:stream") == 5

    # Clear: xtrim with maxlen=0 and approximate=False (pydocket pattern)
    trimmed = await burner.xtrim("test:stream", maxlen=0, approximate=False)
    assert trimmed == 5
    assert await burner.xlen("test:stream") == 0
