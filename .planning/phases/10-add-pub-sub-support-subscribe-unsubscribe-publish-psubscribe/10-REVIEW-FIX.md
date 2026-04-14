---
phase: 10-add-pub-sub-support-subscribe-unsubscribe-publish-psubscribe
fixed_at: 2026-04-14T03:28:04Z
review_path: .planning/phases/10-add-pub-sub-support-subscribe-unsubscribe-publish-psubscribe/10-REVIEW.md
iteration: 1
findings_in_scope: 4
fixed: 4
skipped: 0
status: all_fixed
---

# Phase 10: Code Review Fix Report

**Fixed at:** 2026-04-14T03:28:04Z
**Source review:** .planning/phases/10-add-pub-sub-support-subscribe-unsubscribe-publish-psubscribe/10-REVIEW.md
**Iteration:** 1

**Summary:**
- Findings in scope: 4
- Fixed: 4
- Skipped: 0

## Fixed Issues

### WR-01: publish() sends "message" event unconditionally when no channel subscribers exist

**Files modified:** `src/store.rs`
**Commit:** cefe7d9
**Applied fix:** Changed condition from `if channel_count > 0 || pattern_count == 0` to `if channel_count > 0` so that the regular "message" event is only sent when there are exact-channel subscribers. This eliminates the unnecessary broadcast when there are no subscribers at all and makes the intent explicit.

### WR-02: Subscriber ID metadata is never cleaned up on PubSub teardown

**Files modified:** `src/store.rs`
**Commit:** 5c61bc9
**Applied fix:** Added cleanup logic after the per-channel loop in `unsubscribe()` and after the per-pattern loop in `punsubscribe()`. When the subscriber's channel set or pattern set becomes empty, the entry is removed from `subscriber_channels` / `subscriber_patterns` respectively. This prevents unbounded growth of these maps over time as PubSub objects are created and destroyed.

### WR-03: Background task in _subscribe_listener stops silently on any Python error

**Files modified:** `src/lib.rs`
**Commit:** e13bc74
**Applied fix:** Changed the `match delivered` block to distinguish between `Some(Err(e))` (Python error like QueueFull) and `None` (GIL not available). Both cases now log and continue instead of breaking out of the receive loop. The task only exits on broadcast channel close (`RecvError::Closed`). This prevents transient delivery errors from permanently killing the subscriber's message pipeline.

### WR-04: unsubscribe()/punsubscribe() don't clear local dict when subscriber_id is None

**Files modified:** `python/burner_redis/pubsub.py`
**Commit:** d443487
**Applied fix:** Added `self.channels.clear()` in the early return path of `unsubscribe()` and `self.patterns.clear()` in the early return path of `punsubscribe()` when `_subscriber_id is None`. This ensures local subscription state is always consistent even in the defensive edge case where no backend call is needed.

---

_Fixed: 2026-04-14T03:28:04Z_
_Fixer: Claude (gsd-code-fixer)_
_Iteration: 1_
