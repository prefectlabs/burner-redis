"""Integration tests simulating Prefect/Docket Redis usage patterns.

These tests exercise the exact Redis command patterns that Prefect's Docket task
scheduling library uses: sorted-set-based delayed queues, stream-based immediate
task delivery with consumer groups, hash-based execution state tracking, Lua scripts
for atomic multi-key operations, pipeline batching, and distributed locks.

Purpose: Verify burner-redis can serve as a drop-in backend for Prefect's Docket-based
task scheduling, and identify any missing commands or behavioral gaps with clear markers.
"""
import time

import pytest
from burner_redis import BurnerRedis


# =============================================================================
# 1. Sorted Set Queue (Docket's delayed task scheduling)
# =============================================================================


async def test_sorted_set_queue_schedule_and_retrieve(r):
    """Simulate Docket's scheduler loop: ZADD tasks with timestamps, find due tasks, dequeue.

    Models the scheduler_loop pattern where tasks are added to a sorted set keyed
    by their scheduled execution time, then ZRANGEBYSCORE finds tasks due now,
    and ZREM dequeues them.
    """
    queue_key = "docket:queue:default"

    # Schedule 3 tasks at different times
    now = time.time()
    await r.zadd(queue_key, {
        "task:send-email:abc123": now - 10,    # 10s overdue
        "task:process-data:def456": now - 5,   # 5s overdue
        "task:cleanup:ghi789": now + 300,      # 5min in the future
    })

    # Scheduler loop: find tasks due now (score <= current time)
    due_tasks = await r.zrangebyscore(queue_key, "-inf", str(now))
    assert len(due_tasks) == 2
    assert b"task:send-email:abc123" in due_tasks
    assert b"task:process-data:def456" in due_tasks
    # Future task should NOT be included
    assert b"task:cleanup:ghi789" not in due_tasks

    # Dequeue the due tasks
    for task in due_tasks:
        removed = await r.zrem(queue_key, task)
        assert removed == 1

    # Verify only the future task remains
    remaining = await r.zrange(queue_key, 0, -1)
    assert remaining == [b"task:cleanup:ghi789"]


async def test_sorted_set_queue_replace_existing_task(r):
    """Simulate Docket.replace(): ZREM old + ZADD new score for the same member.

    Models the pattern where a task's scheduled time is updated by removing
    and re-adding it with a new score.
    """
    queue_key = "docket:queue:default"
    task_id = "task:retry-webhook:xyz"

    # Schedule task at original time
    now = time.time()
    await r.zadd(queue_key, {task_id: now + 60})

    # Replace: remove old entry and add with new time
    await r.zrem(queue_key, task_id)
    await r.zadd(queue_key, {task_id: now + 120})

    # Verify the task has the new score
    result = await r.zrangebyscore(queue_key, str(now + 90), str(now + 150), withscores=True)
    assert len(result) == 1
    assert result[0][0] == task_id.encode()
    assert result[0][1] == now + 120


# =============================================================================
# 2. Stream-based Immediate Task Delivery (Docket's stream + consumer group)
# =============================================================================


async def test_stream_task_delivery_lifecycle(r):
    """Full lifecycle: XGROUP CREATE, XADD task, XREADGROUP to claim, XACK.

    Models Worker._worker_loop main path: create consumer group on stream,
    add task messages with Docket's field schema, read with consumer group,
    and acknowledge after processing.
    """
    stream_key = "docket:stream:default"

    # Create consumer group (mkstream creates the stream if it doesn't exist)
    await r.xgroup_create(stream_key, "workers", id="0", mkstream=True)

    # Producer: add a task message with Docket's field schema
    entry_id = await r.xadd(stream_key, {
        "key": "task:send-email:abc123",
        "when": str(time.time()),
        "function": "my_module.send_email",
        "args": '["user@example.com", "Hello"]',
        "kwargs": '{"priority": "high"}',
        "attempt": "1",
    })
    assert isinstance(entry_id, bytes)

    # Worker: read new messages via consumer group
    result = await r.xreadgroup("workers", "worker-1", {stream_key: ">"})
    assert result is not None
    stream_name, entries = result[0]
    assert stream_name == stream_key.encode()
    assert len(entries) == 1

    msg_id, fields = entries[0]
    assert fields[b"key"] == b"task:send-email:abc123"
    assert fields[b"function"] == b"my_module.send_email"
    assert fields[b"attempt"] == b"1"

    # Worker: acknowledge after processing
    ack_count = await r.xack(stream_key, "workers", msg_id.decode())
    assert ack_count == 1

    # Verify no pending messages remain
    pending = await r.xreadgroup("workers", "worker-1", {stream_key: "0"})
    assert pending is None


