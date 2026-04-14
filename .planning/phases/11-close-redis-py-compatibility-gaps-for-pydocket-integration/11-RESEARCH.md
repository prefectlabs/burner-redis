# Phase 11: Close redis-py compatibility gaps for pydocket integration - Research

**Researched:** 2026-04-14
**Domain:** Redis command compatibility, stream consumer group semantics, async timing
**Confidence:** HIGH

## Summary

This phase requires closing the remaining compatibility gaps between burner-redis and pydocket (v0.19.0). The primary investigation reveals two distinct categories of work: (1) a timing race in the delayed task delivery path where `run_until_finished()` can exit before XREADGROUP delivers entries added by Lua XADD, and (2) missing Redis commands that pydocket uses but burner-redis does not implement.

The delayed task race is NOT a `last_delivered_id` tracking bug as initially hypothesized. Empirical testing (100 iterations) shows ~19% failure rate -- the race is between the worker's main loop `check_for_work()` and the scheduler loop's Lua script. When the scheduler atomically moves a task from the sorted set queue to the stream, there is a window where the main loop can observe the queue as empty but has not yet polled XREADGROUP to find the new stream entry. The fix must ensure XREADGROUP with `>` reliably sees entries added by Lua XADD within the same process, likely by making the `block` parameter functional (even a minimal implementation that retries once after a short delay rather than returning immediately).

The missing commands are: `xclaim` (used for lease renewal in the worker), and the `xtrim` `approximate` parameter (used by `docket.clear()`). Running pydocket's own test suite (72 test files at github.com/chrisguidry/docket) will reveal the complete gap inventory.

**Primary recommendation:** Fix the XREADGROUP timing race first (highest impact single fix), then implement missing commands discovered by running pydocket's test suite, then add regression tests to burner-redis for every gap fixed.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- **D-01:** Scope is pydocket-only -- only fix what pydocket's test suite and usage patterns require
- **D-02:** Implement everything pydocket needs -- no partial passes, no deferring edge cases
- **D-03:** Each new command must be full redis-py compatible (all flags, edge cases, return types), not minimal stubs
- **D-04:** Run pydocket's own test suite against BurnerRedis as the primary source of truth for gaps
- **D-05:** Inventory all gaps from pydocket's test suite first, before fixing anything -- avoids rework
- **D-06:** After inventory, fix everything including the XREADGROUP race in priority order
- **D-07:** Fix the root cause at the Store level -- `last_delivered_id` must advance correctly when XADD is called from Lua scripts so XREADGROUP `>` always sees new entries
- **D-08:** No Python-layer workarounds -- the semantics must be correct in Rust
- **D-09:** Phase is done when: (1) pydocket's full test suite passes against BurnerRedis with zero xfails/skips, AND (2) our own regression test suite covers every gap that was fixed
- **D-10:** Use pydocket's test suite to discover gaps, then add key scenarios to our integration tests as regression coverage

### Claude's Discretion
- Implementation order of individual missing commands (after inventory)
- How to run pydocket's test suite (vendored, subprocess, conftest fixture, etc.)
- Rust-side architecture for new commands (follow existing patterns)

### Deferred Ideas (OUT OF SCOPE)
None -- discussion stayed within phase scope
</user_constraints>

## Project Constraints (from CLAUDE.md)

- **Language**: Rust core with Python bindings via PyO3/maturin
- **Pattern**: Store methods return `Result<RedisValue, StoreError>` -- new commands follow same pattern
- **Layers**: Every new command needs: (1) Store method, (2) PyO3 async binding in lib.rs, (3) Pipeline buffer method in pipeline.py, (4) Lua dispatch entry in scripting.rs (if used from Lua)
- **Testing**: pytest with asyncio_mode="auto", integration tests use marker `@pytest.mark.integration`
- **Build**: `maturin develop` for dev builds
- **Architecture**: parking_lot RwLock for data, single-writer atomicity for Lua scripts

## XREADGROUP Timing Race Analysis

### Root Cause (Revised from CONTEXT.md)

The CONTEXT.md hypothesis states: "`last_delivered_id` not advancing when XADD is called through dispatch_command." This is **incorrect as stated**. [VERIFIED: codebase analysis of src/store.rs and src/scripting.rs]

