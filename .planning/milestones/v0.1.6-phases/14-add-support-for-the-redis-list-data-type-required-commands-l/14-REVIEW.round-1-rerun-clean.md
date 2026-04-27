---
phase: 14-add-support-for-the-redis-list-data-type-required-commands-l
type: code-review-rerun
status: clean
reviewed: 2026-04-24
depth: standard
files_reviewed: 8
files_reviewed_list:
  - python/burner_redis/__init__.py
  - python/burner_redis/pipeline.py
  - src/commands/lists.rs
  - src/commands/mod.rs
  - src/lib.rs
  - src/scripting.rs
  - src/store.rs
  - tests/test_lists.py
tally:
  critical: 0
  high: 0
  medium: 0
  low: 0
  info: 2
---

# Phase 14 Re-Review: Redis List Data Type (Post-Fix Pass)

**Verdict:** All five fixes from commits `bf88fb5` (H-01), `123ab8f` (M-01), `567dae8`
(M-02 + M-05), `ea05cc9` (M-04), and `a0dd54a` (M-01 test tightening) are correct
and do not introduce regressions. M-03 (notify_waiters optimization) was deliberately
deferred per the prior review's own guidance and is not addressed here. No new issues
were introduced. Two informational notes are recorded for future scoping decisions
but are NOT phase-14 blockers.

## Fix-by-fix verification

### H-01 — Pipeline value coercion (commit `bf88fb5`)

**Status: correct.**

`python/burner_redis/pipeline.py` now coerces values at buffer time for `set` (line 117),
`lpush` (line 199), `rpush` (line 205), `linsert` (line 232 — only the inserted value,
not the pivot), and `lset` (line 241). This mirrors exactly the monkey-patched client
methods in `__init__.py`:

- `_coerced_set` (line 67-69): coerces value
- `_coerced_lpush` (line 88-91): per-value coercion
- `_coerced_rpush` (line 100-103): per-value coercion
- `_coerced_lset` (line 112-114): coerces value
- `_coerced_linsert` (line 123-125): coerces value, leaves refvalue alone

The standalone `_coerce_value` in `pipeline.py` (lines 8-33) is a deliberate copy of
the function in `__init__.py` (lines 41-61) to avoid a circular import (the comment on
line 11-15 of pipeline.py documents this). The two implementations are byte-identical
in semantics:
- bytes/memoryview pass through
- bool rejected with TypeError BEFORE int check (because `bool` is `isinstance(v, int)`)
- int/float go through `repr(v).encode()`
- str passes through

Tests `test_pipeline_lpush_int_coerced`, `test_pipeline_rpush_float_coerced`,
`test_pipeline_lpush_bool_raises`, `test_pipeline_lset_int_coerced`,
`test_pipeline_linsert_int_coerced`, `test_pipeline_set_int_coerced`, and
`test_pipeline_set_bool_raises` (test_lists.py lines 737-803) lock in the parity.

The bool-raises tests correctly assert TypeError fires at *buffer time* (the
`pipe.lpush(...)` call itself), which matches expectation: coercion happens
before queueing.

### M-01 — Lua blocking-reject error wording (commits `123ab8f` + `a0dd54a`)

**Status: correct.**

`src/scripting.rs:2599-2601` now returns the exact real-Redis wording:
```
"ERR This Redis command is not allowed from script"
```
- singular `"script"` (was plural)
- no colon
- no command name appended

This goes through `LuaError::RuntimeError(msg)` at scripting.rs:189, which mlua wraps
with its own `runtime error: ...` prefix and traceback. The visible Python error after
the round-trip through `Store::eval`/`evalsha` includes a multi-line message: the first
line is the controlled error wording, subsequent lines are mlua's stack traceback.

The tightened test `test_lua_blocking_error_does_not_include_command_name`
(test_lists.py:570-591) correctly:
- splits on `splitlines()` and only checks `first_line` — robust against mlua's
  legitimate use of colons in source paths within tracebacks