async def test_stream_redelivery_via_xautoclaim(r):
    """XAUTOCLAIM reclaims messages from a stalled consumer.

    Models Worker.get_redeliveries(): consumer1 reads but crashes, consumer2
    uses XAUTOCLAIM to pick up the orphaned messages.
    """
    stream_key = "docket:stream:default"

    # Setup: add tasks and read with consumer1
    await r.xgroup_create(stream_key, "workers", id="0", mkstream=True)
    await r.xadd(stream_key, {"key": "task:a", "attempt": "1"})
    await r.xadd(stream_key, {"key": "task:b", "attempt": "1"})

    # consumer1 reads (simulating it starting work then crashing)
    result = await r.xreadgroup("workers", "consumer1", {stream_key: ">"})
    assert result is not None
    assert len(result[0][1]) == 2

    # consumer2 autoclaims idle messages (min_idle_time=0 for immediate claim)
    next_id, claimed, deleted = await r.xautoclaim(
        stream_key, "workers", "consumer2", 0, start_id="0-0"
    )
    assert len(claimed) == 2
    assert claimed[0][1][b"key"] == b"task:a"
    assert claimed[1][1][b"key"] == b"task:b"


async def test_stream_consumer_group_auto_create_on_nogroup(r):
    """NOGROUP recovery: create stream via XADD, handle NOGROUP, then XGROUP CREATE.

    Models the NOGROUP recovery pattern in get_new_deliveries/get_redeliveries:
    attempt XREADGROUP, catch NOGROUP error, create group, retry.
    """
    stream_key = "docket:stream:recovery"

    # Create stream via XADD (no prior XGROUP CREATE)
    await r.xadd(stream_key, {"key": "task:first", "attempt": "1"})

    # Attempt XREADGROUP without creating the group first -- expect NOGROUP
    with pytest.raises(Exception, match="NOGROUP"):
        await r.xreadgroup("workers", "worker-1", {stream_key: ">"})

    # Recovery: create the group and retry
    await r.xgroup_create(stream_key, "workers", id="0")
    result = await r.xreadgroup("workers", "worker-1", {stream_key: ">"})
    assert result is not None
    assert len(result[0][1]) == 1
    assert result[0][1][0][1][b"key"] == b"task:first"


# =============================================================================
# 3. Hash-based Execution State (Docket's runs:key hash)
# =============================================================================


async def test_execution_state_write_and_read_fields(r):
    """HSET runs_key with Docket's execution state fields, HGET each individually.

    Models Execution state tracking: when a task is scheduled, its full state
    is written to a hash keyed by the task's run ID.
    """
    runs_key = "docket:runs:task:send-email:abc123"

    # Write execution state (Docket's field schema)
    fields_written = await r.hset(runs_key, mapping={
        "state": "scheduled",
        "when": "1700000000.0",
        "known": "task:send-email:abc123",
        "stream_id": "1700000000000-0",
        "function": "my_module.send_email",
        "args": '["user@example.com"]',
        "kwargs": '{"priority": "high"}',
    })
    assert fields_written == 7

    # Read each field individually (the way Docket uses HGET)
    assert await r.hget(runs_key, "state") == b"scheduled"
    assert await r.hget(runs_key, "when") == b"1700000000.0"
    assert await r.hget(runs_key, "known") == b"task:send-email:abc123"
    assert await r.hget(runs_key, "stream_id") == b"1700000000000-0"
    assert await r.hget(runs_key, "function") == b"my_module.send_email"
    assert await r.hget(runs_key, "args") == b'["user@example.com"]'
    assert await r.hget(runs_key, "kwargs") == b'{"priority": "high"}'


