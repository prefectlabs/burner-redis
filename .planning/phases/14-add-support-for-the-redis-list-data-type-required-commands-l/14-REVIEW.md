---
phase: 14-add-support-for-the-redis-list-data-type-required-commands-l
type: code-review-external
status: open
reviewed: 2026-04-25
depth: focused
provenance: external review (post-P2-round-1 follow-up; targets Lua + same-key list moves)
files_reviewed:
  - src/scripting.rs
  - src/store.rs
files_reviewed_list:
  - src/scripting.rs
  - src/store.rs
tally:
  critical: 0
  high: 0
  medium: 0
  low: 0
  p2: 3
prior_round_artifacts:
  - 14-REVIEW.p2-round-1.md
  - 14-REVIEW-FIX.p2-round-1.md
---

# Phase 14 External Code Review: Redis List Data Type (P2 Round 2)

**Summary:** A second pass over the list implementation surfaces three Redis-compatibility
correctness gaps that the visible test suite passes around. Two live in the Lua
scripting layer (`src/scripting.rs`), one in the direct-store list-move path
(`src/store.rs`); the Lua move arms duplicate the same `remove → recreate` and
`dst-type-before-src-check` patterns, so each fix lands in two places.

All findings are severity **P2** (medium-priority Redis-compat divergence — not
crashes or data loss, but observable behavioral wrongness against `redis-py` /
real Redis). Numbering continues from the round-1 P2 series (P2-01..P2-07
closed in `14-REVIEW-FIX.p2-round-1.md`); these are P2-08..P2-10.

---

## P2-08 — Preserve Lua list notifications on script errors

**File:** `src/scripting.rs:269-271`

**Problem:** Scripts can mutate the store before raising — Redis does not roll
back earlier writes inside an `EVAL`. Today the `map_err` at the end of
`eval_script` discards the captured `had_list_mutation` flag on the error path,
so `Store::eval` / `Store::evalsha` never call `list_notify.notify_waiters()`
when a script errors mid-way. If a script does `LPUSH`/`RPUSH` and then
`redis.call(...)` errors while another task is parked in `BLPOP`/`BRPOP` with
`timeout=0`, the waiter can remain asleep even though an element exists. The
waiter eventually wakes only when some unrelated list write happens. Finite
timeouts return `None` instead of the pushed element.

**Example:**
```python
# Task A (waiter) parked indefinitely:
await r.blpop(['k'], timeout=0)

# Task B runs a script that pushes then errors:
await r.eval("redis.call('LPUSH', KEYS[1], 'x'); return redis.call('FOO')", 1, 'k')
# Task A should wake up with ['k', 'x'] because LPUSH succeeded;
# currently it stays parked until some other list write fires the notify.
```

**Fix:** In `Scripting::eval_script` at `src/scripting.rs:269-271`, capture
`had_xadd` and `had_list_mutation` *before* `map_err`, then call the
corresponding `notify_waiters` paths from `Store::eval`/`Store::evalsha` on
both the `Ok` and `Err` branches. Equivalently, keep returning a tuple from
`eval_script` and propagate the flags through the error variant (e.g. return
`Result<(RedisValue, bool, bool), (String, bool, bool)>` or wrap the error
with the flags) so the caller fires `list_notify` regardless of script
outcome. Preserve the existing assumption-log A2 / M-04 semantics: only
list-grow operations set the flag.

**Tests to add:**
- `test_lua_eval_pushes_then_errors_wakes_blpop_waiter` — task A `BLPOP timeout=0`,
  task B `EVAL` that `LPUSH`es then errors; task A returns the pushed element
  (currently hangs).
- `test_lua_eval_pushes_then_errors_finite_blpop_returns_value` — finite-timeout
  variant must return the value rather than `None`.

---

## P2-09 — Preserve TTL for same-key list moves

**File:** `src/store.rs:3312-3313` (and the duplicated Lua move arms in
`src/scripting.rs:2501-2503` and `src/scripting.rs:2578-2580`)

**Problem:** When `src == dst` and the list has exactly one element,
`pop_*()` empties the list, `src_empty` becomes true, and the unconditional
`data.remove(src)` deletes the key — including its `expires_at`. The
subsequent `data.entry(dst).or_insert_with(ValueEntry::new_list)` then
recreates a fresh entry with no TTL. So `RPOPLPUSH k k` / `LMOVE k k LEFT
RIGHT` clears an existing expiry on a key that should be a pure rotation.
Real Redis preserves TTL across same-key moves because the key is never
removed — it's just rewritten. The same `remove → re-create` shape lives in
the Lua `LMOVE` arm at `src/scripting.rs:2501-2503` and the Lua `RPOPLPUSH`
arm at `src/scripting.rs:2578-2580`.

**Example:**
```python
await r.rpush('k', 'a')
await r.expire('k', 60)
await r.rpoplpush('k', 'k')          # rotation
ttl = await r.ttl('k')
# Currently ttl == -1 (TTL cleared); should be ~60.
```