The actual mechanism:

1. **Data correctness is fine.** Lua XADD (via `dispatch_command` in scripting.rs:1143-1218) correctly inserts entries into `stream.entries` and updates `stream.last_id`. XREADGROUP (store.rs:1488-1522) correctly queries `stream.entries.range(Excluded(last_delivered_id), Unbounded)`. Both operate on the same `HashMap<Bytes, ValueEntry>` through the parking_lot RwLock. After Lua XADD completes, the entry IS visible to subsequent XREADGROUP calls.

2. **The race is async scheduling.** The worker runs a main loop and a scheduler loop as concurrent asyncio tasks. Because burner-redis ignores the `block` parameter on XREADGROUP (lib.rs:1095 -- accepted for API compatibility, returns immediately), the main loop busy-polls. The failure scenario:
   - Scheduler Lua atomically moves task from queue to stream (ZREMRANGEBYSCORE + XADD)
   - Main loop's XREADGROUP returns empty (ran before XADD) 
   - Main loop's `check_for_work()` sees xlen=0 (entry exists but hasn't been observed), zcard=0 (just removed)
   - `has_work=False`, `active_tasks` empty -- `run_until_finished()` exits
   
3. **Empirical evidence:** Running the delayed task test 100 times shows ~81% pass rate, ~19% failure rate -- a classic timing race. [VERIFIED: local testing]

### Fix Strategy

**Option A (Recommended): Implement minimal `block` support for XREADGROUP.** When `block` is specified and no entries are found, instead of returning immediately, wait for the specified duration (or until new data arrives) before returning empty. This matches Redis semantics and eliminates the race.

**Implementation approach:** Add a `tokio::sync::Notify` or condvar to the Store that is signaled whenever XADD inserts an entry. XREADGROUP with block > 0 can wait on this signal with a timeout. This is the correct architectural fix because:
- It fixes the race for ALL consumers, not just pydocket
- It matches real Redis behavior
- It only requires changes in Rust (D-08)
- The notification mechanism already exists partially (pubsub broadcast sender)

**Option B: Use a dedicated condvar/channel per stream.** More precise but more complex.

**Option C: Simple retry loop.** If XREADGROUP with `>` returns empty and `block > 0`, sleep briefly and retry once. Less correct but simpler.

**Note on D-07:** The locked decision says "fix the root cause at the Store level -- `last_delivered_id` must advance correctly when XADD is called from Lua scripts." The research shows `last_delivered_id` is NOT the issue (it should NOT advance on XADD -- only on XREADGROUP delivery, per Redis semantics). The fix IS at the Store level but addresses the `block` parameter instead. The planner should note this correction.

## Gap Inventory: Commands Pydocket Uses

### Already Implemented (Verified Working) [VERIFIED: codebase grep + test results]