@pytest.mark.xfail(reason="hgetall not yet implemented", raises=AttributeError, strict=True)
async def test_execution_state_hgetall(r):
    """HSET multiple fields, then HGETALL to read all at once.

    Models Execution.sync() and Docket.get_execution() which read all
    fields of the execution state hash in a single call.
    """
    runs_key = "docket:runs:task:sync-test"
    await r.hset(runs_key, mapping={
        "state": "running",
        "function": "my_module.sync_task",
    })
    result = await r.hgetall(runs_key)
    assert result[b"state"] == b"running"
    assert result[b"function"] == b"my_module.sync_task"


@pytest.mark.xfail(reason="hexists not yet implemented", raises=AttributeError, strict=True)
async def test_execution_state_hexists(r):
    """HSET with known field, then HEXISTS to check presence.

    Models the known_exists check in the schedule Lua script which
    verifies whether a task is already known before scheduling.
    """
    runs_key = "docket:runs:task:exists-test"
    await r.hset(runs_key, key="known", value="task:exists-test")
    result = await r.hexists(runs_key, "known")
    assert result is True


async def test_execution_state_transition(r):
    """HSET state=scheduled, then HSET state=running -- single field overwrite.

    Models claim() state transition: when a worker claims a task, it
    overwrites the state field from 'scheduled' to 'running'.
    """
    runs_key = "docket:runs:task:transition-test"

    # Initial state: scheduled
    await r.hset(runs_key, key="state", value="scheduled")
    assert await r.hget(runs_key, "state") == b"scheduled"

    # Worker claims task: transition to running
    await r.hset(runs_key, key="state", value="running")
    assert await r.hget(runs_key, "state") == b"running"


# =============================================================================
# 4. Pipeline Batching (Docket's check_for_work and ack_message)
# =============================================================================


async def test_pipeline_check_for_work(r):
    """Pipeline with xlen to check stream depth.

    Models Worker.check_for_work() which uses a pipeline to efficiently
    query stream length. Note: zcard is not yet available.
    """
    stream_key = "docket:stream:default"

    # Setup: add some tasks
    await r.xadd(stream_key, {"key": "task:a"})
    await r.xadd(stream_key, {"key": "task:b"})

    # Pipeline check: get stream length
    pipe = r.pipeline()
    pipe.xlen(stream_key)
    results = await pipe.execute()

    assert results[0] == 2  # xlen returns entry count


@pytest.mark.xfail(reason="zcard not yet implemented", raises=AttributeError, strict=True)
async def test_pipeline_check_for_work_with_zcard(r):
    """Pipeline with zcard to check queue depth.

    Models the full Worker.check_for_work() which includes ZCARD
    on the delayed queue sorted set.
    """
    queue_key = "docket:queue:default"
    await r.zadd(queue_key, {"task:a": 1.0, "task:b": 2.0})

    pipe = r.pipeline()
    pipe.zcard(queue_key)
    results = await pipe.execute()
    assert results[0] == 2


async def test_pipeline_ack_message(r):
    """Pipeline with xack for message acknowledgment.

    Models Worker.ack_message() which uses a pipeline to acknowledge
    processed messages. Note: xdel is not yet available in pipeline.
    """
    stream_key = "docket:stream:ack-test"

    # Setup: create group, add message, read it
    await r.xgroup_create(stream_key, "workers", id="0", mkstream=True)
    msg_id = await r.xadd(stream_key, {"key": "task:ack-me"})
    await r.xreadgroup("workers", "worker-1", {stream_key: ">"})

    # Pipeline ack
    pipe = r.pipeline()
    pipe.xack(stream_key, "workers", msg_id.decode())
    results = await pipe.execute()

    assert results[0] == 1  # 1 message acknowledged


