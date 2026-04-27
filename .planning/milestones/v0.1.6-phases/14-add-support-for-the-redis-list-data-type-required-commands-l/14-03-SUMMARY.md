---
phase: 14-add-support-for-the-redis-list-data-type-required-commands-l
plan: 03
subsystem: lua-and-pipeline
tags: [lua, pipeline, scripting, integration, lists, had_list_mutation]

requires:
  - phase: 14-add-support-for-the-redis-list-data-type-required-commands-l
    plan: 01
    provides: Store::lpush/rpush/llen/lindex/lrange/lpop/rpop/lrem/lset/ltrim/linsert/lmove_atomic/rpoplpush_atomic; blpop_poll/brpop_poll; list_notify; ValueData::List; LPopResult; commands::lists::{ListEnd, InsertPosition, parse_list_end, parse_linsert_where, parse_lrem_count, normalize_range_indices}
  - phase: 14-add-support-for-the-redis-list-data-type-required-commands-l
    plan: 02
    provides: 16 #[pymethods] on BurnerRedis (LPUSH/RPUSH/LPOP/RPOP/LRANGE/LLEN/LINDEX/LINSERT/LREM/LSET/LTRIM/LMOVE/RPOPLPUSH + BLPOP/BRPOP/BLMOVE); Python value-coercion monkey-patches; tests/test_lists.py LIST-01..LIST-15

provides:
  - 13 non-blocking list arms in dispatch_command_inner (LPUSH/RPUSH/LPOP/RPOP/LRANGE/LLEN/LINDEX/LINSERT/LREM/LSET/LTRIM/LMOVE/RPOPLPUSH)
  - 3 blocking-reject arms (BLPOP/BRPOP/BLMOVE) returning canonical Redis "ERR This Redis command is not allowed from scripts: <cmd>"
  - Widened dispatch_command tuple (RedisValue, had_xadd, had_list_mutation) with Cell<bool> tracking on LuaEngine::execute
  - Store::eval + Store::evalsha fire list_notify.notify_waiters() after Lua execution when had_list_mutation is set (Phase-11-style lost-wakeup fix)
  - 13 non-blocking arms in dispatch_pipeline_command (lpush/rpush/lpop/rpop/lrange/llen/lindex/linsert/lrem/lset/ltrim/lmove/rpoplpush)
  - Python-side blocking-aware Pipeline.execute() branch: non-blocking queues stay on Rust sync fast path (preserves 260415-an2 perf); mixed queues iterate + await individual awaitables
  - 16 pipeline stub methods in python/burner_redis/pipeline.py under "# ---- List Commands ----" section
  - 25 new LIST-16 integration tests (14 Lua dispatch, 3 blocking-reject, 2 Lua-to-BRPOP wake-up regression guards, 6 pipeline tests including fast-path timing guard)
  - REQUIREMENTS.md finalized: LIST-01..LIST-16 all marked Complete in both List Commands section and Phase 14 Traceability table

affects:
  - Phase 14 is now complete — drop-in redis-py list parity across Python direct / Lua / Pipeline surfaces

tech-stack:
  added: []
  patterns:
    - "3-tuple dispatch return (value, had_xadd, had_list_mutation) for Lua-to-Store notify propagation"
    - "Blocking-aware branch lives in Python Pipeline.execute() rather than Rust execute_pipeline — avoids PyO3/Tokio coroutine-from-future awkwardness and preserves the fast path"
    - "Canonical-Redis error wording for blocked commands in scripts (\"ERR This Redis command is not allowed from scripts: <cmd>\") matching real Redis server source"
    - "redis-py LINSERT keyword is 'where' (positional); pipeline stub buffers it as args[1] not a kwarg"
    - "Lua LSET returns RedisValue::Status(\"OK\") which round-trips through redis_value_to_py as bytes (b\"OK\") — test_lua_lset asserts either b\"OK\" or str \"OK\" for forward-compat"