| Command | Store | PyO3 | Pipeline | Lua Dispatch | Used By |
|---------|-------|------|----------|-------------|---------|
| SET | yes | yes | yes | yes | Lua scripts |
| GET | yes | yes | yes | yes | Lua scripts |
| DEL/DELETE | yes | yes | yes | yes | Lua scripts, direct |
| EXISTS | yes | yes | yes | yes | Lua scripts |
| HSET | yes | yes | yes | yes | Lua scripts, progress, state |
| HGET | yes | yes | yes | yes | Lua scripts, state |
| HDEL | yes | yes | yes | yes | Lua scripts |
| HGETALL | yes | yes | yes | yes | Snapshot, scheduler Lua |
| HEXISTS | yes | yes | yes | yes | Schedule Lua |
| HINCRBY | yes | yes | yes | yes | Generation counter, progress |
| HVALS | yes | yes | yes | -- | Not used by pydocket |
| SADD | yes | yes | yes | -- | Worker heartbeat |
| SMEMBERS | yes | yes | yes | -- | Worker info |
| SREM | yes | yes | yes | -- | Not used by pydocket |
| SISMEMBER | yes | yes | yes | -- | Not used by pydocket |
| ZADD | yes | yes | yes | yes | Queue, heartbeat |
| ZREM | yes | yes | yes | yes | Cancel Lua |
| ZRANGE (withscores) | yes | yes | yes | yes | Snapshot, workers |
| ZRANGEBYSCORE | yes | yes | yes | yes | Scheduler Lua |
| ZREMRANGEBYSCORE | yes | yes | yes | yes | Heartbeat, scheduler |
| ZCARD | yes | yes | yes | yes | check_for_work, scheduler |
| ZSCORE | yes | yes | yes | yes | Not directly by pydocket |
| ZCOUNT | yes | yes | yes | -- | Heartbeat metrics |
| EXPIRE | yes | yes | yes | yes | State TTL, heartbeat |
| XADD | yes | yes | yes | yes | Schedule, scheduler Lua |
| XREAD | yes | yes | yes | yes | Strike monitor |
| XLEN | yes | yes | yes | -- | check_for_work, heartbeat |
| XDEL | yes | yes | yes | yes | ACK+DEL, cancel Lua |
| XRANGE | yes | yes | yes | -- | Snapshot, clear |
| XTRIM | yes | yes | yes | -- | Clear (but see gap) |
| XGROUP CREATE | yes | yes | yes | -- | Worker init |
| XGROUP DESTROY | yes | yes | yes | -- | Not used by pydocket |
| XREADGROUP | yes | yes | yes | -- | Worker main loop |
| XACK | yes | yes | yes | yes | ACK messages, reschedule Lua |
| XAUTOCLAIM | yes | yes | yes | -- | Redelivery |
| XPENDING RANGE | yes | yes | yes | -- | Snapshot |
| XINFO GROUPS | yes | yes | yes | -- | Not used by pydocket |
| XINFO CONSUMERS | yes | yes | yes | -- | Not used by pydocket |
| PUBLISH | yes | yes | yes | yes | State events, cancel |
| SUBSCRIBE | yes | yes | -- | -- | State/progress updates |
| PSUBSCRIBE | yes | yes | -- | -- | Cancellation listener |
| EVAL/EVALSHA | yes | yes | yes | -- | All Lua scripts |
| LOCK | yes (Python) | -- | -- | -- | Schedule, perpetual |
| PIPELINE | yes (Python) | -- | -- | -- | Many operations |
| REGISTER_SCRIPT | yes (Python) | -- | -- | -- | All Lua scripts |

### Missing Commands [VERIFIED: codebase grep shows no implementation]

| Command | Where Used | Priority | Complexity |
|---------|-----------|----------|------------|
| **XCLAIM** | `worker._renew_leases()` -- renews message leases to prevent XAUTOCLAIM reclaiming active tasks | HIGH | MEDIUM |

### Behavioral Gaps [VERIFIED: codebase analysis + empirical testing]

| Gap | Where Manifests | Priority | Fix |
|-----|----------------|----------|-----|
| **XREADGROUP `block` ignored** | Worker delivery loop -- causes timing race with delayed tasks (~19% failure rate) | CRITICAL | Implement block with notification mechanism |
| **XTRIM `approximate` parameter** | `docket.clear()` calls `pipeline.xtrim(key, maxlen=0, approximate=False)` | LOW | Add `approximate` kwarg to xtrim (ignore it -- embedded DB is always exact) |

### Potential Additional Gaps (Require Inventory via D-05)

Running pydocket's full test suite (72 test files) may reveal additional gaps not visible from static code analysis. Areas likely to surface issues:

1. **Concurrency limit tests** (`tests/concurrency_limits/`) -- may exercise XCLAIM, XAUTOCLAIM edge cases
2. **Redelivery tests** (`tests/test_redelivery.py`, `tests/concurrency_limits/test_redelivery.py`) -- exercise XAUTOCLAIM and XCLAIM
3. **Cancel tests** (`tests/test_cancellation.py`) -- exercise cancel Lua script, pub/sub interaction
4. **Results storage** (`tests/test_results_retrieval.py`, `tests/test_results_storage.py`) -- may use commands not yet identified
5. **Clear tests** (`tests/test_docket_clear.py`) -- exercises XTRIM with approximate parameter
6. **Progress tests** (`tests/test_progress_basics.py`, `tests/test_progress_pubsub.py`) -- exercises HSET mapping, HINCRBY, pub/sub
7. **Strike list tests** (`tests/test_strikelist.py`, `tests/test_striking.py`) -- exercises XADD, XREAD for strike stream

