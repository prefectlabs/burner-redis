---
phase: quick
plan: 260413-vbg
type: execute
wave: 1
depends_on: []
files_modified:
  - tests/test_prefect_integration.py
autonomous: true
must_haves:
  truths:
    - "PUB/SUB subscribe-publish-receive cycle works as an integration pattern alongside Docket workflows"
    - "Pattern-based pub/sub (PSUBSCRIBE) works for event notification use cases"
    - "Lua scripts can PUBLISH messages to notify subscribers of task state changes"
    - "Pipeline PUBLISH works for batched notification delivery"
    - "PUB/SUB can be used alongside existing Docket patterns (streams, sorted sets, hashes) without interference"
  artifacts:
    - path: "tests/test_prefect_integration.py"
      provides: "Integration tests with new PUB/SUB section added"
      contains: "PUB/SUB Event Notifications"
  key_links:
    - from: "tests/test_prefect_integration.py"
      to: "burner_redis.PubSub"
      via: "r.pubsub() factory"
      pattern: "r\\.pubsub\\("
---

<objective>
Add PUB/SUB integration test coverage to the existing Prefect/Docket integration test
suite. These tests demonstrate pub/sub as a companion to Docket's stream-based task
scheduling -- publishing event notifications when tasks are scheduled, completed, or
cancelled, and subscribing to those events for monitoring and coordination.

Purpose: Prove burner-redis's pub/sub works in realistic Prefect-adjacent patterns
where pub/sub complements the existing Docket workflows (streams, sorted sets, hashes,
Lua scripts, pipelines).

Output: Updated tests/test_prefect_integration.py with new section 8
</objective>

<execution_context>
@$HOME/.claude/get-shit-done/workflows/execute-plan.md
@$HOME/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@tests/test_prefect_integration.py
@tests/test_pubsub.py
@tests/conftest.py
@python/burner_redis/__init__.py
@python/burner_redis/pubsub.py
@python/burner_redis/pipeline.py
</context>

<tasks>

<task type="auto">
  <name>Task 1: Add PUB/SUB integration tests to test_prefect_integration.py</name>
  <files>tests/test_prefect_integration.py</files>
  <action>
Add a new section 8 to tests/test_prefect_integration.py after the existing section 7
(Missing Command Identification). Add `asyncio` to the imports at the top of the file.

The new section header should be:

```python
# =============================================================================
# 8. PUB/SUB Event Notifications (task lifecycle notifications via pub/sub)
# =============================================================================
```

Add the following integration tests that demonstrate pub/sub used alongside Docket
workflows for event-driven coordination:

**test_pubsub_task_scheduled_notification(r):**
Simulate publishing a notification when a task is scheduled via the sorted-set queue.
- Create a PubSub subscriber on channel "docket:events:default" with
  ignore_subscribe_messages=True
- ZADD a task to the queue, then PUBLISH a notification to "docket:events:default" with
  message "scheduled:task:notify-test"
- await asyncio.sleep(0.1) for delivery
- get_message(timeout=1.0) and verify type="message", channel=b"docket:events:default",
  data=b"scheduled:task:notify-test"
- aclose() the pubsub
- Docstring: Models a pattern where the scheduler publishes event notifications after
  enqueuing tasks, allowing monitoring systems to react to task scheduling events.

**test_pubsub_task_completed_notification_pattern(r):**
Simulate subscribing to a glob pattern for task lifecycle events.
- Create a PubSub subscriber with psubscribe("docket:events:*") and
  ignore_subscribe_messages=True
- PUBLISH to "docket:events:default" with message "completed:task:abc123"
- await asyncio.sleep(0.1) for delivery
- get_message(timeout=1.0) and verify type="pmessage", pattern=b"docket:events:*",
  channel=b"docket:events:default", data=b"completed:task:abc123"
- aclose() the pubsub
- Docstring: Models a monitoring system that uses pattern subscriptions to observe
  task lifecycle events across all Docket queues.

**test_pubsub_with_stream_task_lifecycle(r):**
Full lifecycle combining streams and pub/sub: schedule via stream, process, publish
completion event.
- Create PubSub subscriber on "docket:events:default" with ignore_subscribe_messages=True
- XGROUP CREATE stream, XADD task message, XREADGROUP to claim, XACK to acknowledge
- After XACK, PUBLISH "completed:task:lifecycle-test" to "docket:events:default"
- await asyncio.sleep(0.1) for delivery
- get_message(timeout=1.0) and verify the completion notification was received with
  data=b"completed:task:lifecycle-test"