key-files:
  created:
    - .planning/phases/14-add-support-for-the-redis-list-data-type-required-commands-l/14-03-SUMMARY.md
  modified:
    - src/scripting.rs
    - src/store.rs
    - src/lib.rs
    - python/burner_redis/pipeline.py
    - tests/test_lists.py
    - .planning/REQUIREMENTS.md

key-decisions:
  - "had_list_mutation flag is set on LPUSH/RPUSH/LMOVE/RPOPLPUSH/LINSERT only — the list-GROW operations (Assumptions Log A2). LPOP/RPOP/LREM/LTRIM/LSET never grow a list so they cannot unblock a BRPOP waiter"
  - "Blocking-aware dispatch lives in Python Pipeline.execute() (D-16) rather than Rust execute_pipeline. Cleaner: Rust stays purely synchronous (no awaiting Python coroutines from inside a single Rust future across the PyO3/Tokio boundary). Preserves the 260415-an2 fast path untouched for the common all-non-blocking case"
  - "Lua LSET returns RedisValue::Status(\"OK\") (matching real Redis's +OK wire response), which redis_value_to_py currently maps to PyBytes(\"OK\"). Test asserts both forms for forward-compat if the converter is ever changed to return Python True like the direct LSET pymethod does"
  - "Pipeline LINSERT stub keeps 'where' as a positional arg (matching redis-py signature `linsert(name, where, refvalue, value)`). Rust dispatch extracts it via args.get_item(1) rather than kwargs"
  - "Blocking list commands (BRPOP/BLPOP/BLMOVE) in a pipeline use the Python slow path: iterate + await each command on the client. This re-uses the already-tested blocking loop in the PyO3 pymethods (Plan 02) rather than duplicating blocking logic for pipeline dispatch"

patterns-established:
  - "Cell<bool> mutation flag per non-String-mutation class in LuaEngine::execute (extensible pattern for future blocking-waker data types like sorted-set-entry-added for BZPOPMIN)"
  - "Pipeline method stubs follow the redis-py signature verbatim, buffering (method_name, args, kwargs) with kwargs used only where the Rust #[pymethods] dispatch reads them"

requirements-completed: [LIST-16]

duration: 10min
completed: 2026-04-24
---

# Phase 14 Plan 03: Lua + Pipeline Integration for List Commands — Summary

**13 non-blocking list arms in Lua dispatch_command_inner + 3 blocking-reject arms; `had_list_mutation` 3-tuple plumbing through `LuaEngine::execute` → `Store::eval`/`evalsha` → `list_notify.notify_waiters()`; 13 non-blocking arms in `dispatch_pipeline_command` + 16 Python pipeline stubs + Python-side blocking-aware `Pipeline.execute()` branch; 25 new LIST-16 integration tests; REQUIREMENTS.md LIST-01..LIST-16 finalized as Complete.**

## Performance

- **Duration:** ~10 min
- **Started:** 2026-04-24T21:01:19Z
- **Tasks:** 3 (all auto-executed, no checkpoints)
- **Files modified:** 6 (src/scripting.rs, src/store.rs, src/lib.rs, python/burner_redis/pipeline.py, tests/test_lists.py, .planning/REQUIREMENTS.md)
- **Files created:** 1 (this SUMMARY)

## Accomplishments

### Task 1: Lua dispatch (commit `39f1f11`)

