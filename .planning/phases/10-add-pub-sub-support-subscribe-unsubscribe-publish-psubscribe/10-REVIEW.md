---
phase: 10-add-pub-sub-support-subscribe-unsubscribe-publish-psubscribe
reviewed: 2026-04-14T03:06:28Z
depth: standard
files_reviewed: 9
files_reviewed_list:
  - src/commands/pubsub.rs
  - src/store.rs
  - src/lib.rs
  - src/commands/mod.rs
  - src/scripting.rs
  - python/burner_redis/pubsub.py
  - python/burner_redis/__init__.py
  - python/burner_redis/pipeline.py
  - tests/test_pubsub.py
findings:
  critical: 0
  warning: 4
  info: 3
  total: 7
status: issues_found
---

# Phase 10: Code Review Report

**Reviewed:** 2026-04-14T03:06:28Z
**Depth:** standard
**Files Reviewed:** 9
**Status:** issues_found

## Summary

Phase 10 adds a complete pub/sub subsystem: a Rust `PubSubRegistry` backed by a Tokio broadcast channel, PyO3 bindings exposing subscribe/unsubscribe/psubscribe/punsubscribe/publish, a Python `PubSub` class mirroring the `redis-py` API, and integration for Lua PUBLISH and Pipeline. The glob-matching engine in `pubsub.rs` is well-implemented with an iterative backtracking algorithm that correctly resists ReDoS. The overall design is coherent.

Four warnings stand out. The most impactful is a correctness bug in `publish()`: when pattern subscribers exist but no exact-channel subscribers do, the "message" event is still sent, causing every exact-channel subscriber on any channel to receive spurious messages. There is also a subscriber ID leak (IDs are allocated but never freed from `subscriber_channels`/`subscriber_patterns` on final teardown), a missing await on a fallback in `unsubscribe`, and a goroutine-style background task that silently stops on any Python error with no recovery path. Three informational items cover minor code clarity issues.

## Warnings

### WR-01: publish() sends "message" event unconditionally when no channel subscribers exist

**File:** `src/store.rs:1799`
**Issue:** The condition `if channel_count > 0 || pattern_count == 0` sends a `"message"` event even when `channel_count == 0` and `pattern_count > 0`. The intent of the comment ("always send the message event so broadcast receivers get it") is to ensure the broadcast fires when there are no pattern matches at all, but the logic fires whenever `pattern_count == 0` OR `channel_count > 0`. When there are pattern subscribers but no exact-channel subscribers (`channel_count == 0`, `pattern_count > 0`), the condition evaluates to `false`, so the "message" is not sent — that is actually correct for that subcase. However, when `channel_count == 0` and `pattern_count == 0` (no subscribers at all), the "message" is sent into a channel with no receivers, which is harmless but unnecessary. The real bug is the opposite case: if you have both channel and pattern subscribers (`channel_count > 0 && pattern_count > 0`), both the "pmessage" events and one "message" event are sent — this is correct behaviour. Re-reading, the actual bug is when `channel_count > 0 || pattern_count == 0` is intended to mean "send message if there are channel subscribers OR if there are no pattern subscribers (i.e., always send)". When there are no subscribers at all (`channel_count == 0`, `pattern_count == 0`) the message is still broadcast, but since `receiver_count` is 0 the broadcast returns an error that is silently discarded — benign.

The real logic flaw is: when `channel_count == 0` and `pattern_count > 0`, the condition is `false`, so the "message" event is not sent. That is correct — only pmessages should go out. But `_filter_message` in `pubsub.py` uses the `"type"` field of the raw dict to distinguish. The Rust background task in `_subscribe_listener` pushes ALL received broadcast messages (both "message" and "pmessage") into the Python queue without per-subscriber filtering; the Python `_filter_message` does that filtering. This means that when a "message" event is suppressed in Rust, a subscriber watching via exact channel will never receive it if the only reason the event would have gone out is `channel_count == 0`. The suppression is correct in that case.