## Architecture Patterns

### Pattern: Running Pydocket's Test Suite Against BurnerRedis

**Recommended approach: conftest.py fixture override**

Pydocket's tests use a Docker Redis container (via `redis_server` fixture). To run against BurnerRedis:

1. Clone pydocket's repo or vendor its test directory
2. Create a conftest.py override that replaces `redis_url` fixture to provide a BurnerRedis-backed URL
3. Monkey-patch `RedisConnection` (same pattern as `tests/test_pydocket_compat.py`)
4. Run with `pytest tests/` -- skip cluster-mode tests

```python
# conftest.py override concept
@pytest.fixture
def redis_url(burner):
    """Override pydocket's redis_url to use BurnerRedis."""
    # Monkey-patch RedisConnection to use burner instance
    monkeypatch_redis_connection(burner)
    return "redis://localhost"  # URL is ignored, monkey-patch handles routing
```

**Alternative: subprocess approach** -- run pydocket's tests via subprocess with environment overrides. Less control but simpler setup.

### Pattern: Adding New Commands (Established in Prior Phases)

Every new command follows the four-layer pattern: [VERIFIED: codebase patterns from Phase 2-9]

1. **Store method** in `src/store.rs`: `pub fn xclaim(&self, ...) -> Result<..., StoreError>`
2. **PyO3 binding** in `src/lib.rs`: `fn xclaim<'py>(&self, py, ...) -> PyResult<Bound<'py, PyAny>>`
3. **Pipeline method** in `python/burner_redis/pipeline.py`: `def xclaim(self, ...): self._commands.append(...)`
4. **Lua dispatch** in `src/scripting.rs` (if needed from Lua): `"XCLAIM" => { ... }`

### Pattern: XREADGROUP Block Implementation

The block mechanism requires a notification channel so XREADGROUP can wait for new data:

```rust
// In Store struct:
pub struct Store {
    pub data: RwLock<HashMap<Bytes, ValueEntry>>,
    // New: notification for stream writes
    stream_notify: tokio::sync::Notify,
    // ... existing fields
}

// In xadd (Store method):
pub fn xadd(&self, ...) -> Result<...> {
    // ... existing logic ...
    self.stream_notify.notify_waiters();
    Ok(id_string)
}

// In xreadgroup:
pub async fn xreadgroup_blocking(
    &self, group, consumer, keys, ids, count, block_ms: Option<u64>
) -> Result<...> {
    // Try non-blocking first
    let result = self.xreadgroup(group, consumer, keys, ids, count)?;
    if !result.is_empty() || block_ms.is_none() || block_ms == Some(0) {
        return Ok(result);
    }
    // Wait for notification or timeout
    let timeout = Duration::from_millis(block_ms.unwrap());
    tokio::select! {
        _ = self.stream_notify.notified() => {
            // Retry after notification
            self.xreadgroup(group, consumer, keys, ids, count)
        }
        _ = tokio::time::sleep(timeout) => {
            Ok(Vec::new())
        }
    }
}
```

**Important:** The Lua `dispatch_command` XADD must ALSO trigger the notification. Since dispatch_command operates on raw data (not through Store methods), the notification must be wired through the data reference or a separate channel.

## XCLAIM Command Specification

### Redis XCLAIM Semantics [CITED: https://redis.io/docs/latest/commands/xclaim/]

`XCLAIM key group consumer min-idle-time id [id ...] [IDLE ms] [TIME ms-unix-time] [RETRYCOUNT count] [FORCE] [JUSTID] [LASTID id]`

Transfers ownership of pending stream entries from one consumer to another. Used by pydocket for:
- Lease renewal: `xclaim(stream, group, consumer, min_idle_time=0, message_ids=[...], idle=0)` -- resets idle time to prevent XAUTOCLAIM from reclaiming

**redis-py signature:**
```python
async def xclaim(
    self, name, groupname, consumername, min_idle_time, message_ids,
    idle=None, time=None, retrycount=None, force=False, justid=False
) -> list
```