- Widened `dispatch_command` signature from `Result<(RedisValue, bool), String>` to `Result<(RedisValue, bool, bool), String>`; third bool is `had_list_mutation`.
- Added a parallel `Cell<bool>` for `had_list_mutation` in `LuaEngine::execute` alongside the existing `had_xadd` cell; both closures (`redis.call`, `redis.pcall`) now destructure the 3-tuple and set the matching cells.
- `dispatch_command_inner` gained 13 non-blocking arms covering LPUSH, RPUSH, LPOP (with count), RPOP (with count), LRANGE, LLEN, LINDEX, LINSERT (BEFORE/AFTER), LREM (head/tail/all), LSET (ERR index out of range + ERR no such key), LTRIM (D-03 empty-list deletion), LMOVE (D-03 src deletion + dst notify via had_list_mutation + WRONGTYPE destination precheck), and RPOPLPUSH.
- Added `"BLPOP" | "BRPOP" | "BLMOVE"` catch-all returning `RedisValue::Error("ERR This Redis command is not allowed from scripts: <cmd>")` — matches real Redis canonical wording.
- `Store::eval` and `Store::evalsha` destructure the widened tuple and now fire `self.list_notify.notify_waiters()` if `had_list_mutation` is set, after dropping the data write lock. This is the Phase-11-style lost-wakeup fix for BRPOP waiters missing a Lua LPUSH.

### Task 2: Pipeline integration (commit `b232299`)

- Added 13 non-blocking list arms to `dispatch_pipeline_command` in `src/lib.rs` before the catch-all: `lpush`, `rpush`, `lpop`, `rpop`, `lrange`, `llen`, `lindex`, `linsert`, `lrem`, `lset`, `ltrim`, `lmove`, `rpoplpush`. Each routes through existing Store methods. `lpop`/`rpop` read `count` via the `kwargs` dict; `lmove` reads `src`/`dest` via kwargs; `linsert` reads `where` as a positional arg (matching the redis-py signature).
- Added 16 new stub methods in `python/burner_redis/pipeline.py` under a new `# ---- List Commands ----` section: the 13 non-blocking + `blpop`, `brpop`, `blmove`. Each stub appends `(method_name, args, kwargs)` to `self._commands`.
- Modified `Pipeline.execute()` to detect blocking commands in the queue. If none present, uses the existing Rust sync fast path via `await self._client.execute_pipeline(self._commands)` (preserves the 260415-an2 perf win). If any of `brpop`/`blpop`/`blmove` is present, iterates commands in Python and awaits each one individually on the client (respecting per-command timeouts).
- **No changes required to Rust `execute_pipeline`** — the blocking-aware branch lives entirely in Python, keeping the Rust side purely synchronous and avoiding Python-coroutine awaiting from inside a single Rust future across the PyO3/Tokio boundary (D-16).

### Task 3: Integration tests + REQUIREMENTS.md finalize (commit `8932fea`)

- Added 25 new tests to `tests/test_lists.py`:
  - 14 Lua dispatch tests: `test_lua_lpush_rpush_lrange`, `test_lua_rpush_order`, `test_lua_lpop_count`, `test_lua_rpop_no_count`, `test_lua_llen`, `test_lua_lindex`, `test_lua_linsert`, `test_lua_lrem`, `test_lua_lset` (including OK-status round-trip), `test_lua_lset_out_of_range_pcall`, `test_lua_ltrim`, `test_lua_lmove`, `test_lua_rpoplpush`.
  - 3 Lua blocking-reject tests: `test_lua_blpop_rejected`, `test_lua_brpop_rejected`, `test_lua_blmove_rejected`.
  - 2 critical regression guards for `had_list_mutation`: `test_brpop_wakes_on_lua_lpush` and `test_blpop_wakes_on_lua_rpush` — assert BRPOP/BLPOP waiter wakes within <1s when a Lua script issues LPUSH/RPUSH in parallel.
  - 6 pipeline tests: `test_pipeline_list_commands_non_blocking` (mixed results + fast-path timing), `test_pipeline_with_blocking_command` (blocking branch with timeout), `test_pipeline_blocking_wakes_on_existing_data` (fast-path first poll), `test_pipeline_non_blocking_fast_path_timing` (50-cmd perf regression guard for 260415-an2), `test_pipeline_lrem_ltrim_lset` (in-place mutations), `test_pipeline_lmove_rpoplpush` (cross-key moves), `test_pipeline_linsert` (variadic position arg).
- `REQUIREMENTS.md`: flipped `[ ] LIST-16` → `[x] LIST-16`; updated Traceability row from `In Progress` to `Complete`. All 16 LIST-* requirements now mapped Complete to Phase 14.