async def test_pipeline_clear_docket(r):
    """Pipeline with xtrim(maxlen=0) + delete for clearing a Docket queue.

    Models Docket.clear() which trims the stream to zero length and
    deletes associated keys.
    """
    stream_key = "docket:stream:clear-test"
    queue_key = "docket:queue:clear-test"

    # Setup: populate stream and sorted set
    await r.xadd(stream_key, {"key": "task:a"})
    await r.xadd(stream_key, {"key": "task:b"})
    await r.zadd(queue_key, {"task:c": 100.0})

    # Pipeline clear: trim stream to 0 + delete queue key
    pipe = r.pipeline()
    pipe.xtrim(stream_key, maxlen=0)
    pipe.delete(queue_key)
    results = await pipe.execute()

    assert results[0] == 2  # xtrim removed 2 entries
    assert results[1] == 1  # delete removed 1 key

    # Verify both are empty
    assert await r.xlen(stream_key) == 0
    assert await r.exists(queue_key) == 0


# =============================================================================
# 5. Lua Script Atomic Operations (Docket's schedule and cancel scripts)
# =============================================================================


async def test_lua_schedule_immediate_task(r):
    """Lua script: XADD + HSET atomically for immediate task scheduling.

    Simplified version of Docket's schedule script for the immediate path:
    atomically add a task to the stream and write its execution state hash.
    """
    stream_key = "docket:stream:default"
    runs_key = "docket:runs:task:immediate-test"

    script = """
    local stream_key = KEYS[1]
    local runs_key = KEYS[2]
    local task_key = ARGV[1]
    local function_name = ARGV[2]
    local args = ARGV[3]
    local kwargs = ARGV[4]

    -- Add to stream for immediate delivery
    local stream_id = redis.call('XADD', stream_key, '*', 'key', task_key, 'function', function_name)

    -- Write execution state
    redis.call('HSET', runs_key, 'state', 'queued', 'function', function_name,
               'args', args, 'kwargs', kwargs, 'stream_id', stream_id)

    return stream_id
    """

    result = await r.eval(
        script, 2, stream_key, runs_key,
        "task:immediate-test", "my_module.do_work", '["arg1"]', '{"k": "v"}'
    )

    # Verify stream entry was created
    assert isinstance(result, bytes)
    assert b"-" in result

    # Verify hash state was written
    assert await r.hget(runs_key, "state") == b"queued"
    assert await r.hget(runs_key, "function") == b"my_module.do_work"
    assert await r.hget(runs_key, "args") == b'["arg1"]'
    assert await r.hget(runs_key, "stream_id") == result


async def test_lua_schedule_delayed_task(r):
    """Lua script: HSET (park data) + ZADD (queue) + HSET (runs state) atomically.

    Models the scheduled (non-immediate) branch of Docket's schedule script:
    park the task data in a hash, add to the delayed queue sorted set, and
    write the execution state.
    """
    park_key = "docket:park:task:delayed-test"
    queue_key = "docket:queue:default"
    runs_key = "docket:runs:task:delayed-test"

    script = """
    local park_key = KEYS[1]
    local queue_key = KEYS[2]
    local runs_key = KEYS[3]
    local task_key = ARGV[1]
    local when = ARGV[2]
    local function_name = ARGV[3]

    -- Park the task data for later retrieval
    redis.call('HSET', park_key, 'key', task_key, 'function', function_name)

    -- Add to delayed queue with scheduled time as score
    redis.call('ZADD', queue_key, when, task_key)

    -- Write execution state
    redis.call('HSET', runs_key, 'state', 'scheduled', 'when', when, 'known', task_key)

    return 1
    """

    result = await r.eval(
        script, 3, park_key, queue_key, runs_key,
        "task:delayed-test", "1700000060.0", "my_module.delayed_work"
    )
    assert result == 1

    # Verify park data
    assert await r.hget(park_key, "key") == b"task:delayed-test"
    assert await r.hget(park_key, "function") == b"my_module.delayed_work"

    # Verify queue entry
    queue_entries = await r.zrangebyscore(queue_key, "-inf", "+inf", withscores=True)
    assert len(queue_entries) == 1
    assert queue_entries[0][0] == b"task:delayed-test"
    assert queue_entries[0][1] == 1700000060.0

    # Verify execution state
    assert await r.hget(runs_key, "state") == b"scheduled"
    assert await r.hget(runs_key, "known") == b"task:delayed-test"