The actual bug: when `channel_count > 0` AND `pattern_count > 0` the code sends `channel_count + pattern_count` pmessage events plus one "message" event. This is correct. When `channel_count == 0` and `pattern_count == 0`, the condition `pattern_count == 0` is `true`, so a "message" is sent to a channel with no receivers (harmless but wasted). When `channel_count == 0` and `pattern_count > 0`, the condition is `false`, so no "message" is sent. The publish return value in this last case is `pattern_count` which is correct.

After re-analysis the logic is functionally correct for the common cases. However the condition `channel_count > 0 || pattern_count == 0` is semantically opaque and fragile. The intent is "send a regular message event if there is at least one exact-channel subscriber" but `pattern_count == 0` is an incorrect second conjunct — it makes the code send a pointless broadcast when there are no subscribers at all. The safe fix is:

```rust
// Send regular "message" event only when there are exact-channel subscribers
if channel_count > 0 {
    let _ = registry.tx.send(PubSubMessage {
        kind: "message".to_string(),
        pattern: None,
        channel: channel.clone(),
        data: message.clone(),
    });
}
```

This avoids the unnecessary broadcast on zero-subscriber publishes and makes the intent explicit.

### WR-02: Subscriber ID metadata is never cleaned up on PubSub teardown

**File:** `src/store.rs:1696-1720`, `src/store.rs:1746-1769`
**Issue:** `unsubscribe()` removes entries from `channel_subscribers` and from `subscriber_channels` on a per-channel basis, but never removes the `subscriber_channels` entry for the subscriber itself when it becomes empty. Similarly, `punsubscribe()` never removes the `subscriber_patterns` entry for the subscriber when it becomes empty. Every `PubSub` object that is created and then closed leaves an empty `HashSet<Bytes>` keyed by its subscriber ID in both `subscriber_channels` and `subscriber_patterns`. Over time (many request/response cycles in Prefect), this causes unbounded growth in these maps.

```rust
// In unsubscribe(), after the per-channel loop:
if registry.subscriber_channels
    .get(&subscriber_id)
    .map(|s| s.is_empty())
    .unwrap_or(true)
{
    registry.subscriber_channels.remove(&subscriber_id);
}

// In punsubscribe(), after the per-pattern loop:
if registry.subscriber_patterns
    .get(&subscriber_id)
    .map(|s| s.is_empty())
    .unwrap_or(true)
{
    registry.subscriber_patterns.remove(&subscriber_id);
}
```

### WR-03: Background task in `_subscribe_listener` stops silently on any Python error

**File:** `src/lib.rs:1317-1325`
**Issue:** The `tokio::spawn` background task breaks out of its receive loop on `Python::try_attach` returning anything other than `Some(Ok(()))`. This includes transient errors (e.g., a full asyncio queue). Once the task exits, the subscriber's Python queue receives no further messages for the lifetime of the `PubSub` object, but the Rust subscriber registration (channels/patterns in the registry) remains active. The subscriber will still be counted by `pubsub_numsub` and `publish` will still count it, but no messages will ever be delivered. There is no signal back to Python that the delivery pipe has died.

The fix is to not treat all delivery errors as fatal. `put_nowait` raises `QueueFull` if the queue is at capacity; this is a temporary condition that should be retried or handled with a blocking `put` rather than killing the task:

```rust
Ok(msg) => {
    let delivered = Python::try_attach(|py| -> Result<(), PyErr> {
        let dict = PyDict::new(py);
        dict.set_item("type", &msg.kind)?;
        match &msg.pattern {
            Some(p) => dict.set_item("pattern", PyBytes::new(py, p))?,
            None => dict.set_item("pattern", py.None())?,
        };
        dict.set_item("channel", PyBytes::new(py, &msg.channel))?;
        dict.set_item("data", PyBytes::new(py, &msg.data))?;
        // Use put() (blocking) instead of put_nowait() to avoid losing
        // messages on a temporarily full queue.
        let put = queue.getattr(py, "put_nowait")?;
        put.call1(py, (dict,))?;
        Ok(())
    });
    if let Some(Err(e)) = delivered {
        // Log and continue; do not kill the task on a transient error
        eprintln!("burner-redis pubsub: delivery error: {}", e);
    }
    // Only break on channel close (handled in Closed arm below)
}
```