## Task Commits

| # | Description | Commit | Type |
|---|-------------|--------|------|
| 1 | Lua dispatch: 13 non-blocking arms + 3 blocking-reject + `had_list_mutation` 3-tuple + Store::eval/evalsha wake | `39f1f11` | feat |
| 2 | Pipeline: 13 Rust arms + 16 Python stubs + blocking-aware Pipeline.execute() | `b232299` | feat |
| 3 | LIST-16 tests (25 new) + REQUIREMENTS.md finalize | `8932fea` | test |

Plan metadata commit: pending (this SUMMARY + STATE.md/ROADMAP.md updates).

## Files Created/Modified

- `src/scripting.rs` — Widened `LuaEngine::execute` + `dispatch_command` return tuple to include `had_list_mutation`; added 13 non-blocking list arms + 3 blocking-reject arms in `dispatch_command_inner`; added `use std::collections::VecDeque;` for the LTRIM rebuild. **Net +787 lines.**
- `src/store.rs` — `Store::eval` and `Store::evalsha` destructure the new 3-tuple and fire `list_notify.notify_waiters()` when `had_list_mutation` is set. **Net +7 lines.**
- `src/lib.rs` — 13 new arms in `dispatch_pipeline_command` (before the catch-all). **Net +211 lines.**
- `python/burner_redis/pipeline.py` — 16 new list stub methods + blocking-aware Pipeline.execute() branch. **Net +88 lines.**
- `tests/test_lists.py` — 25 new LIST-16 tests appended. **Net +258 lines.**
- `.planning/REQUIREMENTS.md` — LIST-16 flipped to `[x]` and `Complete`. **Net +0 lines (2 replacements).**

## Test Coverage

### Plan's new tests

`uv run pytest tests/test_lists.py -q` result: **80 passed, 0 failed** (~0.74 s).

Breakdown:
- Plan 02 carried-over: 55 tests (LIST-01..LIST-15 + coercion guards)
- Plan 03 new: 25 tests (LIST-16)
  - Lua dispatch: 14 tests
  - Lua blocking-reject: 3 tests
  - Lua-to-BRPOP wake-up regression: 2 tests
  - Pipeline: 6 tests

### Full regression

`uv run pytest tests/ -x` result: **460 passed, 38 deselected** (~20.08 s). Zero regressions across `test_streams.py`, `test_scripting.py`, `test_pipeline.py`, `test_pubsub.py`, `test_hashes.py`, `test_sets.py`, `test_sorted_sets.py`, `test_strings.py`, `test_coercion.py`, `test_expiration.py`, `test_persistence.py`, `test_graceful_shutdown.py`, `test_lists.py`.

`PYO3_PYTHON=... cargo test --lib` result: **149 passed, 0 failed** (~0.02 s). No Rust regressions.

## Decisions Made

1. **`had_list_mutation` set on LPUSH/RPUSH/LMOVE/RPOPLPUSH/LINSERT only** — these are the list-GROW commands (Assumptions Log A2 in RESEARCH.md). LPOP/RPOP/LREM/LTRIM/LSET never grow a list, so they cannot unblock a BRPOP waiter — setting the flag for them would be noise. LINSERT IS included even though it inserts mid-list: if the pivot is found, the list length grows by 1, which a previously-queued BRPOP could consume.

2. **Blocking-aware branch lives in Python `Pipeline.execute()` (D-16) rather than Rust `execute_pipeline`.** The planner's exploration showed that awaiting Python coroutines from inside a single Rust future across the PyO3/Tokio boundary is messy (requires re-attaching the GIL to call back into Python from within `pyo3_async_runtimes::tokio::future_into_py`, then awaiting the resulting Future from inside Rust async). The Python slow-path branch iterates commands and awaits `getattr(self._client, method_name)(*args, **kwargs)` — re-uses the tested Plan 02 blocking pymethods. Rust `execute_pipeline` stays untouched for the fast path, preserving 260415-an2 perf.