**Return value:** List of successfully claimed messages as `[(id, {field: value}), ...]` or `[id, ...]` if JUSTID.

### Implementation Requirements

For pydocket, only the core case matters:
- Transfer entries in PEL from any consumer to the specified consumer
- Support `idle` parameter to reset idle time
- min_idle_time filtering (skip entries idle less than threshold)
- Return format: list of `(id, fields)` tuples

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Running pydocket's test suite | Custom test runner | pytest with conftest override | pydocket uses standard pytest; just swap fixtures |
| XREADGROUP blocking | Sleep-retry loop in Python | tokio::sync::Notify in Rust | Must be correct at Store level (D-08), Notify is zero-cost when not waiting |
| Stream change notifications | Polling timer | tokio::sync::Notify | Purpose-built for "wake up waiters when state changes" pattern |

## Common Pitfalls

### Pitfall 1: Misdiagnosing the XREADGROUP Race

**What goes wrong:** Attempting to fix `last_delivered_id` tracking when the real issue is async timing.
**Why it happens:** The xfail message says "last_delivered_id timing" which suggests a data correctness issue.
**How to avoid:** The research confirms data correctness is fine. The fix is implementing `block` support.
**Warning signs:** If tests still fail ~19% of the time after a "fix", the wrong thing was fixed.

### Pitfall 2: Notification Not Reaching XREADGROUP After Lua XADD

**What goes wrong:** `dispatch_command` XADD writes to stream but doesn't trigger the Notify, so blocking XREADGROUP never wakes up.
**Why it happens:** `dispatch_command` operates on raw `&mut HashMap` without access to Store's Notify.
**How to avoid:** Thread the Notify through to dispatch_command (like pubsub_tx is already threaded through), or use a return-value signal that the caller (Store::eval) checks to fire the notification.
**Warning signs:** Blocking XREADGROUP works for direct XADD but not for Lua XADD.

### Pitfall 3: Deadlock with Blocking XREADGROUP

**What goes wrong:** If XREADGROUP holds the write lock while waiting, no other operation can proceed (including the XADD that would wake it up).
**Why it happens:** Store methods acquire RwLock synchronously. Blocking inside the lock = deadlock.
**How to avoid:** XREADGROUP must NOT hold the lock while waiting. Pattern: try non-blocking read (with lock) -> if empty, release lock -> await notification -> reacquire lock -> retry.
**Warning signs:** All operations hang when any consumer is blocking.

### Pitfall 4: XCLAIM Without Full PEL Transfer

**What goes wrong:** XCLAIM implementation only resets idle time but doesn't transfer PEL entry ownership between consumers.
**Why it happens:** Pydocket's primary use is lease renewal (same consumer), so tests might pass without proper cross-consumer transfer.
**How to avoid:** Implement full XCLAIM semantics including consumer transfer, even if pydocket only uses self-claim.
**Warning signs:** Redelivery tests fail because XAUTOCLAIM can't find entries that XCLAIM should have transferred.

### Pitfall 5: Pipeline Method Missing for New Commands

**What goes wrong:** New command works via direct call but fails in pipeline context.
**Why it happens:** Forgetting to add the pipeline buffer method in `pipeline.py`.
**How to avoid:** Checklist: every new command gets Store + PyO3 + Pipeline + (optionally) Lua dispatch.
**Warning signs:** Tests pass individually but fail when using `async with redis.pipeline()`.

## Code Examples

### XCLAIM Store Implementation Pattern

