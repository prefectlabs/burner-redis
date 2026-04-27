---
phase: 11-close-redis-py-compatibility-gaps-for-pydocket-integration
reviewed: 2026-04-14T19:01:34Z
depth: standard
files_reviewed: 7
files_reviewed_list:
  - Cargo.toml
  - python/burner_redis/pipeline.py
  - src/lib.rs
  - src/scripting.rs
  - src/store.rs
  - tests/test_pydocket_compat.py
  - tests/test_streams.py
findings:
  critical: 0
  warning: 5
  info: 4
  total: 9
status: issues_found
---

# Phase 11: Code Review Report

**Reviewed:** 2026-04-14T19:01:34Z
**Depth:** standard
**Files Reviewed:** 7
**Status:** issues_found

## Summary

Phase 11 adds XCLAIM command support, XTRIM approximate parameter acceptance, blocking XREADGROUP with Lua XADD wake-through, and the full Lua dispatch for XCLAIM. The changes span the Rust core (store, scripting, lib), the Python pipeline shim, and integration/unit tests. The implementation is largely correct and well-structured, but there are several issues worth addressing around exclusive score bound handling in Lua dispatch, a potential silent data loss path in XAUTOCLAIM when entries are deleted from PEL but also added back, a race in the blocking XREADGROUP single-notify pattern, and incorrect PUBLISH subscriber counting in Lua scripts.

## Warnings

### WR-01: Blocking XREADGROUP only wakes on first notification, may miss data

**File:** `src/lib.rs:1189-1201`
**Issue:** The blocking XREADGROUP implementation uses `tokio::select!` with a single `notify.notified()` and a single retry. If the notification fires but the retry returns empty (e.g., a different stream was the source of the XADD, or another consumer grabbed the entry first), the consumer returns empty instead of retrying until timeout. In real Redis, `XREADGROUP ... BLOCK` continues polling until timeout or data is available. This pattern can cause pydocket workers to miss stream entries under concurrent-consumer scenarios.
**Fix:** Use a loop that re-waits on `notified()` until either entries are found or the timeout expires:
```rust
let deadline = tokio::time::Instant::now() + timeout_duration;
loop {
    let remaining = deadline - tokio::time::Instant::now();
    if remaining.is_zero() {
        return format_xreadgroup_result(Vec::new());
    }
    tokio::select! {
        _ = notify.notified() => {
            let results = store
                .xreadgroup(&group, &consumer, &keys, &id_strs, count)
                .map_err(store_err_to_py)?;
            if !results.is_empty() {
                return format_xreadgroup_result(results);
            }
            // No data yet, loop and wait again
        }
        _ = tokio::time::sleep(remaining) => {
            return format_xreadgroup_result(Vec::new());
        }
    }
}
```

### WR-02: Exclusive score bound prefix "(" silently treated as inclusive in Lua dispatch

**File:** `src/scripting.rs:1711-1728`
**Issue:** The `parse_score_arg` function correctly identifies the `(` prefix for exclusive bounds but then parses the numeric value and returns it as a plain `f64`. The calling code in `ZRANGEBYSCORE` and `ZREMRANGEBYSCORE` Lua commands uses this value with `Bound::Included`, meaning `(5` is treated identically to `5`. Redis ZRANGEBYSCORE with `(5` should exclude the value 5. A comment acknowledges this ("for simplicity we treat as inclusive"), but this divergence from Redis can cause subtle data bugs when Lua scripts use exclusive bounds (e.g., Docket scheduler scripts that move entries from sorted sets).
**Fix:** Return a tagged enum or tuple from `parse_score_arg` indicating exclusive vs inclusive, and use `Bound::Excluded` in range queries when the `(` prefix is present:
```rust
enum ScoreBound {
    Inclusive(f64),
    Exclusive(f64),
}
```
Then convert to the appropriate `std::ops::Bound` variant at the call sites.

### WR-03: XAUTOCLAIM adds deleted entries to claiming consumer's PEL

**File:** `src/store.rs:1714-1741`
**Issue:** In the XAUTOCLAIM loop, when an entry is found in the PEL but no longer exists in the stream (trimmed), the code correctly adds the ID to `deleted_ids` but then *also* inserts the entry into the claiming consumer's PEL (lines 1728-1740 run unconditionally). This means stale PEL entries accumulate for the new consumer -- they will never be acknowledged (the stream data is gone) and will show up in subsequent XPENDING/XAUTOCLAIM calls as phantom entries, causing repeated processing attempts that always fail.
**Fix:** Only insert into the claiming consumer's PEL when the entry still exists in the stream:
```rust
if let Some(fields) = stream.entries.get(entry_id) {
    claimed_entries.push((*entry_id, fields.clone()));
    // Only add to PEL if entry exists
    let claiming_consumer = cg
        .consumers
        .entry(consumer.clone())
        .or_insert_with(|| Consumer { pending: HashMap::new() });
    claiming_consumer.pending.insert(
        *entry_id,
        PendingEntry {
            delivery_time: Instant::now(),
            delivery_count: old_delivery_count + 1,
        },
    );
} else {
    deleted_ids.push(*entry_id);
}
```

