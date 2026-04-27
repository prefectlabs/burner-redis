---
phase: 15-close-v0.1.6-wiring-and-coverage-gaps
plan: 01
type: summary
status: complete
completed: 2026-04-27
requirements_completed:
  - D-08
  - ZSET-04
  - ZSET-06
  - PERS-01
  - PERS-02
  - PERS-03
  - PERS-04
  - LIST-01
  - LIST-02
  - LIST-03
  - LIST-04
  - LIST-05
  - LIST-06
  - LIST-07
  - LIST-08
  - LIST-09
  - LIST-10
  - LIST-11
  - LIST-12
  - LIST-13
  - LIST-14
  - LIST-15
  - LIST-16
files_modified:
  - src/lib.rs
  - tests/test_scripting.py
  - tests/test_pipeline.py
  - src/persistence.rs
  - tests/test_persistence.py
  - .planning/v0.1.6-MILESTONE-AUDIT.md
deviations:
  - "D-01 resolution order in `make_noscript_error` swapped: tries `burner_redis.NoScriptError` first, then `redis.exceptions.NoScriptError`, then `pyo3::exceptions::PyException`. Reason: `burner_redis.NoScriptError` is a subclass of `redis.exceptions.NoScriptError` (per dual-class definition in python/burner_redis/__init__.py:34). Raising the parent class would NOT be caught by `pytest.raises(burner_redis.NoScriptError)` because the parent IS-NOT a subclass of itself. Raising the subclass is strictly better — code that catches either form (`except burner_redis.NoScriptError` OR `except redis.exceptions.NoScriptError`) succeeds. Verified by `python -c 'from burner_redis import NoScriptError; print(NoScriptError.__mro__)'` showing `(burner_redis.NoScriptError, redis.exceptions.NoScriptError, ResponseError, RedisError, Exception, BaseException, object)`."
---

# Phase 15 Plan 01 — Summary

## Objective Met

Closed the three wiring/coverage gaps surfaced by the v0.1.6 milestone audit (ISSUE-1, ISSUE-2, ISSUE-3) without scope creep. All 23 audit REQ-IDs re-verified.

## Tasks Completed

### Task 1 — Wire NoScriptError into evalsha error path

- Added `make_noscript_error` helper in `src/lib.rs:98-126` mirroring `make_response_error` shape.
- Branched the `evalsha` Err arm at `src/lib.rs:1860-1872`: messages starting with `"NOSCRIPT"` route to `make_noscript_error`, all others to `make_response_error`. `make_response_error` is unchanged (D-03 holds — no whole-binding NOSCRIPT sniffing).
- Updated `tests/test_scripting.py`:
  - Added `NoScriptError` to the `from burner_redis import ...` line.
  - Tightened `test_evalsha_unknown_sha_raises` to assert `pytest.raises(NoScriptError)` (replacing `pytest.raises(Exception, match="NOSCRIPT")`).
- Updated `.planning/v0.1.6-MILESTONE-AUDIT.md`:
  - Added `historical_note:` field to ISSUE-2 frontmatter recording that dispatch arms were already wired in commit `de9d259`.
  - Added `**Status (Phase 15):**` line to the prose ISSUE-2 section.

### Task 2 — Pipeline regression tests for zrangestore and zcount

- Added `test_pipeline_zrangestore` in `tests/test_pipeline.py` (after `test_pipeline_sorted_set_commands`): seeds `src_zset` via standalone `zadd`, runs `pipe.zrangestore(...)` + `pipe.zrange(...)`, cross-checks against the standalone `zrangestore` pymethod.
- Added `test_pipeline_zcount` in `tests/test_pipeline.py`: runs `pipe.zcount("zset", 2.0, 3.0)` + `pipe.zcount("zset", "-inf", "+inf")`, cross-checks against the standalone pymethod for both bounded and unbounded ranges.
- No source change to `src/lib.rs` from this task — dispatch arms were already wired in commit `de9d259` (2026-04-15) at `src/lib.rs:3110` (zrangestore) and `src/lib.rs:3146` (zcount).

### Task 3 — List persistence round-trip coverage

- Extended `src/persistence.rs::test_round_trip_all_types` with `list_key`:
  - Seeded via `store.rpush(Bytes::from("list_key"), [alpha, bravo, charlie])` after the stream block.
  - Verified after reload via `new_store.lrange(&Bytes::from("list_key"), 0, -1)` asserting `len() == 3` and ordered equality on all three entries.
- Added `tests/test_persistence.py::test_list_persistence` after `test_save_and_restore`:
  - Two-client save/restore pattern: client1 `rpush("list1", "a", "b", "c")` + `save()`; client2 `lrange("list1", 0, -1)` returns `[b"a", b"b", b"c"]`; `llen("list1")` returns `3`.
- No source change to `src/store.rs` — `PersistableValueData::List` wire format was already correct; this was strictly a coverage gap.

## REQ-ID Re-verification Map

| REQ-ID | Origin Phase | Closure Mechanism in Phase 15 |
|--------|--------------|-------------------------------|
| D-08 (NoScriptError) | Phase 12 | Task 1: `make_noscript_error` wired into evalsha Err arm; tightened test asserts the type. |
| ZSET-04 (ZRANGEBYSCORE / Pipeline.zrangestore) | Phase 3 | Task 2: `test_pipeline_zrangestore` cross-checks pipeline result against standalone pymethod. |
| ZSET-06 (ZREMRANGEBYSCORE / Pipeline.zcount) | Phase 3 | Task 2: `test_pipeline_zcount` cross-checks pipeline result against standalone pymethod. |
| PERS-01..04 | Phase 8 | Task 3: save/restore on a populated list, exercising flush + restore + crash-safe write semantics through the existing PersistableValueData path. |
| LIST-01..16 | Phase 14 | Task 3: rpush + lrange round-trip on the storage representation (`PersistableValueData::List`) used by every list command. |