- aclose() the pubsub
- Docstring: Models a worker that publishes a completion event after processing a task
  from the stream, enabling external observers to track task progress.

**test_lua_publish_task_event(r):**
Lua script that atomically writes state and publishes a notification.
- Create PubSub subscriber on "docket:events:default" with ignore_subscribe_messages=True
- Run a Lua script with KEYS=[runs_key, event_channel] and ARGV=[task_key, state]:
  ```lua
  local runs_key = KEYS[1]
  local event_channel = KEYS[2]
  local task_key = ARGV[1]
  local state = ARGV[2]
  redis.call('HSET', runs_key, 'state', state)
  return redis.call('PUBLISH', event_channel, state .. ':' .. task_key)
  ```
- Use runs_key="docket:runs:task:lua-event-test", event_channel="docket:events:default",
  task_key="task:lua-event-test", state="completed"
- Verify HSET wrote the state, and the PUBLISH return value is an int (subscriber count)
- await asyncio.sleep(0.1) for delivery
- get_message(timeout=1.0) and verify data=b"completed:task:lua-event-test"
- aclose() the pubsub
- Docstring: Models Docket's atomic Lua scripts extended with PUBLISH -- after atomically
  updating task state, a notification is sent so watchers are informed without polling.

**test_pipeline_publish_task_events(r):**
Pipeline that performs task operations and publishes multiple event notifications.
- Create PubSub subscriber on "docket:events:default" with ignore_subscribe_messages=True
- Create a pipeline, add:
  - hset("docket:runs:task:pipe-event", key="state", value="completed")
  - publish("docket:events:default", b"completed:task:pipe-event-1")
  - publish("docket:events:default", b"completed:task:pipe-event-2")
- Execute pipeline, verify results[0] is an int (hset count), results[1] and results[2]
  are ints (publish subscriber counts)
- await asyncio.sleep(0.1) for delivery
- Collect two messages via get_message(timeout=1.0) and verify both arrived with correct
  data values
- aclose() the pubsub
- Docstring: Models batched task completion with notifications -- a worker pipeline that
  acknowledges multiple tasks and publishes events for each in a single batch.

**test_pubsub_multiple_queue_monitoring(r):**
Subscribe to events from multiple queues simultaneously using pattern subscribe.
- Create PubSub subscriber with psubscribe("docket:events:*") and
  ignore_subscribe_messages=True
- PUBLISH to "docket:events:queue-a" with "scheduled:task:a"
- PUBLISH to "docket:events:queue-b" with "completed:task:b"
- await asyncio.sleep(0.1) for delivery
- Collect 2 messages via get_message, verify they came from different channels
  (docket:events:queue-a and docket:events:queue-b) and have correct data
- aclose() the pubsub
- Docstring: Models a centralized monitoring system that observes task events across
  multiple Docket queues using a single pattern subscription.

Follow existing test file conventions: docstrings on every test, clear section comment
headers, use the `r` fixture from conftest.py, all tests are async (auto mode).
Use `pytestmark = pytest.mark.integration` already set at module level.
  </action>
  <verify>
    <automated>cd /Users/alexander/dev/prefectlabs/burner-redis && uv run python -m pytest tests/test_prefect_integration.py -v -m integration 2>&1 | tail -50</automated>
  </verify>
  <done>
    - tests/test_prefect_integration.py has new section 8 with 6 PUB/SUB integration tests
    - All 6 new pub/sub tests pass
    - All 14 existing passing integration tests still pass (no regressions)
    - All 10 existing xfail tests still xfail correctly
    - Tests demonstrate pub/sub alongside Docket patterns: sorted sets, streams, hashes, Lua, pipelines
  </done>
</task>

</tasks>

<verification>
Run the full integration test suite:
```
cd /Users/alexander/dev/prefectlabs/burner-redis && uv run python -m pytest tests/test_prefect_integration.py -v -m integration
```

Run the full test suite to verify no regressions:
```
cd /Users/alexander/dev/prefectlabs/burner-redis && uv run python -m pytest tests/ -v
```
</verification>

<success_criteria>
- tests/test_prefect_integration.py has section 8 with 6 pub/sub integration tests
- All new tests pass (20 passing total, up from 14)
- All existing xfail tests remain xfail (10 xfailed)
- Full test suite passes without regressions
- Tests cover: subscribe+publish, psubscribe patterns, pub/sub with streams lifecycle, Lua PUBLISH, pipeline PUBLISH, multi-queue monitoring
</success_criteria>

<output>
After completion, create `.planning/quick/260413-vbg-update-the-integration-tests-that-ensure/260413-vbg-SUMMARY.md`
</output>