async def test_lua_cancel_task(r):
    """Lua script: ZREM + DEL + HSET(state=cancelled) atomically.

    Models a simplified version of Docket._cancel script: remove from queue,
    delete parked data, and mark execution as cancelled.
    """
    queue_key = "docket:queue:default"
    park_key = "docket:park:task:cancel-test"
    runs_key = "docket:runs:task:cancel-test"

    # Setup: schedule a task first
    await r.zadd(queue_key, {"task:cancel-test": 1700000060.0})
    await r.hset(park_key, mapping={"key": "task:cancel-test", "function": "my_module.work"})
    await r.hset(runs_key, mapping={"state": "scheduled", "known": "task:cancel-test"})

    script = """
    local queue_key = KEYS[1]
    local park_key = KEYS[2]
    local runs_key = KEYS[3]
    local task_key = ARGV[1]

    -- Remove from delayed queue
    redis.call('ZREM', queue_key, task_key)

    -- Delete parked data
    redis.call('DEL', park_key)

    -- Mark as cancelled
    redis.call('HSET', runs_key, 'state', 'cancelled')

    return 1
    """

    result = await r.eval(
        script, 3, queue_key, park_key, runs_key,
        "task:cancel-test"
    )
    assert result == 1

    # Verify removal from queue
    remaining = await r.zrange(queue_key, 0, -1)
    assert remaining == []

    # Verify park data deleted
    assert await r.exists(park_key) == 0

    # Verify state is cancelled
    assert await r.hget(runs_key, "state") == b"cancelled"


@pytest.mark.xfail(
    reason="HGETALL not yet available in Lua dispatch",
    strict=True,
)
async def test_lua_move_due_tasks_to_stream(r):
    """Lua script: ZRANGEBYSCORE + HGETALL + XADD + DEL + HSET for each due task.

    Models the scheduler_loop Lua script: find due tasks in the sorted set,
    retrieve their parked data via HGETALL, add to stream, delete park key,
    and update execution state.
    """
    queue_key = "docket:queue:default"
    stream_key = "docket:stream:default"

    now = time.time()

    # Setup: park two tasks and add them to the queue as overdue
    for i in range(2):
        task_id = f"task:due-{i}"
        park_key = f"docket:park:{task_id}"
        runs_key = f"docket:runs:{task_id}"
        await r.hset(park_key, mapping={"key": task_id, "function": f"module.func_{i}"})
        await r.hset(runs_key, mapping={"state": "scheduled"})
        await r.zadd(queue_key, {task_id: now - 10})  # 10s overdue

    script = """
    local queue_key = KEYS[1]
    local stream_key = KEYS[2]
    local now = ARGV[1]

    -- Find due tasks
    local due = redis.call('ZRANGEBYSCORE', queue_key, '-inf', now)
    local moved = 0

    for i, task_key in ipairs(due) do
        local park_key = 'docket:park:' .. task_key
        local runs_key = 'docket:runs:' .. task_key

        -- Get parked data (requires HGETALL)
        local data = redis.call('HGETALL', park_key)

        -- Add to stream
        redis.call('XADD', stream_key, '*', 'key', task_key, unpack(data))

        -- Delete parked data
        redis.call('DEL', park_key)

        -- Update state
        redis.call('HSET', runs_key, 'state', 'queued')

        -- Remove from queue
        redis.call('ZREM', queue_key, task_key)

        moved = moved + 1
    end

    return moved
    """

    result = await r.eval(script, 2, queue_key, stream_key, str(now))
    assert result == 2