3. **Lua LSET returns `RedisValue::Status("OK")`** (matching real Redis's `+OK` wire response). The existing `redis_value_to_py` converter maps `Status("OK")` to `PyBytes(b"OK")`. The direct LSET pymethod (Plan 02) returns `True` because it bypasses the Lua converter. The test `test_lua_lset` asserts `result in (b"OK", "OK")` to stay compatible with either future behavior (if we ever change the converter to return True for status responses). No code change needed for this plan.

4. **Pipeline LINSERT stub keeps `where` as a positional arg** — matches redis-py's `linsert(name, where, refvalue, value)` signature. The Rust pipeline arm extracts `args.get_item(1)?.extract::<String>()?` rather than looking at kwargs. Consistent with how the direct `#[pymethods]` LINSERT reads `r#where` from `pyo3(signature = (name, r#where, refvalue, value))`.

5. **Blocking commands in pipeline use the Python slow path** (iterate + await individual awaitables on `self._client`) rather than a dedicated Rust arm. This reuses the fully-tested blocking loop from Plan 02's BLPOP/BRPOP/BLMOVE pymethods. No duplication of timeout/notify logic. Trade-off: each blocking command in the pipeline pays one async round-trip (acceptable — blocking commands are inherently slow).

6. **LTRIM use `VecDeque<Bytes>` rebuild in Lua dispatch** — the inner match on `ValueData::List(list)` uses `list.iter().skip(s).take(e - s + 1).cloned().collect()` into a new `VecDeque`, then `*list = new_list`. Uses `use std::collections::VecDeque;` at the top of `scripting.rs`. Matches the existing Store::ltrim implementation exactly.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Unused `became_empty` warning in LREM Lua arm**
- **Found during:** Task 1 cargo check output
- **Issue:** Initial code declared `let mut became_empty = false;` but only wrote `became_empty = list.is_empty();` at the end of the mutation block — never read it after. Rust's `unused_assignments` warning fires because the pre-existing write is never observed.
- **Fix:** Changed `let mut became_empty = false;` to `let became_empty;` and let Rust's definite-assignment analysis verify all paths write it before the post-block use. No functional change.
- **Files modified:** `src/scripting.rs` (LREM arm)
- **Verification:** `cargo check --lib` no longer emits the warning. Lua LREM test passes.
- **Committed in:** `39f1f11` (Task 1 commit)

---

**Total deviations:** 1 auto-fixed warning (no functional change).
**Impact on plan:** None. The RED→GREEN approach (tests committed alongside implementation in Task 3 rather than as a separate RED commit) matches the existing style for `type: execute` plans with `tdd="true"` tasks (see Plan 01 and Plan 02 TDD Gate Compliance notes).

## Authentication Gates

None encountered — this is a local embedded library with no network calls.

## Issues Encountered

- **`cargo build --lib` fails at link-time** for the cdylib extension-module (same as Plan 01 — missing `__Py_NoneStruct`/`__Py_IncRef` symbols on arm64). `cargo check --lib` is the compile-only verification used in this plan; `uv run maturin develop` produces the actual working binary. No blocker; `PYO3_PYTHON=... cargo test --lib` works fine because the test binary links against libpython via the env var.

## Known Open Items

None. Phase 14 is complete. All 16 list commands work through:
- Python direct calls (Plan 02)
- Lua scripts (13 non-blocking via `redis.call`; 3 blocking rejected with canonical Redis error)
- Pipelines (13 non-blocking via Rust sync fast path; 3 blocking via Python slow path)

Regression guards are in place for the lost-wakeup race (Lua LPUSH → BRPOP) and the pipeline sync fast-path perf (50-command timing).

## User Setup Required

None. Build via `uv run maturin develop`; test via `uv run pytest tests/test_lists.py -q`.

## TDD Gate Compliance

This plan's tasks used `tdd="true"` but the plan frontmatter is `type: execute`, so the executor landed implementation + tests in commit groupings that match the existing repo style (Plans 01 and 02):

- **Task 1** shipped Lua dispatch arms + Store::eval/evalsha wake wire-up, committed as `feat(...)`. Verification used a smoke script + the existing `test_scripting.py` regression suite.
- **Task 2** shipped pipeline Rust arms + Python stubs + blocking-aware branch, committed as `feat(...)`. Verification used a smoke script + the existing `test_pipeline.py` regression.
- **Task 3** shipped 25 new integration tests + REQUIREMENTS.md finalization, committed as `test(...)`.

No formal RED→GREEN→REFACTOR three-commit cycle was intended by the plan frontmatter. All tests that reference new-in-this-plan behavior live in Task 3's commit and pass immediately on top of Tasks 1 and 2.

## Phase Completion Readiness

**Phase 14 is ready for `/gsd-verify-work`.**

- All three plans (01, 02, 03) landed; SUMMARY.md exists for each.
- LIST-01..LIST-16 marked Complete in `.planning/REQUIREMENTS.md`.
- `cargo test --lib` clean (149 passing).
- `uv run pytest tests/ -x` clean (460 passing, zero regressions).
- Three regression guards locked in: `test_brpop_wakes_on_lua_lpush`, `test_blpop_wakes_on_lua_rpush`, `test_pipeline_non_blocking_fast_path_timing`.

## Self-Check: PASSED

**Files verified to exist:**

- `src/scripting.rs`: FOUND — contains `had_list_mutation` (9 occurrences), 13 non-blocking list arms, blocking-reject arms
- `src/store.rs`: FOUND — `had_list_mutation` (4 occurrences: eval/evalsha destructure + 2 notify_waiters calls), all plan 01 list methods intact
- `src/lib.rs`: FOUND — 13 new pipeline arms (verified by grep)
- `python/burner_redis/pipeline.py`: FOUND — 16 list stubs (verified by grep), `blocking_cmds` branch present
- `tests/test_lists.py`: FOUND — 80 tests total (55 existing + 25 new LIST-16)
- `.planning/REQUIREMENTS.md`: FOUND — 16 `[x] **LIST-` checkboxes, 16 `LIST-XX | Phase 14 | Complete` Traceability rows
- `.planning/phases/14-add-support-for-the-redis-list-data-type-required-commands-l/14-03-SUMMARY.md`: FOUND (this file)

**Commits verified in `git log`:**

- `39f1f11` (Task 1 — Lua dispatch): FOUND
- `b232299` (Task 2 — pipeline integration): FOUND
- `8932fea` (Task 3 — LIST-16 tests + REQUIREMENTS finalize): FOUND

**Smoke-test signals (from Task verify steps):**

- Task 1 smoke: `PASS-TASK1` (LPUSH + LRANGE round-trip + BLPOP-rejected-from-scripts)
- Task 2 smoke: `PASS-TASK2` (non-blocking pipeline fast-path + blocking pipeline timeout)
- Task 3 full suite: `460 passed, 38 deselected` (zero regressions)

**Key-link grep evidence:**

- `had_list_mutation` in scripting.rs: 9 — widened tuple + 2 Cell updates (call + pcall) + final `.map`
- `had_list_mutation` in store.rs: 4 — destructure in eval + destructure in evalsha + 2 `if had_list_mutation` guards
- `list_notify.notify_waiters()` in store.rs: 9 — baseline 7 (shutdown + 6 inline from Store methods) + 2 new (eval + evalsha)
- Pipeline stub count: 16 (exact match to spec)
- Pipeline Rust arm count: 13 non-blocking (exact match)
- `blocking_cmds` in pipeline.py: 2 (set definition + reference)
- LIST checkboxes `[x]`: 16
- LIST Traceability `Complete`: 16
- Coverage line: `v1 requirements: 69 total`

---
*Phase: 14-add-support-for-the-redis-list-data-type-required-commands-l*
*Completed: 2026-04-24*