Note: Redis 7+ actually does add deleted entries to the new consumer's PEL to maintain consistency, but it also returns them in `deleted_ids` so the caller can XACK them. The current behavior matches Redis here. However, pydocket may not handle this correctly, so it is worth verifying that pydocket's XAUTOCLAIM handler always XACKs the deleted IDs. If it does, this is not a bug but an integration risk.

### WR-04: Lua PUBLISH counts broadcast receivers rather than matching subscribers

**File:** `src/scripting.rs:1689-1704`
**Issue:** The PUBLISH command in Lua dispatch uses `tx.receiver_count()` to determine the return value, which counts all active broadcast receivers (including those subscribed to *different* channels or patterns). The Store's `publish()` method (in `src/store.rs:2301-2336`) correctly counts only matching channel + pattern subscribers. This means Lua scripts that use `redis.call('PUBLISH', ...)` will get inflated subscriber counts compared to calls via the Python API.
**Fix:** The Lua dispatch PUBLISH should mirror the Store's `publish()` logic, counting only subscribers to the matching channel and patterns, or call through to the Store's publish method:
```rust
"PUBLISH" => {
    // Count matching subscribers (channel + pattern) like Store::publish
    if args.len() != 2 {
        return Ok(RedisValue::Error(
            "ERR wrong number of arguments for 'publish' command".to_string(),
        ));
    }
    let channel = &args[0];
    let message = &args[1];
    match pubsub_tx {
        Some(tx) => {
            // Send "message" type
            let _ = tx.send(PubSubMessage {
                kind: "message".to_string(),
                pattern: None,
                channel: channel.clone(),
                data: message.clone(),
            });
            // Return 0 since we can't count matching subscribers without the registry
            // TODO: pass PubSubRegistry read access into Lua dispatch for accurate counting
            Ok(RedisValue::Integer(0))
        }
        None => Ok(RedisValue::Integer(0)),
    }
}
```
Alternatively, pass the subscriber count from the pubsub registry into the Lua dispatch context.

### WR-05: Pipeline xadd does not forward maxlen/minid parameters

**File:** `python/burner_redis/pipeline.py:120-121`
**Issue:** The Pipeline's `xadd` method accepts `maxlen` and `minid` parameters in its signature but does not include them in the queued command kwargs. This means `pipeline.xadd(name, fields, maxlen=100)` silently ignores the trimming parameters.
**Fix:**
```python
def xadd(self, name, fields, id="*", maxlen=None, minid=None):
    self._commands.append(("xadd", (name, fields), {"id": id, "maxlen": maxlen, "minid": minid}))
    return self
```
Note: This also requires the underlying `BurnerRedis.xadd()` Rust method to accept these parameters, which it currently does not. This is a known gap, but the Pipeline should still forward them so behavior is correct once the underlying method is extended.

## Info

### IN-01: Global mutable state in test module

**File:** `tests/test_pydocket_compat.py:84-89`
**Issue:** `_call_log` is a module-level global list used to track function calls across tests. While `_reset_call_log()` is called at the start of each test, if a test fails before calling `_reset_call_log()`, stale data could bleed between tests. Using a per-test fixture would be safer.
**Fix:** Convert `_call_log` to a pytest fixture:
```python
@pytest.fixture(autouse=True)
def call_log():
    log = []
    # Inject into module scope for task functions
    global _call_log
    _call_log = log
    yield log
```

### IN-02: Unused `_time` parameter in XCLAIM

**File:** `src/store.rs:1760`
**Issue:** The `_time` parameter in `Store::xclaim()` is accepted but completely unused (prefixed with underscore). Redis's XCLAIM TIME option sets the idle time to a specific UNIX timestamp in milliseconds, which is different from the IDLE option. This is an unimplemented parameter that could confuse callers who expect it to work.
**Fix:** Add a TODO comment or implement the TIME parameter. At minimum, document that it is not currently supported.

### IN-03: Commented-out code pattern: `#[allow(unused_variables)]` annotations

**File:** `src/lib.rs:846-847, 952-953, 1153`
**Issue:** Several `#[allow(unused_variables)]` annotations appear on parameters that are intentionally accepted but ignored (e.g., `block` in XREAD, `approximate` in XTRIM, `noack` in XREADGROUP). While not bugs, these indicate unimplemented Redis features that could diverge from expected behavior.
**Fix:** Consider adding brief doc comments or TODO markers explaining what behavior is intentionally not implemented (e.g., `// XREAD block is a no-op because...`, `// noack is not implemented because...`).

### IN-04: mlua version mismatch with CLAUDE.md recommendation

**File:** `Cargo.toml:18`
**Issue:** CLAUDE.md recommends `mlua 0.11.x` but `Cargo.toml` specifies `mlua 0.10`. This is a minor version discrepancy. The code works with 0.10 but may miss API improvements or bug fixes in 0.11.
**Fix:** Evaluate upgrading to mlua 0.11 when convenient. This is low priority as 0.10 appears to work correctly.

---

_Reviewed: 2026-04-14T19:01:34Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