# =============================================================================
# 6. Lock-based Coordination (Docket's per-task-key locking)
# =============================================================================


async def test_lock_for_task_scheduling(r):
    """Acquire lock, perform schedule operations, release lock.

    Models Execution.schedule()'s lock pattern: acquire a lock on the
    task's known key to prevent concurrent scheduling of the same task,
    perform HSET + XADD or ZADD, then release.
    """
    lock_name = "docket:known:task:lock-test:lock"
    stream_key = "docket:stream:default"
    runs_key = "docket:runs:task:lock-test"

    # Acquire lock
    lock = r.lock(lock_name, timeout=10)
    acquired = await lock.acquire()
    assert acquired is True

    # Perform schedule operations while holding lock
    entry_id = await r.xadd(stream_key, {
        "key": "task:lock-test",
        "function": "my_module.locked_work",
    })
    await r.hset(runs_key, mapping={
        "state": "queued",
        "stream_id": entry_id.decode(),
    })

    # Release lock
    await lock.release()

    # Verify operations completed
    assert await r.hget(runs_key, "state") == b"queued"
    assert await r.hget(runs_key, "stream_id") == entry_id

    # Verify lock is released (can acquire again)
    lock2 = r.lock(lock_name, timeout=10)
    acquired2 = await lock2.acquire()
    assert acquired2 is True
    await lock2.release()


# =============================================================================
# 7. Missing Command Identification
# =============================================================================


@pytest.mark.xfail(reason="hgetall not yet implemented", raises=AttributeError, strict=True)
async def test_missing_hgetall(r):
    """Documents that HGETALL Python API method is needed for Docket compatibility.

    Used by: Execution.sync(), Docket.get_execution(), scheduler_loop Lua script.
    """
    await r.hset("key", key="field", value="value")
    await r.hgetall("key")


@pytest.mark.xfail(reason="hexists not yet implemented", raises=AttributeError, strict=True)
async def test_missing_hexists(r):
    """Documents that HEXISTS Python API method is needed for Docket compatibility.

    Used by: schedule Lua script's known_exists check.
    """
    await r.hset("key", key="field", value="value")
    await r.hexists("key", "field")


@pytest.mark.xfail(reason="zcard not yet implemented", raises=AttributeError, strict=True)
async def test_missing_zcard(r):
    """Documents that ZCARD Python API method is needed for Docket compatibility.

    Used by: Worker.check_for_work() to get queue depth.
    """
    await r.zadd("key", {"member": 1.0})
    await r.zcard("key")


@pytest.mark.xfail(reason="expire not yet implemented", raises=AttributeError, strict=True)
async def test_missing_expire(r):
    """Documents that EXPIRE Python API method is needed for Docket compatibility.

    Used by: Various Docket patterns for key TTL management.
    """
    await r.set("key", "value")
    await r.expire("key", 60)


@pytest.mark.xfail(reason="xdel not yet implemented", raises=AttributeError, strict=True)
async def test_missing_xdel(r):
    """Documents that XDEL Python API method is needed for Docket compatibility.

    Used by: Worker.ack_message() to delete processed stream entries.
    """
    entry_id = await r.xadd("stream", {"f": "v"})
    await r.xdel("stream", entry_id.decode())


@pytest.mark.xfail(reason="xrange not yet implemented", raises=AttributeError, strict=True)
async def test_missing_xrange(r):
    """Documents that XRANGE Python API method is needed for Docket compatibility.

    Used by: Various Docket debugging and inspection patterns.
    """
    await r.xadd("stream", {"f": "v"})
    await r.xrange("stream", "-", "+")
