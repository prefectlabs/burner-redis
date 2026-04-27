# Phase 14: List data type (LPUSH, BRPOP, BLPOP, and full list command set) - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-04-24
**Phase:** 14-add-support-for-the-redis-list-data-type-required-commands-l
**Areas discussed:** Phase scope, Blocking notify architecture, Lua redis.call() coverage, Pipeline + blocking interaction

---

## Phase scope

### Q1: What command surface should Phase 14 deliver?

| Option | Description | Selected |
|--------|-------------|----------|
| Required-only (3 cmds) | LPUSH, BRPOP, BLPOP only. Minimal plan (~1 plan, fastest delivery). BRPOP/BLPOP without RPOP/LPOP companions is oddly shaped. | |
| Full stretch (16 cmds) | All 16 commands: LPUSH, RPUSH, LPOP, RPOP, LRANGE, LLEN, LINDEX, LINSERT, LREM, LSET, LTRIM, LMOVE, RPOPLPUSH, BRPOP, BLPOP, BLMOVE. Cohesive surface, matches how redis-py ships. | ✓ |
| Core + blocking (7 cmds) | LPUSH, RPUSH, LPOP, RPOP, LRANGE, BRPOP, BLPOP. Skips introspection/mutation and LMOVE/BLMOVE/RPOPLPUSH. | |
| Everything except legacy | All stretch EXCEPT RPOPLPUSH (superseded by LMOVE). 15 commands. | |

**User's choice:** Full stretch (16 commands)
**Notes:** Cohesive surface, one phase instead of two. Pattern is well-established after 13 phases.

### Q2: LPOP/RPOP count argument support?

| Option | Description | Selected |
|--------|-------------|----------|
| Full redis-py parity | LPOP/RPOP support count: None returns single bytes (or None), count=N returns list[bytes] of up to N popped elements (or None if empty). | ✓ |
| Single-element only | LPOP/RPOP ignore count. Simpler but creates a drop-in gap. | |

**User's choice:** Full redis-py parity
**Notes:** Matches redis-py exactly; drop-in compatibility is the governing discipline.

---

## Blocking notify architecture

### Q1: How should BRPOP/BLPOP waiters wake up when LPUSH/RPUSH adds an element?

| Option | Description | Selected |
|--------|-------------|----------|
| New list_notify | Dedicated Arc<Notify> on Store (like stream_notify). BRPOP/BLPOP use the XREAD blocking pattern. Clean separation from streams. | ✓ |
| Reuse stream_notify | Use existing stream_notify for both streams and lists. Less infrastructure but spurious wakeups muddle the conceptual model. | |
| Per-key fine-grained notify | HashMap<Bytes, Arc<Notify>> keyed by list name, lazily created. Zero spurious wakeups but more bookkeeping. | |

**User's choice:** New list_notify
**Notes:** Matches Phase 11's proven pattern; simple to reason about.

### Q2: Multi-key BRPOP/BLPOP wake behavior?

| Option | Description | Selected |
|--------|-------------|----------|
| Check keys in order, first non-empty wins | Scan keys in order on wake; pop from first non-empty list. Matches Redis spec exactly. | ✓ |
| Any-first (no ordering guarantee) | Return from whichever key happens to be checked first. Violates Redis spec. | |

**User's choice:** Check keys in order, first non-empty wins
**Notes:** Only behavior that passes redis-py test suites.

### Q3: Where does list_notify.notify_waiters() get called?

| Option | Description | Selected |
|--------|-------------|----------|
| Inside the write lock | Call from Store method while holding write lock, matching existing XADD pattern. Waiters re-acquire lock anyway. | ✓ |
| After lock drop | Return from Store method, drop lock, then notify from PyO3 binding layer. Theoretical contention reduction, adds cross-layer coordination. | |

**User's choice:** Inside the write lock
**Notes:** Matches store.rs:1262 and 2402 XADD pattern; simpler code.

### Q4: BRPOP timeout type?