- guards against both the old plural (`"from scripts"`) and the old colon-suffixed
  (`"from script:"`) wordings
- positively asserts the new substring `"not allowed from script"` is present

The three command-specific tests (lines 547-567) use `match="not allowed from script"`,
which correctly matches both the new and old (plural) wording — they don't regression-
guard the wording change on their own, but the explicit `test_lua_blocking_error_does_
not_include_command_name` test pins it down.

### M-02 — Slow-path BLPOP/BRPOP wake tests (commit `567dae8`)

**Status: correct.**

Two new tests in test_lists.py at lines 296-339:
`test_blpop_slow_path_wake_elapsed_lower_bound` and the BRPOP mirror.

The mechanism: a pusher task sleeps 0.15s then pushes; meanwhile `BLPOP/BRPOP`
runs with a 2.0s timeout. Since `asyncio.create_task` only schedules — it does not
run — the BLPOP `future_into_py` future starts and performs its first synchronous
`store.blpop_poll(&key_list)` BEFORE the sleeping pusher task gets to the LPUSH.
So the first poll returns None, the future enters the `tokio::select!` slow path,
and is woken by `list_notify.notify_waiters()` from inside the LPUSH write lock
(`src/store.rs:2787`).

The lower bound of `SLEEP * 0.8` (0.12s) gives ~20% slack for async scheduling jitter
while still excluding any race-won fast path (which would return in < 1ms). The upper
bound of 2.0s (the timeout itself) ensures we exited via the wake, not the timeout
expiry. Pre-existing `test_blpop_wakes_on_push`/`test_brpop_pops_from_tail` tests
remain in place; these new ones add the timing assertion that distinguishes "fast path
race-won" from "actual slow path wake."

The `await task` after the BLPOP awaits the pusher cleanly — no orphaned coroutines.

### M-04 — `had_list_mutation` per-command refinement (commit `ea05cc9`)

**Status: correct. Verified by reading the per-command return contracts in
`dispatch_command_inner`.**

The new `match cmd { ... }` block at `src/scripting.rs:302-315` correctly maps
each list-mutation command to its real success-discriminator:

| Command  | Returns on success         | Returns on no-op             | Match arm                          |
|----------|----------------------------|------------------------------|------------------------------------|
| LPUSH    | `Integer(list.len())`     | (n/a — always grows on success) | `Integer(_)` ✓                  |
| RPUSH    | `Integer(list.len())`     | (n/a)                        | `Integer(_)` ✓                     |
| LINSERT  | `Integer(list.len())` >0  | `Integer(0)` (key missing) or `Integer(-1)` (pivot not found) | `Integer(n) if n > 0` ✓ |
| LMOVE    | `BulkString(popped)`      | `Nil` (empty/missing source) | `BulkString(_)` ✓                  |
| RPOPLPUSH| `BulkString(popped)`      | `Nil` (empty/missing source) | `BulkString(_)` ✓                  |

I traced every return arm in `dispatch_command_inner`:

- **LPUSH** (scripting.rs:1847-1874): `Ok(Integer(list.len()))` after pushing 1+
  values, or `Ok(Error(WRONGTYPE))`. List length is always ≥1 on success. The
  `Integer(_)` match correctly fires only on success. ✓
- **RPUSH** (scripting.rs:1876-1902): same structure as LPUSH. ✓
- **LINSERT** (scripting.rs:2197-2241): `Ok(Integer(0))` if expired/missing key,
  `Ok(Integer(-1))` if pivot not found, `Ok(Integer(list.len()))` after insertion
  (always ≥1 since the list had ≥1 element matching the pivot before insert).
  The `Integer(n) if n > 0` match correctly excludes the 0 and -1 cases. ✓