**Fix:** In `Store::lmove_atomic` (`src/store.rs:3312-3313`), gate the
`data.remove(src)` on `src != dst`. When `src == dst`, leave the entry in
place — the existing `data.entry(dst).or_insert_with(...)` call resolves to
the still-present entry, preserving `expires_at`. Apply the same gate to
both Lua arms:
- `src/scripting.rs:2501-2503` (LMOVE)
- `src/scripting.rs:2578-2580` (RPOPLPUSH)

If the same-key entry's list became empty before push, the immediate push
re-fills it (single-element rotation), so the entry transitioning through an
empty inner `VecDeque` is fine — what must not change is the outer
`ValueEntry`. Audit `Store::rpoplpush_atomic` at `src/store.rs:3338+` for the
same pattern and apply the same fix if present.

**Tests to add:**
- `test_rpoplpush_same_key_preserves_ttl` — set + expire + rotate, assert
  TTL within ±1s of original.
- `test_lmove_same_key_preserves_ttl` — LMOVE rotation mirror.
- `test_lua_lmove_same_key_preserves_ttl` — `EVAL` calling `LMOVE k k ...`
  preserves TTL.
- `test_lua_rpoplpush_same_key_preserves_ttl` — `EVAL` calling `RPOPLPUSH k k`
  preserves TTL.
- `test_rpoplpush_diff_keys_does_not_carry_src_ttl` — negative control:
  cross-key moves must NOT propagate TTL to dst (regression guard against an
  over-eager fix).

---

## P2-10 — Check Lua move source before destination type

**File:** `src/scripting.rs:2462-2467` (and the duplicated RPOPLPUSH arm at
`src/scripting.rs:2544-2553`)

**Problem:** In the Lua `LMOVE` dispatch, the destination type-check
(`!matches!(dst_entry.data, ValueData::List(_))`) runs *before* verifying
that the source key exists or has a poppable element. So
`redis.call('LMOVE', missing, string_dst, 'LEFT', 'RIGHT')` raises
`WRONGTYPE` even though the operation should be a no-op (return `nil`)
because there is nothing to move. `Store::lmove_atomic` already gets this
right after the round-1 P2-02 fix — it returns `Ok(None)` for missing/empty
source without inspecting `dst`. Redis scripts use the same command
semantics as direct calls, so the Lua arm must match. The duplicated
`RPOPLPUSH` block at `src/scripting.rs:2544-2553` has the same ordering bug.

**Example:**
```python
await r.set('string_dst', 'x')
# Direct call (post-P2-02): returns None
await r.rpoplpush('missing', 'string_dst')    # → None (correct)

# Same op via Lua: still WRONGTYPE
await r.eval(
    "return redis.call('LMOVE', KEYS[1], KEYS[2], 'LEFT', 'RIGHT')",
    2, 'missing', 'string_dst',
)
# Currently raises WRONGTYPE; should return nil.
```

**Fix:** In `src/scripting.rs:2462-2467` (LMOVE), move the destination
type-check to *after* the source-pop block, mirroring the direct
`Store::lmove_atomic` ordering: confirm the source exists and has a
poppable element first; if it doesn't, return `RedisValue::Nil` immediately
without touching `dst`. Only validate `dst` once there is a value to move,
and keep the type-check just before the push to preserve atomicity (the
dispatch already holds the data lock for the whole arm).

Apply the symmetric fix to the duplicated RPOPLPUSH block at
`src/scripting.rs:2544-2553`. If the source-pop block is refactored into a
shared helper, both arms benefit at once and the duplication that produced
the round-2 finding goes away.

**Tests to add:**
- `test_lua_lmove_missing_src_with_string_dst_returns_nil` — Lua `LMOVE`
  with missing src + string dst returns `nil` (was: WRONGTYPE).
- `test_lua_rpoplpush_missing_src_with_string_dst_returns_nil` — Lua
  RPOPLPUSH mirror.
- `test_lua_lmove_empty_src_with_string_dst_returns_nil` — empty-list src
  variant.
- `test_lua_lmove_nonempty_src_with_string_dst_still_wrongtype` —
  atomicity guard: when src DOES have an element, dst type-check must
  still fire BEFORE pop (matches the round-1 P2-02 atomicity test).

---

## Notes

- All three findings are P2 (medium): observable Redis-compat divergence,
  not crashes or data-loss bugs. Each is independently testable and the
  fixes are surgical.
- The Lua `LMOVE` and `RPOPLPUSH` arms are textbook duplicates of each
  other. Two of the three findings (P2-09, P2-10) land in both arms — the
  fixer should land each fix in both arms in the same atomic commit, or
  refactor to a shared helper as part of the fix.
- The round-1 P2 source and fix report are preserved at
  `14-REVIEW.p2-round-1.md` and `14-REVIEW-FIX.p2-round-1.md`. Round-1
  findings P2-01..P2-07 are CLOSED.
- Apply on `phase-14-redis-lists` branch; commit prefix `fix(14): P2-NN ...`
  to match the established pattern.
