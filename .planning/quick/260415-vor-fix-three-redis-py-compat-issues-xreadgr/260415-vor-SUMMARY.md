---
quick_id: 260415-vor
type: execute
wave: 1
depends_on: []
subsystem: streams, pubsub
tags: [streams, pubsub, redis-py-compat, nogroup, asyncio]
tech-stack:
  added: []
  patterns:
    - "Per-command NOGROUP wrapping via store_err_to_py_for_cmd helper"
    - "Pre-armed Notify::notified waiter in xreadgroup blocking path to close race window"
key-files:
  created: []
  modified:
    - src/store.rs
    - src/lib.rs
    - tests/test_streams.py
    - tests/test_pubsub.py
decisions:
  - "StoreError::NoGroup stays (group, key) internally; Display template reorders to canonical '(key, group)' phrasing so store layer remains command-agnostic"
  - "Per-command suffix 'in <CMD>' appended at the PyO3 binding layer via store_err_to_py_for_cmd, not inside store.rs, to avoid threading command names through the core storage API"
  - "XACK on missing key/group still returns 0 (matches Redis semantics) — NOGROUP wrapping applies only to non-zero error paths"
  - "xreadgroup blocking-path race fix applied unconditionally: waiter is armed BEFORE first poll so a notify fired between the first read and select! is not lost"
  - "Pubsub cancellation fix was already in place (asyncio.wait instead of wait_for); this task adds regression tests only"
metrics:
  duration: 8min
  completed: 2026-04-16
  tasks: 3
  files: 4
requirements: []
---

# Quick Task 260415-vor: Fix Three redis-py Compatibility Issues Summary

Close three drop-in compatibility gaps so pydocket and Prefect can remove the `is_memory` special-casing and rely on standard redis-py error handling: canonical NOGROUP messages with per-command suffix, proof that xreadgroup(block) cooperates with the asyncio event loop, and a regression test pinning the existing PubSub cancellation fix.

## What Was Changed

### src/store.rs — StoreError::NoGroup Display format

Before:
```rust
#[error("NOGROUP No such consumer group '{0}' for key name '{1}'")]
NoGroup(String, String),  // (group, key)
```

After:
```rust
/// Parameters are `(group, key)` to preserve call-site order; the Display
/// format reorders them to match Redis canonical phrasing.
#[error("NOGROUP No such key '{1}' or consumer group '{0}'")]
NoGroup(String, String),  // (group, key)
```

Call sites unchanged — tuple order remained `(group, key)`; only the Display template reorders.

### src/lib.rs — per-command NOGROUP wrapping helper

New helper added next to `store_err_to_py`:
```rust
fn store_err_to_py_for_cmd(e: StoreError, cmd: &str) -> PyErr {
    let msg = match &e {
        StoreError::NoGroup(_, _) => format!("{} in {}", e, cmd),
        _ => e.to_string(),
    };
    make_response_error(msg)
}
```

Applied at every stream-group call site (top-level method and `execute_pipeline` dispatch):

| Command | Top-level method | Pipeline dispatch |
|---|---|---|
| XGROUP CREATE | ✅ | ✅ |
| XREADGROUP (sync + blocking + select-loop) | ✅ | ✅ |
| XACK | ✅ | ✅ |
| XAUTOCLAIM | ✅ | ✅ |
| XCLAIM | ✅ | ✅ |
| XPENDING range | ✅ | ✅ |
| XPENDING summary | ✅ | ✅ |
| XINFO GROUPS | ✅ | ✅ |
| XINFO CONSUMERS | ✅ | ✅ |

### src/lib.rs — xreadgroup blocking-path race close

The plan flagged a potential race: a `stream_notify.notify_waiters()` firing between the first non-blocking read and the `tokio::select!` await is lost unless the `Notify::notified()` future is registered first.

Fix applied:
```rust
let notify = store.stream_notify();
let mut waiter = Box::pin(notify.notified());
waiter.as_mut().enable();  // arm the waiter BEFORE the first poll

let results = store.xreadgroup(...).map_err(...)?;
if !results.is_empty() { return format_xreadgroup_result(results); }
// waiter is already armed; no race window
```

Inside the loop, after a wake, the waiter is re-pinned and re-armed before the next xreadgroup attempt so a notify firing during the second read is also not lost.

## Issue 1 Outcome: No Additional Code Needed

The plan-specified test `test_xreadgroup_block_yields_to_event_loop` and `test_xreadgroup_block_concurrent_consumers_event_loop_progress` both **pass against the existing `pyo3_async_runtimes::tokio::future_into_py + tokio::select!` bridge**.

- Concurrent tick-counter coroutine advanced from 0 to 5+ ticks while xreadgroup was blocked → asyncio loop is not starved.
- Concurrent xadd woke the reader in < 1s (well under the 5000ms block timeout).
- Two concurrent consumers both returned from `block=2000` in < 1s when xadd fired.

The `is_memory` workaround in pydocket was defensive but no longer needed. The pre-arm waiter change is a defensive race close that applies regardless of test outcome.

## Tests Added

