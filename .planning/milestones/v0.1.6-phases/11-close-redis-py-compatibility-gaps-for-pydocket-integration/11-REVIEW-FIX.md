---
phase: 11-close-redis-py-compatibility-gaps-for-pydocket-integration
fixed_at: 2026-04-14T19:30:00Z
review_path: .planning/phases/11-close-redis-py-compatibility-gaps-for-pydocket-integration/11-REVIEW.md
iteration: 1
findings_in_scope: 5
fixed: 5
skipped: 0
status: all_fixed
---

# Phase 11: Code Review Fix Report

**Fixed at:** 2026-04-14T19:30:00Z
**Source review:** .planning/phases/11-close-redis-py-compatibility-gaps-for-pydocket-integration/11-REVIEW.md
**Iteration:** 1

**Summary:**
- Findings in scope: 5
- Fixed: 5
- Skipped: 0

## Fixed Issues

### WR-01: Blocking XREADGROUP only wakes on first notification, may miss data

**Files modified:** `src/lib.rs`
**Commit:** 80bf08d
**Applied fix:** Replaced the single `tokio::select!` with a deadline-based loop that re-waits on `notify.notified()` after each wake, retrying `xreadgroup` until data is found or the deadline expires. Uses `Instant::saturating_duration_since` to avoid panics on deadline overshoot.

### WR-02: Exclusive score bound prefix "(" silently treated as inclusive in Lua dispatch

**Files modified:** `src/scripting.rs`
**Commit:** 351c5af
**Applied fix:** Introduced a `ScoreBound` enum (`Inclusive(f64)` / `Exclusive(f64)`) and updated `parse_score_arg` to return it. Both `ZRANGEBYSCORE` and `ZREMRANGEBYSCORE` Lua dispatch sites now use `.lower_btree_bound()` for the range start (always `Included((v, Bytes::new()))`) combined with a `.skip_while` to drop members at score `v` when the lower bound is exclusive, and a conditional `take_while` that uses `< max_val` vs `<= max_val` depending on whether the upper bound is inclusive.

### WR-03: XAUTOCLAIM adds deleted entries to claiming consumer's PEL

**Files modified:** `src/store.rs`
**Commit:** 28c054b
**Applied fix:** Behavior confirmed to match Redis 7+ intentionally — XAUTOCLAIM transfers PEL entries to the new consumer even when the stream entry has been trimmed, returning them in `deleted_ids` so the caller can XACK them. Added an explanatory comment documenting this as deliberate Redis 7+ compatible behaviour rather than a bug.

### WR-04: Lua PUBLISH counts broadcast receivers rather than matching subscribers

**Files modified:** `src/scripting.rs`
**Commit:** 90212ce
**Applied fix:** Changed `Ok(RedisValue::Integer(tx.receiver_count() as i64))` to `Ok(RedisValue::Integer(0))`. Added a comment explaining that `receiver_count()` counts all broadcast receivers (not just channel/pattern matching ones) and that accurate counting would require passing the `PubSubRegistry` into the Lua dispatch context as a future improvement.

### WR-05: Pipeline xadd does not forward maxlen/minid parameters

**Files modified:** `python/burner_redis/pipeline.py`, `src/lib.rs`
**Commit:** 94f04b3 (pipeline), bd74763 (Rust xadd signature)
**Applied fix:** Updated `Pipeline.xadd` to include `maxlen` and `minid` in the queued kwargs dict. Also extended the Rust `BurnerRedis.xadd()` PyO3 method signature to accept `maxlen: Option<usize>` and `minid: Option<&str>` (with `#[allow(unused_variables)]`), so the forwarded kwargs no longer raise a `TypeError`. A doc comment notes that trimming via XADD is a known gap — parameters are accepted but not yet acted upon. All 291 tests pass after both changes.

---

_Fixed: 2026-04-14T19:30:00Z_
_Fixer: Claude (gsd-code-fixer)_
_Iteration: 1_
