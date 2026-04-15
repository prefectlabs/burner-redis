# Quick Task 260415-mn4: Fix asyncio.wait_for cancellation bug in PubSub.get_message

**Status:** Complete
**Date:** 2026-04-15

## Changes

### Task 1: Replace asyncio.wait_for with asyncio.wait
- **File:** `python/burner_redis/pubsub.py`
- **Commit:** `c2a31aa`
- Replaced `asyncio.wait_for(self._queue.get(), timeout=timeout)` with `asyncio.wait({get_task}, timeout=timeout)` in the timeout branch of `get_message()`
- Avoids cpython#86296 where external `task.cancel()` can be lost on Python < 3.12 when racing with wait_for's internal timeout machinery

### Task 2: Add cancellation propagation test
- **File:** `tests/test_pubsub.py`
- **Commit:** `a8161ab`
- Added `test_get_message_cancellation_propagates` verifying external cancellation propagates correctly through `get_message(timeout=N)`