```rust
// Source: Based on existing XAUTOCLAIM pattern in src/store.rs
pub fn xclaim(
    &self,
    key: &Bytes,
    group: &Bytes,
    consumer: &Bytes,
    min_idle_time: u64,
    ids: &[StreamId],
    idle: Option<u64>,
) -> Result<Vec<(StreamId, HashMap<Bytes, Bytes>)>, StoreError> {
    let mut data = self.data.write();
    // ... get stream, get consumer group ...
    
    let mut claimed = Vec::new();
    for &id in ids {
        // Find entry in ANY consumer's PEL
        let mut found_consumer = None;
        for (cname, c) in &cg.consumers {
            if let Some(pe) = c.pending.get(&id) {
                let idle_ms = pe.delivery_time.elapsed().as_millis() as u64;
                if idle_ms >= min_idle_time {
                    found_consumer = Some(cname.clone());
                }
                break;
            }
        }
        
        if let Some(from_consumer) = found_consumer {
            // Remove from source consumer's PEL
            let pe = cg.consumers.get_mut(&from_consumer).unwrap()
                .pending.remove(&id).unwrap();
            
            // Add to target consumer's PEL with updated delivery info
            let target = cg.consumers.entry(consumer.clone())
                .or_insert_with(|| Consumer { pending: HashMap::new() });
            target.pending.insert(id, PendingEntry {
                delivery_time: if idle.is_some() {
                    Instant::now()  // Reset idle time
                } else {
                    pe.delivery_time
                },
                delivery_count: pe.delivery_count + 1,
            });
            
            // Return the entry data
            if let Some(fields) = stream.entries.get(&id) {
                claimed.push((id, fields.clone()));
            }
        }
    }
    
    Ok(claimed)
}
```

### Blocking XREADGROUP Pattern (Conceptual)