- **LMOVE** (scripting.rs:2428-2519): `Ok(Nil)` if src missing or empty (after
  `pop_front`/`pop_back` returns None at lines 2477 and 2497); `Ok(BulkString(popped))`
  on the success path at line 2518; `Ok(Error(WRONGTYPE))` on type mismatches. The
  `BulkString(_)` match correctly fires only on actual moves. ✓
- **RPOPLPUSH** (scripting.rs:2521-2593): same shape as LMOVE — `Ok(Nil)` for empty/
  missing src, `Ok(BulkString(popped))` on success. ✓

WRONGTYPE error returns produce `RedisValue::Error(_)`, which never matches
`Integer(_)` or `BulkString(_)`, so the flag stays false. ✓

`Err(String)` short-circuits via the `?` at scripting.rs:299 before the match,
so any deeper failure also produces `had_list_mutation = false`. ✓

The fix correctly avoids spurious wakes of BLPOP/BRPOP waiters when a Lua script's
LINSERT no-ops on a missing key/pivot, or when LMOVE/RPOPLPUSH no-ops on an empty
source. This is a real correctness improvement: previously a Lua script that just
happened to issue an LINSERT against a missing key would wake every BLPOP waiter,
who would then re-poll, find nothing, and go back to sleep — wasted work and
potentially observable latency under contention.

**Re: prior fixer's "requires human verification" flag.** The prior reviewer flagged
M-04 because LINSERT no-op (`Integer(0)`/`Integer(-1)`) and LMOVE/RPOPLPUSH `Nil`
branches aren't directly asserted by tests. I verified the per-command match logic
matches the actual return contracts of `dispatch_command_inner` by tracing each
arm. The fix is correct as written. A follow-up that asserts no-spurious-wake at
the test level (e.g., a BLPOP against `k` that should NOT wake when a script does
`LINSERT k BEFORE missing_pivot v`) would be welcome but is NOT a phase-14 blocker
— the change is provably equivalent to checking "did the command actually grow
the list" by reading the dispatcher.

One observational note (no defect): direct (non-Lua) `Store::linsert` does NOT
fire `list_notify.notify_waiters()` at all (store.rs:3169-3204, see doc comment
at lines 3165-3168). The Lua path now correctly wakes on a successful LINSERT. So
direct LINSERT and Lua-LINSERT have diverging wake behaviors. This is consistent
with the documented intention in `RESEARCH.md` Assumption A2 and the existing doc
comment, and is being preserved deliberately. Captured below as IN-02 for future
visibility but is not a fix candidate.

### M-05 — Timing upper bounds (commit `567dae8`)

**Status: correct.**

`test_blpop_timeout_returns_none` (test_lists.py:236-244) and
`test_blmove_timeout_returns_none` (test_lists.py:371-377) both widen the upper
bound from 0.5s to 2.0s. The lower bound (0.05s) remains, which is the meaningful
assertion: we DID wait at least the requested 0.1s timeout (with 50% jitter slack).
The upper bound is now generous enough for loaded CI not to flake.

Comments at lines 241-243 and 376-377 correctly explain why the lower bound
carries the meaning. No regression in the test's value as a regression guard.

## Cross-file consistency checks

1. **Pipeline → store routing for list commands.** `dispatch_pipeline_command`
   in `src/lib.rs:3556-3770` covers all 13 non-blocking list commands plus
   `lmove`/`rpoplpush`/`linsert`. The pipeline's slow-path (Python-side iterate)
   correctly handles `blpop`/`brpop`/`blmove` by deferring to the awaitable
   methods on the client. No method gaps.

2. **Notify protocol invariants.** Direct mutations (LPUSH/RPUSH/LMOVE/RPOPLPUSH)
   call `list_notify.notify_waiters()` *inside* the data write lock (store.rs
   lines 2787, 2814, 3297). The Lua path takes the data write lock, runs the
   script, releases the data lock, and THEN fires `list_notify.notify_waiters()`
   if any growing mutation fired (store.rs:2433-2436, 2466-2467). Both protocols
   are correct: BLPOP waiters use the "arm-permit-before-poll" pattern (lib.rs:2522-2524,
   2636-2637, 2750-2751) so any wake during the first-poll window is captured.