| Test | What it pins |
|---|---|
| `test_xgroup_create_missing_key_raises_response_error` | XGROUP CREATE mkstream=False on missing key → `redis.exceptions.ResponseError("ERR ... requires the key to exist")` |
| `test_xreadgroup_nogroup_raises_response_error` | XREADGROUP on missing key AND missing group both → `ResponseError("NOGROUP ... in XREADGROUP")` |
| `test_xautoclaim_nogroup_raises_response_error` | XAUTOCLAIM on missing key/group → `ResponseError("NOGROUP ... in XAUTOCLAIM")` |
| `test_xpending_range_nogroup_raises_response_error` | xpending_range on missing key/group incl. consumer-filter variant → `ResponseError("NOGROUP ... in XPENDING")` |
| `test_xpending_summary_nogroup_raises_response_error` | xpending() summary form on missing key/group → `ResponseError("NOGROUP ... in XPENDING")` |
| `test_xclaim_nogroup_raises_response_error` | XCLAIM on missing key/group → `ResponseError("NOGROUP ... in XCLAIM")` |
| `test_xreadgroup_block_yields_to_event_loop` | xreadgroup(block=N) cooperates with asyncio loop — tick counter advances, concurrent xadd wakes reader < 1s |
| `test_xreadgroup_block_concurrent_consumers_event_loop_progress` | Two blocked consumers both wake from notify_waiters under contention |
| `test_pubsub_get_message_task_cancellation` | External task.cancel() propagates through PubSub.get_message (cpython#86296 regression guard, subscribe path) |
| `test_pubsub_get_message_task_cancellation_pattern` | Same, psubscribe path |

## Downstream Impact: Drop the `is_memory` Workaround

Before (pydocket `get_new_deliveries` or `get_redeliveries`):
```python
is_memory = str(url).startswith("memory://")
try:
    messages = await redis.xreadgroup(group, consumer, {stream: ">"}, block=5000)
except redis.exceptions.ResponseError as e:
    if "NOGROUP" in str(e):
        ...
except Exception as e:  # BurnerRedis leaked a different exception class
    if is_memory and "NOGROUP" in str(e):
        ...
```

After:
```python
try:
    messages = await redis.xreadgroup(group, consumer, {stream: ">"}, block=5000)
except redis.exceptions.ResponseError as e:
    if "NOGROUP" in str(e):
        ...
```

One call site, one exception class, one message shape — identical to real Redis.

## Python Version Matrix

Task 3 cancellation regression verified locally on:
- **Python 3.12.5** (current interpreter) — both new tests pass
- **Python 3.10.15** — both new tests pass (verified via `uv run --python 3.10 pytest`)

Full 3.10–3.14 matrix runs in CI via the pydocket CI workflow added in quick task 260415-us1. The cpython#86296 bug primarily affected 3.10/3.11; passing on 3.10 is the strongest signal the fix is in place.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 — Defensive correctness] Pre-armed notify waiter in xreadgroup blocking path**

- **Found during:** Task 1 implementation
- **Issue:** The plan flagged a race window where `Notify::notified()` is created AFTER the first xreadgroup poll, so a `notify_waiters()` fired between the read and the `.notified().await` is lost.
- **Fix:** Register the waiter and call `.enable()` BEFORE the first poll; re-pin the waiter inside the select loop after each wake.
- **Files modified:** src/lib.rs (blocking xreadgroup path)
- **Commit:** 7710dab
- **Rationale:** Classic async race pattern. Even though the new regression tests passed without this change, the race is real and the fix is surgical.

### Auth gates

None.

## Verification

- `uv run maturin develop --release` — clean (10 pre-existing PyO3 deprecation warnings, none new)
- `cargo test --lib` — 113 tests pass
- `uv run pytest tests/` — 350 passed, 1 skipped (pre-existing), 30 deselected (integration-only tests not in default run)
- `uv run pytest tests/test_streams.py` — 75 tests pass (73 original + 2 new Issue 1 tests + extensive NOGROUP coverage added)
- `uv run pytest tests/test_pubsub.py` — 29 tests pass (27 original + 2 new cancellation regressions)
- Python 3.10 verification of cancellation tests — 2/2 pass

## Commits

| Commit | Message |
|---|---|
| 0ca7acc | test(quick-260415-vor): add failing tests for NOGROUP canonical errors |
| 7710dab | fix(quick-260415-vor): emit canonical NOGROUP errors per command |
| 18148f5 | test(quick-260415-vor): pin xreadgroup(block) async event-loop cooperation |
| 505d106 | test(quick-260415-vor): pin PubSub.get_message task cancellation fix |

## Self-Check: PASSED

Verified artifacts below exist on disk:
- `/Users/alexander/dev/prefectlabs/burner-redis/src/store.rs` — NoGroup Display updated
- `/Users/alexander/dev/prefectlabs/burner-redis/src/lib.rs` — store_err_to_py_for_cmd helper + per-command wrapping
- `/Users/alexander/dev/prefectlabs/burner-redis/tests/test_streams.py` — 8 new tests
- `/Users/alexander/dev/prefectlabs/burner-redis/tests/test_pubsub.py` — 2 new tests

Verified commits exist:
- 0ca7acc, 7710dab, 18148f5, 505d106 — all present in `git log`