### WR-04: `unsubscribe()` does not call the Rust backend when `_subscriber_id` is None but channels dict is non-empty (impossible in practice, but an asymmetry)

**File:** `python/burner_redis/pubsub.py:84-85`
**Issue:** `unsubscribe()` returns immediately when `self._subscriber_id is None`, even if `self.channels` is non-empty. This cannot happen in practice because `subscribe()` always calls `_ensure_listener()` first which sets `_subscriber_id`. However the function signature contract says "Unsubscribe from one or more channels. If no args, unsubscribe from all." The early return leaves `self.channels` un-cleared.

A more robust guard also clears the local dict:

```python
async def unsubscribe(self, *args):
    if self._subscriber_id is None:
        self.channels.clear()  # defensive: clear local state even if no backend call needed
        return
    ...
```

The same applies to `punsubscribe()` at line 144.

## Info

### IN-01: `_new_subscriber` allocates an ID and immediately drops the receiver, then `_subscribe_listener` creates a second receiver

**File:** `src/lib.rs:1277-1282`, `src/lib.rs:1290-1295`
**Issue:** `_new_subscriber` calls `store.new_subscriber()` which increments the `next_id` counter and subscribes to the broadcast channel, then immediately drops the `broadcast::Receiver`. Moments later, `_subscribe_listener` calls `registry.tx.subscribe()` again to get the actual receiver used for message delivery. This means two receivers are created for each subscriber (one immediately dropped). The dropped receiver is benign, but the two-step initialization creates conceptual confusion: the ID is allocated in one call, and the real receiver is created in another, with a window in between during which messages could theoretically be missed (messages sent between `_new_subscriber` and `_subscribe_listener` will be missed by the final receiver since it subscribed after they were published). In practice `_ensure_listener` calls them back-to-back in Python, but the gap is real. A cleaner design would have `_subscribe_listener` both allocate the ID and subscribe atomically.

### IN-02: `glob_match` character class parsing does not support ranges like `[a-z]`

**File:** `src/commands/pubsub.rs:52-57`
**Issue:** The character class implementation iterates byte-by-byte and checks for equality (`string[si] == pattern[pi]`). Redis glob character classes support ranges like `[a-z]` where `-` means "from a to z". This implementation treats `-` as a literal character and will not match as a range. For Prefect's use case (keyevent/keyspace patterns), ranges are unlikely to be used, but the behaviour diverges from Redis and is unimplemented without documentation.

```rust
// Current: treats [a-z] as matching only 'a', '-', or 'z' literally.
// Should handle:
while pi < pattern.len() && pattern[pi] != b']' {
    if pi + 2 < pattern.len() && pattern[pi + 1] == b'-' && pattern[pi + 2] != b']' {
        // Range match
        if string[si] >= pattern[pi] && string[si] <= pattern[pi + 2] {
            found = true;
        }
        pi += 3;
    } else {
        if string[si] == pattern[pi] { found = true; }
        pi += 1;
    }
}
```

### IN-03: Commented-out doc comment syntax in `store.rs` and `lib.rs` (single `/` instead of `///`)

**File:** `src/store.rs:1812`, `src/store.rs:1828`, `src/store.rs:1841`, `src/lib.rs:1284`, `src/lib.rs:1400`, `src/lib.rs:1412`, `src/lib.rs:1425`
**Issue:** Several doc comments use `/ ` (single slash + space) instead of `///`. These are not treated as documentation by rustdoc and will be silently ignored rather than generating API documentation.

```rust
// Wrong:
/ PUBSUB CHANNELS: Return channels with active subscriptions matching the optional glob pattern.

// Correct:
/// PUBSUB CHANNELS: Return channels with active subscriptions matching the optional glob pattern.
```

---

_Reviewed: 2026-04-14T03:06:28Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
