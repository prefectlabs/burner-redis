---
phase: 14-add-support-for-the-redis-list-data-type-required-commands-l
type: code-review-fix
status: all_fixed
fixed_date: 2026-04-25
review_source: 14-REVIEW.md (P2 round 2)
findings_in_scope: 3
fixed: 3
skipped: 0
iteration: 1
fix_scope: all
---

# Phase 14 Code Review Fix Report (P2 Round 2)

## Summary

All 3 P2 round-2 findings (P2-08, P2-09, P2-10) from the external review
were applied as atomic per-finding commits with regression tests added
alongside each fix. Each affirmative test was verified to fail under the
pre-fix code path and pass after the surgical change (atomicity-guard
tests pass both pre- and post-fix and exist purely to pin the ordering
behavior). The full Rust unit suite (`cargo test --lib` -- `149 passed`),
the lists test file (`pytest tests/test_lists.py` -- `122 passed`,
+11 vs. the round-1 closing baseline of 111), and the full Python suite
(`pytest -q` -- `502 passed, 38 deselected`, +11 vs. round-1's 491) are
all green.

Round-2 findings P2-08..P2-10 are CLOSED. Round-1 (P2-01..P2-07) is
preserved at `14-REVIEW-FIX.p2-round-1.md` and remains CLOSED.

## Fixes Applied

### P2-08 -- Preserve Lua list notifications on script errors
- **Commit:** `2d757a0`
- **Files:** `src/scripting.rs`, `src/store.rs`, `tests/test_lists.py`
- **Tests added:**
  - `test_lua_eval_pushes_then_errors_wakes_blpop_waiter` -- task A is
    parked in `BLPOP timeout=2.0`; task B runs `EVAL` that LPUSHes then
    raises via `redis.call('FOO')`. Task A must wake within 1s with
    the pushed element. Pre-fix: parks the full 2s.
  - `test_lua_eval_pushes_then_errors_finite_blpop_returns_value` --
    finite-timeout variant; must return `(b"k2", b"y")` rather than
    `None`.
- **Verification:** PASS (2/2 fail pre-fix, both pass post-fix).
  Direct verification by stash + rebuild + run cycle confirmed both
  tests fail with `elapsed >= 2.0s` against the pre-fix code, then
  pass after restoring the change.
- **Change:** Restructured `LuaEngine::execute` to return a new
  `EvalOutcome { result: Result<RedisValue, String>, had_xadd: bool,
  had_list_mutation: bool }` struct. Previously the function returned
  `Result<(RedisValue, bool, bool), String>`, and the `?` operator at
  the call sites in `Store::eval` / `Store::evalsha` discarded the
  flags whenever the script ultimately raised. Real Redis does not
  roll back earlier writes inside a script that errors mid-way, so any
  list-grow command that already ran must wake parked BRPOP/BLPOP
  waiters regardless of the script's outcome. The new struct carries
  the cumulative flags into both Ok and Err branches, and `eval` /
  `evalsha` now fire `stream_notify` / `list_notify` based on the
  flags BEFORE returning `outcome.result`.

### P2-09 -- Preserve TTL for same-key list moves
- **Commit:** `844f45b`
- **Files:** `src/scripting.rs`, `src/store.rs`, `tests/test_lists.py`
- **Tests added (4 affirmative + 1 negative control):**
  - `test_rpoplpush_same_key_preserves_ttl` -- direct
    `RPOPLPUSH k k` on a single-element list must preserve a 60s TTL.
  - `test_lmove_same_key_preserves_ttl` -- direct
    `LMOVE k k LEFT RIGHT` on a single-element list must preserve TTL.
    (Single-element triggers the empty-then-recreate path; multi-element
    rotation never empties so it would mask the bug.)
  - `test_lua_lmove_same_key_preserves_ttl` -- same via `EVAL`,
    exercising the Lua dispatch arm.
  - `test_lua_rpoplpush_same_key_preserves_ttl` -- RPOPLPUSH mirror
    via `EVAL`.
  - `test_rpoplpush_diff_keys_does_not_carry_src_ttl` -- negative
    control: cross-key `RPOPLPUSH src dst` must NOT propagate src's
    TTL onto dst (passes both pre- and post-fix; guards against an
    over-eager fix that special-cased TTL on the destination).
- **Verification:** PASS (4/4 affirmative fail pre-fix with `ttl=-1`,
  all 5 pass post-fix). Direct stash + rebuild verification.
- **Change:** Gated the empty-source removal on `src != dst` in three
  duplicated locations:
  - `Store::lmove_atomic` in `src/store.rs` (this also covers
    `Store::rpoplpush_atomic`, which simply delegates to
    `lmove_atomic`).
  - The Lua `LMOVE` arm in `src/scripting.rs`.
  - The Lua `RPOPLPUSH` arm in `src/scripting.rs`.

  When `src == dst` and the inner `VecDeque` is empty after the pop,
  we leave the `ValueEntry` in place so its `expires_at` survives;
  the immediately-following `entry().or_insert_with(...)` resolves
  to the still-present entry and the push re-fills the inner list.
  The empty-then-refilled inner is invisible to callers because the
  data write lock spans the whole operation.

  **Duplication note:** the Lua `LMOVE` and `RPOPLPUSH` arms duplicate
  the same shape as `Store::lmove_atomic`. Per the review, this fix
  lands in all three places in the same atomic commit. Extracting a
  shared helper is a larger refactor outside this fix's scope and was
  consciously deferred.

### P2-10 -- Check Lua move source before destination type
- **Commit:** `47b792c`
- **Files:** `src/scripting.rs`, `tests/test_lists.py`
- **Tests added (3 affirmative + 1 atomicity guard):**
  - `test_lua_lmove_missing_src_with_string_dst_returns_nil` -- via
    `EVAL`, `LMOVE missing string_dst LEFT RIGHT` must return `nil`
    (pre-fix: WRONGTYPE).
  - `test_lua_rpoplpush_missing_src_with_string_dst_returns_nil` --
    `RPOPLPUSH` mirror.
  - `test_lua_lmove_empty_src_with_string_dst_returns_nil` -- src
    that exists transiently but was drained (LPOP'd) and removed.
  - `test_lua_lmove_nonempty_src_with_string_dst_still_wrongtype` --
    **atomicity guard**: when src DOES have a poppable element, the
    dst type-check must STILL fire BEFORE the pop. Mirrors the
    round-1 P2-02 atomicity test for the direct path. This test
    passes both pre- and post-fix and exists purely to pin ordering.
- **Verification:** PASS (3/3 affirmative fail pre-fix with
  WRONGTYPE, all 4 pass post-fix). Direct stash + rebuild
  verification.
- **Change:** Reordered both Lua arms (LMOVE at
  `src/scripting.rs:2455+` and RPOPLPUSH at `src/scripting.rs:2554+`)
  to validate src state FIRST (read-only, no mutation):
    1. Missing src -> return `RedisValue::Nil`.
    2. Empty list src -> return `RedisValue::Nil`.
    3. Wrongtype src -> return `WRONGTYPE`.
    4. Otherwise (src has at least one element), type-check dst -- still
       BEFORE pop so the dst type-check fires before any mutation.
    5. Pop and push.

  This mirrors the direct path `Store::lmove_atomic` after round-1
  P2-02 and matches real Redis behavior. The data write lock spans
  the whole arm, so dst's type cannot change between the check and
  the push -- atomicity is preserved.

  **Duplication note (same as P2-09):** the LMOVE and RPOPLPUSH Lua
  arms duplicate the same shape; the fix lands in both atomically.

## Skipped

None. All 3 findings were fixed as specified.

## Verification

| Suite                                  | Command                                    | Result                            | vs. Round-1 baseline       |
|----------------------------------------|--------------------------------------------|-----------------------------------|----------------------------|
| Rust unit tests                        | `cargo test --lib`                         | **149 passed, 0 failed**          | unchanged (149)            |
| Rust library compilation               | `cargo check --lib`                        | clean (11 pre-existing warnings)  | unchanged warnings         |
| Python lists tests                     | `pytest tests/test_lists.py -q`            | **122 passed**                    | +11 vs. round-1's 111      |
| Full Python suite                      | `pytest -q --timeout=10`                   | **502 passed, 38 deselected**     | +11 vs. round-1's 491      |

`cargo build` (full binary link) is the well-known PyO3-extension link
error against Python symbols and is unrelated to this change; the
project uses `maturin develop` for Python-extension builds, which
succeeded cleanly for each fix's verification cycle.

**Per-finding test deltas (verified by stash + rebuild + re-run):**

| Finding | Affirmative tests added | Affirmative pre-fix result | Negative/guard tests added | Total commit |
|---------|-------------------------|----------------------------|----------------------------|--------------|
| P2-08   | 2                       | both FAIL (timeout 2s+)    | 0                          | 2/2 added    |
| P2-09   | 4                       | all 4 FAIL (`ttl=-1`)      | 1 (cross-key TTL control)  | 5/5 added    |
| P2-10   | 3                       | all 3 FAIL (WRONGTYPE)     | 1 (atomicity guard)        | 4/4 added    |
| Total   | 9                       | --                         | 2                          | **11**       |

All three findings closed. No tests skipped, no regressions
introduced. The duplicated Lua/direct shape called out in the review
is preserved as parallel fixes (not refactored into a shared helper)
to keep each commit surgical; the duplication is a known structural
pattern across `dispatch_command_inner`'s arms.

---

_Fixed: 2026-04-25_
_Fixer: Claude (gsd-code-fixer, Opus 4.7 1M)_
_Iteration: 1_