```rust
// In lib.rs PyO3 binding -- the async wrapper enables blocking
fn xreadgroup<'py>(
    &self, py: Python<'py>,
    groupname, consumername, streams, count, block, noack,
) -> PyResult<Bound<'py, PyAny>> {
    let store = self.store.clone();
    // ... parse args ...
    
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        // Non-blocking attempt
        let result = store.xreadgroup(&group, &consumer, &keys, &ids, count)?;
        if !result.is_empty() || block_ms.is_none() {
            return format_result(result);
        }
        
        let block_duration = Duration::from_millis(block_ms.unwrap());
        // Wait for stream notification or timeout
        tokio::select! {
            _ = store.stream_notified() => {
                let result = store.xreadgroup(&group, &consumer, &keys, &ids, count)?;
                format_result(result)
            }
            _ = tokio::time::sleep(block_duration) => {
                format_result(Vec::new())
            }
        }
    })
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `block` parameter ignored | Implement with Notify | This phase | Fixes ~19% delayed task race |
| 4 pass / 1 xfail pydocket tests | Full pydocket suite green | This phase | Validates production readiness |

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | The `block` parameter fix will resolve the delayed task race | XREADGROUP Race Analysis | If wrong, there's a deeper data visibility issue to investigate. Mitigation: empirical testing after fix. |
| A2 | pydocket v0.19.0 test suite can run against BurnerRedis with conftest monkey-patching | Architecture Patterns | If wrong, need to investigate pydocket test infrastructure more deeply. May need to vendor and modify tests. |
| A3 | XCLAIM is the only major missing command | Gap Inventory | If wrong, the D-05 inventory step will discover additional gaps. This is by design. |
| A4 | The Notify approach won't cause performance regression for non-blocking callers | Code Examples | Tokio Notify is zero-cost when no waiters exist, so this should be safe. [ASSUMED based on Tokio docs knowledge] |

## Open Questions

1. **How to run pydocket's test suite**
   - What we know: pydocket has 72 test files in tests/ directory, uses pytest with Docker Redis fixtures
   - What's unclear: Which tests can run without modification? Which require cluster mode? How many need the `key_leak_checker` (which calls FLUSHALL)?
   - Recommendation: Clone pydocket repo, create conftest override, run and triage failures (D-05 inventory step)

2. **Exact scope of XCLAIM needed**
   - What we know: pydocket uses XCLAIM for lease renewal (same consumer, idle=0)
   - What's unclear: Do any pydocket tests exercise cross-consumer XCLAIM?
   - Recommendation: Implement full XCLAIM semantics per D-03

3. **Whether `Notify` needs to be per-stream or global**
   - What we know: XREADGROUP waits on specific streams
   - What's unclear: Whether spurious wakeups from unrelated streams cause performance issues
   - Recommendation: Start with global Notify (simple), optimize to per-stream only if needed

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | pytest 9.0.3 + pytest-asyncio 1.3.0 |
| Config file | pyproject.toml `[tool.pytest.ini_options]` |
| Quick run command | `.venv/bin/python -m pytest tests/ -q --tb=short -m 'not integration'` |
| Full suite command | `.venv/bin/python -m pytest tests/ -q --tb=short -m integration` |

### Phase Requirements -> Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| GAP-01 | XREADGROUP block parameter functional | integration | `.venv/bin/python -m pytest tests/test_pydocket_compat.py::test_docket_add_delayed_task -m integration --runxfail -x` | Yes (xfail) |
| GAP-02 | XCLAIM command implemented | unit | `.venv/bin/python -m pytest tests/test_streams.py -k xclaim -x` | No -- Wave 0 |
| GAP-03 | All pydocket tests pass | integration | `.venv/bin/python -m pytest tests/test_pydocket_compat.py -m integration -x` | Partial |
| GAP-04 | No regressions in existing tests | unit | `.venv/bin/python -m pytest tests/ -q -m 'not integration'` | Yes (282 tests) |

### Sampling Rate
- **Per task commit:** `.venv/bin/python -m pytest tests/ -q -m 'not integration' -x`
- **Per wave merge:** `.venv/bin/python -m pytest tests/ -q -m integration`
- **Phase gate:** Full suite including integration tests with zero xfails

### Wave 0 Gaps
- [ ] `tests/test_streams.py` -- add XCLAIM test cases
- [ ] `tests/test_pydocket_compat.py` -- expand with regression tests for each gap fixed
- [ ] Pydocket test suite runner -- conftest override for running pydocket's own tests

## Security Domain

Not applicable for this phase. The work involves implementing Redis command compatibility in an embedded in-process database. No authentication, authorization, or cryptographic operations are introduced.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Rust compiler | Rust code changes | Yes | 1.86.0 | -- |
| maturin | Build Python wheels | Yes | via .venv | -- |
| pydocket | Test suite target | Yes | 0.19.0 | -- |
| pytest | Test runner | Yes | 9.0.3 | -- |
| pytest-asyncio | Async test support | Yes | 1.3.0 | -- |
| pydocket source (GitHub) | Running pydocket's own tests (D-04) | Via GitHub clone | -- | Use our own integration tests only |

**Missing dependencies with no fallback:** None

**Missing dependencies with fallback:**
- pydocket source repo for full test suite -- can use our integration tests as proxy, but D-04 says to run their suite

## Sources

### Primary (HIGH confidence)
- `src/store.rs` -- XREADGROUP implementation (lines 1438-1540), ConsumerGroup struct (lines 83-88), XADD (lines 1199-1251)
- `src/scripting.rs` -- Lua dispatch_command XADD (lines 1143-1219), all dispatch commands
- `src/lib.rs` -- PyO3 bindings, block parameter ignored (line 1095)
- `python/burner_redis/pipeline.py` -- all pipeline methods
- `tests/test_pydocket_compat.py` -- 5 integration tests, 4 pass, 1 xfail
- Empirical testing: 100-iteration race reproduction (81 pass, 19 fail)

### Secondary (MEDIUM confidence)
- [chrisguidry/docket GitHub](https://github.com/chrisguidry/docket) -- pydocket source, 72 test files
- pydocket v0.19.0 installed source at `.venv/lib/python3.14/site-packages/docket/`
- `docket/worker.py` -- scheduler Lua script, XCLAIM for lease renewal, XREADGROUP blocking
- `docket/execution.py` -- schedule/cancel/claim Lua scripts
- `docket/docket.py` -- XGROUP CREATE, clear(), snapshot()
- `docket/strikelist.py` -- XADD, XREAD for strike stream monitoring

### Tertiary (LOW confidence)
- [Redis XCLAIM docs](https://redis.io/docs/latest/commands/xclaim/) -- command specification (not verified in this session, from training data)

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- no new dependencies, all existing patterns
- Architecture (XREADGROUP block fix): HIGH -- root cause confirmed empirically, Tokio Notify well-understood
- Architecture (XCLAIM): HIGH -- follows established command implementation pattern
- Gap inventory completeness: MEDIUM -- static analysis covers core paths, D-05 inventory step will reveal edge cases
- Pitfalls: HIGH -- identified from deep codebase analysis

**Research date:** 2026-04-14
**Valid until:** 2026-05-14 (stable -- pydocket v0.19.0 frozen, codebase well-understood)
