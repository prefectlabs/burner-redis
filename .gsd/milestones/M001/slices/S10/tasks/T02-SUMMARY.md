# T02: Implement the Python PubSub class mirroring redis-py's async PubSub interface, integrate PUBLISH into Pipeline and Lua scripting, and provide a comprehensive test suite covering all 13 CONTEXT.

**Slice:** S10 — **Milestone:** M001

## Legacy Summary

---
phase: 10-add-pub-sub-support
plan: 02
subsystem: python-pubsub-api
tags: [pubsub, python, pipeline, lua, testing]
dependency_graph:
  requires: [10-01]
  provides: [pubsub-python-api, pipeline-publish, lua-publish]
  affects: [python/burner_redis/pubsub.py, python/burner_redis/__init__.py, python/burner_redis/pipeline.py, src/scripting.rs, src/store.rs, tests/test_pubsub.py]
tech_stack:
  added: []
  patterns: [monkey-patch-factory, broadcast-filter, async-generator, worker-thread]
key_files:
  created: [python/burner_redis/pubsub.py, tests/test_pubsub.py]
  modified: [python/burner_redis/__init__.py, python/burner_redis/pipeline.py, src/scripting.rs, src/store.rs]
decisions:
  - "PubSub class uses _filter_message to locally filter broadcast messages by subscriber's channel/pattern sets"
  - "PubSubWorkerThread uses asyncio.new_event_loop() for Python 3.9+ compatibility"
  - "Lua PUBLISH dispatches through cloned broadcast::Sender to avoid deadlock with data write lock"
  - "Used inspect.iscoroutinefunction instead of deprecated asyncio.iscoroutinefunction for Python 3.16 forward compatibility"
metrics:
  duration: 7min
  completed: 2026-04-14
  tasks: 2
  files: 6
---

# Phase 10 Plan 02: Python PubSub API, Pipeline/Lua Integration Summary

Python PubSub class with full redis-py async interface, PUBLISH in Pipeline and Lua scripts, 26-test comprehensive suite covering all CONTEXT.md decisions.

## Task Results

| Task | Name | Commit | Status |
|------|------|--------|--------|
| 1 | Python PubSub class, monkey-patch, Pipeline.publish, Lua PUBLISH dispatch | 150794c | Done |
| 2 | Comprehensive pub/sub test suite | 7198ba4 | Done |

## What Was Built

### Task 1: Python PubSub class, monkey-patch, Pipeline.publish, Lua PUBLISH dispatch

- **python/burner_redis/pubsub.py**: Full PubSub class with subscribe, unsubscribe, psubscribe, punsubscribe, listen (async generator), get_message (polling with timeout), handle_message (handler dispatch), run_in_thread (background daemon thread), close/aclose/reset. PubSubWorkerThread class for background message processing with clean stop() shutdown.
- **python/burner_redis/__init__.py**: Added `from burner_redis.pubsub import PubSub`, monkey-patched `pubsub()` factory onto BurnerRedis, added PubSub to `__all__`.
- **python/burner_redis/pipeline.py**: Added `publish(channel, message)` method to Pipeline for batched PUBLISH execution.
- **src/scripting.rs**: Updated `dispatch_command` and `LuaEngine::execute` signatures to accept `Option<&broadcast::Sender<PubSubMessage>>`. Added PUBLISH match arm that sends through broadcast channel and returns receiver count.
- **src/store.rs**: Updated `eval()` and `evalsha()` to clone pubsub broadcast sender BEFORE acquiring data write lock (deadlock prevention), then pass it to LuaEngine::execute.

### Task 2: Comprehensive pub/sub test suite

- **tests/test_pubsub.py**: 26 test functions covering all 13 CONTEXT.md decisions:
  - D-02: subscribe/publish, unsubscribe, psubscribe, punsubscribe, PUBSUB CHANNELS/NUMSUB/NUMPAT
  - D-04: PubSub factory, close/aclose cleanup, subscribed property
  - D-05: Sync and async handler callbacks
  - D-08: ignore_subscribe_messages, subscribe confirmation messages
  - D-09: Message format (type/pattern/channel/data keys), listen generator, get_message polling/timeout
  - D-10: Multiple subscribers both receive messages
  - D-11: Lua PUBLISH via redis.call
  - D-12: Pipeline.publish batch execution
  - Additional: run_in_thread lifecycle, unsubscribe-all, punsubscribe-all

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed asyncio.iscoroutinefunction deprecation**
- **Found during:** Task 2 (test run showed DeprecationWarning)
- **Issue:** `asyncio.iscoroutinefunction` is deprecated in Python 3.14+ and slated for removal in 3.16
- **Fix:** Changed to `inspect.iscoroutinefunction` with `import inspect`
- **Files modified:** python/burner_redis/pubsub.py
- **Commit:** 7198ba4 (included in Task 2 commit)

## Verification

- `cargo check` passes with no errors
- `uv run maturin develop` builds successfully
- `uv run pytest tests/test_pubsub.py -x` -- all 26 tests pass
- `uv run pytest tests/ -x` -- all 276 tests pass (full suite, no regressions)
- PubSub class supports all redis-py methods: subscribe, unsubscribe, psubscribe, punsubscribe, listen, get_message, handle_message, run_in_thread, close, aclose, reset
- PUBLISH works from direct call, pipeline, and Lua script

## Self-Check: PASSED

All created files exist, all modified files exist, all commit hashes verified, SUMMARY.md present.