| Option | Description | Selected |
|--------|-------------|----------|
| Match redis-py exactly | Accept float seconds (0 = infinite, positive = deadline). Convert to milliseconds at Python layer, pass as Option<u64> to Rust. | ✓ |
| Integer seconds only | Reject float timeouts. Breaks redis-py compatibility. | |

**User's choice:** Match redis-py exactly
**Notes:** Zero special-cased in Rust loop identical to xread/xreadgroup block=0.

---

## Lua redis.call() coverage

### Q1: Should list commands be callable from Lua scripts?

| Option | Description | Selected |
|--------|-------------|----------|
| All non-blocking + reject blocking | Add all non-blocking list commands to dispatch_command_inner. BRPOP/BLPOP/BLMOVE from Lua return error matching real Redis. | ✓ |
| All commands including blocking | Allow BRPOP/BLPOP/BLMOVE from Lua, degrading to non-blocking variants. Silently divergent from Redis. | |
| Required-only subset | Only LPUSH in Lua. Leaves unpredictable holes. | |
| Defer all | No list commands in dispatch_command_inner. | |

**User's choice:** All non-blocking + reject blocking
**Notes:** Real Redis blocks BRPOP/BLPOP from scripts (would deadlock — scripts are atomic). Maintains compat contract.

### Q2: Lua list mutations fire list_notify.notify_waiters()?

| Option | Description | Selected |
|--------|-------------|----------|
| Yes, fire notify from Lua dispatch | Return a had_list_mutation flag from dispatch_command alongside had_xadd, call list_notify.notify_waiters() after Lua execution. Prevents the class-of-bug Phase 11 fixed for streams. | ✓ |
| No, skip notify from Lua | Lua LPUSH writes to HashMap but doesn't notify. Would re-introduce exactly the bug Phase 11 fixed. | |

**User's choice:** Yes, fire notify from Lua dispatch
**Notes:** Direct analog to Phase 11's XADD-from-Lua fix pattern.

---

## Pipeline + blocking interaction

### Q1: Pipeline containing BRPOP/BLPOP/BLMOVE behavior?

| Option | Description | Selected |
|--------|-------------|----------|
| Respect the timeout per-command | Each blocking command blocks up to its timeout during execute(). Matches redis-py semantics — pipelines are batched, not atomic; blocking commands do block. | ✓ |
| Degrade to non-blocking in pipelines | BRPOP/BLPOP inside pipeline ignore timeout, return immediately. Faster but silently diverges from redis-py. | |
| Raise a clear error | Raise at queue time: "Blocking commands not supported in pipelines". Forces correct usage but breaks redis-py composability. | |

**User's choice:** Respect the timeout per-command
**Notes:** Consistent with user mental model; matches redis-py exactly.

### Q2: Pipeline dispatch path for blocking commands?

| Option | Description | Selected |
|--------|-------------|----------|
| Async pipeline path for blocking | execute_pipeline() detects blocking commands. None present → sync fast path. Any present → per-command async loop. | ✓ |
| Always async pipeline | Unifies path but regresses sync fast-path performance win from quick task 260415-an2. | |
| Sync loop + tokio::Handle::block_on | Nested blocking on tokio runtime from inside tokio task is a known deadlock source. | |

**User's choice:** Async pipeline path for blocking
**Notes:** Preserves 260415-an2 sync fast path for non-blocking pipelines (common case); pays async cost only when unavoidable.

---

## Claude's Discretion

- Exact helper boundary between `src/store.rs` and `src/commands/lists.rs`
- LRANGE negative-index normalization logic
- LINSERT pivot-not-found return code
- LREM count-sign semantics
- LPOP count=0 exact return (empty list vs None — follow redis-py real behavior)
- Internal organization of 16 new `#[pymethods]` in `src/lib.rs`
- Plan split (2-plan engine+python vs 3-plan engine/python/lua+pipeline)

## Deferred Ideas

- BRPOPLPUSH (blocking legacy variant) — not in ROADMAP stretch, superseded by BLMOVE
- Per-key fine-grained notify (HashMap<Bytes, Arc<Notify>>) — over-engineered for embedded single-process use
- LPOS command — not requested, not in ROADMAP