3. **Type-check ordering.** LMOVE/RPOPLPUSH type-check destination BEFORE popping
   source (store.rs:3241-3247, scripting.rs:2462-2472, 2544-2553) — Pitfall 4 from
   RESEARCH. Verified consistent across both code paths.

4. **D-03 (delete-on-empty).** All pop paths (LPOP/RPOP/LMOVE-src/RPOPLPUSH-src/
   LREM/LTRIM/BLPOP/BRPOP) correctly delete the key when the list becomes empty
   (store.rs:2954-2961, 3274-3279, 3343-3346, 3373-3376; scripting.rs:2502-2503,
   2578-2580, 2422-2424). Tests `test_lpop_deletes_empty_key` and
   `test_ltrim_empty_result_deletes_key` verify this externally.

5. **`count=0` WRONGTYPE precedence.** LPOP/RPOP correctly type-check before the
   count=0 fast-return at store.rs:2918-2932 and 2975-2985. (Pitfall 4 again.)

## Information items (NOT phase-14 blockers)

### IN-01: Pipeline coercion gap for `setex`, `lrem`, etc.

**File:** `python/burner_redis/pipeline.py:235-237, 405-407`
**Issue:** H-01 fixed `set`/`lpush`/`rpush`/`lset`/`linsert` in the pipeline to mirror
the client. But `pipe.setex(name, time, value)` does NOT coerce `value`, while
`r.setex(name, time, 42)` succeeds (the client's `_setex` on `__init__.py:75-80`
calls `await self.set(...)` which goes through the monkey-patched coercer). Same
for `lrem` value, and several string/hash/sorted-set commands that lack pipeline
coercion entirely. For phase 14 this is OUT OF SCOPE — `lrem`'s value is also
not coerced at the *client* level (no `_coerced_lrem` wrapper exists in
`__init__.py`), so the pipeline correctly mirrors the client (both reject ints
with `extract_bytes` TypeError). `setex` is a string command and was never part
of phase 14. Logged here for cross-phase awareness.
**Fix:** N/A for phase 14. Future phase should audit all pipeline stubs vs.
`_coerced_*` wrappers and produce a complete parity table.

### IN-02: LINSERT wake-protocol divergence between direct and Lua paths

**File:** `src/store.rs:3165-3168`, `src/scripting.rs:302-315`
**Issue:** Direct `Store::linsert` deliberately does NOT fire `list_notify.notify_
waiters()`, per its own doc comment ("redis-py's LINSERT is documented as not
waking BRPOP waiters in real Redis"). After the M-04 fix, however, a Lua script's
LINSERT that actually inserts an element WILL fire `list_notify.notify_waiters()`
via the script's post-execution `had_list_mutation` branch (store.rs:2433-2436).
This is consistent with the broader Phase-11 guarantee that any list-grow
mutation visible to a waiter wakes them up — and is intentional per RESEARCH.md
Assumption A2 — but the asymmetry is worth pinning in a comment so future
maintainers don't "fix" one side or the other. (Real Redis's behavior is
debatable: LINSERT into a non-empty list could plausibly satisfy a BRPOP if the
list grew from 0; but since LINSERT can only insert when the pivot exists, the
list is by definition non-empty, so any waiter on that key is already a no-op
race anyway. Practical impact: nil. Pinning the asymmetry as a code comment is
the only ask.)
**Fix:** Optional follow-up — add a comment to `Store::linsert` referencing the
Lua-path divergence so the contract is explicit.

---

_Re-reviewed: 2026-04-24_
_Reviewer: Claude (gsd-code-reviewer, Opus 4.7 1M)_
_Depth: standard_
_All five targeted fixes verified correct. M-03 deferred per prior guidance._