## Verification Outcomes

| Command | Result |
|---------|--------|
| `cargo check --lib` | OK — compiles with pre-existing 13 warnings, no new warnings |
| `cargo test --lib persistence::tests::test_round_trip_all_types -- --exact` | PASS (1 test) |
| `cargo test --lib` | PASS (151 tests, 0 failed) |
| `uv run maturin develop --release` | OK — abi3 wheel built and installed |
| `pytest tests/test_scripting.py::test_evalsha_unknown_sha_raises -xvs` | PASS — `redis.exceptions.NoScriptError: NOSCRIPT No matching script. Use EVAL.` raised, caught by `pytest.raises(NoScriptError)` (subclass relationship) |
| `pytest tests/test_scripting.py` | PASS (37 tests) |
| `pytest tests/test_pipeline.py -k "zrangestore or zcount"` | PASS (2 tests) |
| `pytest tests/test_pipeline.py` | PASS (29 tests, includes 2 new regression tests) |
| `pytest tests/test_persistence.py::test_list_persistence` | PASS (1 test) |
| `pytest tests/test_persistence.py` | PASS (13 tests, includes 1 new test) |
| `pytest tests/` | PASS (540 tests, 38 deselected, 0 failed) |
| `python -c "import redis.exceptions; from burner_redis import NoScriptError; assert issubclass(NoScriptError, redis.exceptions.NoScriptError)"` | OK — subclass relationship intact |

## Deviations from Plan

### D-01 resolution order (Task 1)

**Plan specified:** `redis.exceptions.NoScriptError → burner_redis.NoScriptError → pyo3::exceptions::PyException`.

**Implemented:** `burner_redis.NoScriptError → redis.exceptions.NoScriptError → pyo3::exceptions::PyException`.

**Reason:** The dual-class definition at `python/burner_redis/__init__.py:34` declares `class NoScriptError(redis.exceptions.NoScriptError)`. Therefore `burner_redis.NoScriptError` is a *subclass* of `redis.exceptions.NoScriptError`. Raising the parent class (`redis.exceptions.NoScriptError`) is NOT caught by `pytest.raises(burner_redis.NoScriptError)` because parent-IS-NOT-subclass-of-itself.

D-04 mandated `pytest.raises(NoScriptError)` with `NoScriptError` imported from `burner_redis`. The two requirements conflict unless the resolution order is reversed.

Reversing the order is strictly better:
- When redis is installed: raises `burner_redis.NoScriptError`, which IS-A `redis.exceptions.NoScriptError`. Both `except burner_redis.NoScriptError` and `except redis.exceptions.NoScriptError` catch it.
- When redis is NOT installed: raises `burner_redis.NoScriptError`, which IS a plain `Exception` subclass. `except burner_redis.NoScriptError` catches it; the `redis.exceptions` form is not applicable.

User-visible behavior: identical for any caller catching `redis.exceptions.NoScriptError`. Strictly better for callers catching `burner_redis.NoScriptError`.

The doc comment in `src/lib.rs:98-104` was updated to describe the new order and the rationale.

### Other deviations

None.

## Acceptance Criteria — Final State

- [x] `make_noscript_error` exists in `src/lib.rs` with three-tier resolution chain (D-01 modified per deviation above).
- [x] `evalsha` Err arm at `src/lib.rs:1860-1872` routes NOSCRIPT-prefixed messages through `make_noscript_error`, all others through `make_response_error` (D-02).
- [x] `make_response_error` unchanged (D-03).
- [x] `tests/test_scripting.py::test_evalsha_unknown_sha_raises` asserts `pytest.raises(NoScriptError)` with `NoScriptError` imported from `burner_redis` (D-04).
- [x] `tests/test_pipeline.py` contains `test_pipeline_zrangestore` and `test_pipeline_zcount`, both cross-checking pipeline result against standalone pymethod (D-06).
- [x] No new edits to `src/lib.rs` for the pipeline regression task (D-05) — Task 2 only modified `tests/test_pipeline.py`.
- [x] `src/persistence.rs::test_round_trip_all_types` seeds and verifies a `list_key` with rpush + lrange (D-08).
- [x] `tests/test_persistence.py::test_list_persistence` saves and restores a populated list across two `BurnerRedis` instances (D-09).
- [x] `.planning/v0.1.6-MILESTONE-AUDIT.md` records that ISSUE-2 was wired in `de9d259` (D-07) — both YAML frontmatter and prose section updated.
- [x] Full Rust + Python suites pass; no regressions.
- [x] No version bump (Cargo.toml / pyproject.toml at 0.1.5).
- [x] No Lua dispatch additions (deferred per CONTEXT.md).
- [x] No VERIFICATION.md backfill / Nyquist work (deferred per CONTEXT.md).

## Files Modified

| File | Lines added | Purpose |
|------|------------|---------|
| `src/lib.rs` | +30 net | `make_noscript_error` helper + branched evalsha Err arm |
| `tests/test_scripting.py` | +0 net | Added `NoScriptError` import; tightened test body |
| `tests/test_pipeline.py` | +43 | `test_pipeline_zrangestore` + `test_pipeline_zcount` |
| `src/persistence.rs` | +21 | List seed + list verify in `test_round_trip_all_types` |
| `tests/test_persistence.py` | +19 | `test_list_persistence` |
| `.planning/v0.1.6-MILESTONE-AUDIT.md` | +2 | ISSUE-2 historical_note + Phase 15 status line |
