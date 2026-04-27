# Phase 15: Close v0.1.6 wiring and coverage gaps - Context

**Gathered:** 2026-04-27
**Status:** Ready for planning

<domain>
## Phase Boundary

Close the three minor wiring/coverage gaps surfaced by the v0.1.6 milestone audit (`.planning/v0.1.6-MILESTONE-AUDIT.md`, status `tech_debt`):

1. **ISSUE-1 (NoScriptError):** `EVALSHA` on an unknown SHA must raise `redis.exceptions.NoScriptError` (with fallback to `burner_redis.NoScriptError` when redis is not installed) — currently routed through the generic `make_response_error` and surfaces as `ResponseError`.
2. **ISSUE-2 (Pipeline `zrangestore`/`zcount`):** **Already wired in commit `de9d259` (2026-04-15).** Dispatch arms exist at `src/lib.rs:3110` (`zrangestore`) and `src/lib.rs:3146` (`zcount`); the audit's "no Rust dispatch arm" finding referenced stale line numbers (`src/lib.rs:3823-3824`). Phase 15's job here is regression-test only — confirm the wired path produces correct results through `Pipeline.zrangestore()` / `Pipeline.zcount()`.
3. **ISSUE-3 (List persistence):** `ValueData::List` round-trip is correct in production code (`src/store.rs:3476-3478` per audit / `src/persistence.rs:84` `test_round_trip_all_types`) but has no test coverage on either side. Add Rust unit-test coverage in `test_round_trip_all_types` and a Python `test_list_persistence`.

**Not in scope (deferred to backlog or future phases per audit):**
- Lua dispatch coverage gaps for `ZRANGESTORE`/`ZCOUNT`/`XREADGROUP`/`XAUTOCLAIM`/`XINFO GROUPS|CONSUMERS`/`XTRIM`/`XRANGE` (consistent with real Redis Lua semantics; documentation gap, not behavioral)
- `PUBLISH` from inside Lua returning 0 subscribers (documented design tradeoff at `src/scripting.rs:1854-1857`)
- VERIFICATION.md backfill for Phases 1–9, 13 (procedural)
- Nyquist `wave_0_complete: true` work for Phases 1, 10, 11, 12, 14 (deferred quality work)
- Phase 13-03 SUMMARY.md backfill (conda-forge feedstock confirmation)

</domain>

<decisions>
## Implementation Decisions

### ISSUE-1: NoScriptError Mapping (real fix)
- **D-01:** Add a dedicated `make_noscript_error(msg: String) -> PyErr` helper at the top of `src/lib.rs`, mirroring `make_response_error` (lib.rs:82). Resolution order: `redis.exceptions.NoScriptError` → `burner_redis.NoScriptError` → `pyo3::exceptions::PyException`. The double-fallback matches the existing `make_response_error` pattern verbatim and matches the dual-class shape in `python/burner_redis/__init__.py:26-38`.
- **D-02:** Detection happens **in Rust**, at the `evalsha` `Err` branch (`src/lib.rs:1860`). Replace the unconditional `Err(make_response_error(msg))` with: if `msg.starts_with("NOSCRIPT")`, call `make_noscript_error(msg)`; otherwise call `make_response_error(msg)`. No Python monkey-patch wrapper — keeps the routing single-sourced and avoids an extra try/except on every evalsha call.
- **D-03:** Detection scope is **only** the `evalsha` call site. Do NOT generalize `make_response_error` to sniff the `NOSCRIPT` prefix — couples error class selection to message-string detection across the whole binding layer and creates a slippery slope for future error-prefix routing.
- **D-04:** Tighten `tests/test_scripting.py:83` `test_evalsha_unknown_sha_raises` **in place**: import `NoScriptError` from `burner_redis`, change `pytest.raises(Exception, match="NOSCRIPT")` to `pytest.raises(NoScriptError)`. Audit remediation matches verbatim ("Update test to assert pytest.raises(NoScriptError)"). No new test alongside, no second redis-exceptions-subclass assertion test.

### ISSUE-2: Pipeline zrangestore/zcount (regression-only)
- **D-05:** No `src/lib.rs` source change. Dispatch arms already exist (`zrangestore` at lib.rs:3110, `zcount` at lib.rs:3146; added in commit `de9d259` on 2026-04-15, before the audit ran). Phase 15's role is to verify and prevent regression.
- **D-06:** Add regression test cases to `tests/test_pipeline.py`. The existing `test_pipeline_sorted_set_commands` (line 80) covers `zadd` + `zrange` only; extend it (or add adjacent `test_pipeline_zrangestore` and `test_pipeline_zcount` tests — planner picks the cleaner shape) to assert that:
  - `Pipeline.zrangestore("dest", "src", 0, -1)` returns the count of stored elements and produces the same result as the standalone `BurnerRedis.zrangestore()` pymethod.
  - `Pipeline.zcount("zset", min, max)` returns the count and produces the same result as the standalone `BurnerRedis.zcount()` pymethod.
- **D-07:** Update `.planning/v0.1.6-MILESTONE-AUDIT.md` to note that ISSUE-2 was already wired in `de9d259` and Phase 15 added regression coverage. This avoids the next audit re-flagging the same stale-line-number issue.

### ISSUE-3: List Persistence Coverage (real coverage gap)
- **D-08:** **Rust side:** Extend the existing `test_round_trip_all_types` in `src/persistence.rs:84` to include a `list_key`. Use `rpush` to seed an ordered list (e.g., `["x", "y", "z"]`); after reload, assert `lrange("list_key", 0, -1)` returns the same byte sequence in the same order. Add the assertion alongside the existing string/hash/set/zset/stream/script checks. Do NOT introduce a separate `test_round_trip_list` — extending the comprehensive test matches the established pattern; introducing a List-only test would create a precedent the other variants don't follow.
- **D-09:** **Python side:** Add `test_list_persistence` to `tests/test_persistence.py`, mirroring the existing `test_save_and_restore` pattern: client1 with `persistence_path=tmp_path` does `await r.rpush("list1", "a", "b", "c")` then `await r.save()`; client2 with the same path does `await r.lrange("list1", 0, -1)` and asserts `[b"a", b"b", b"c"]`. Minimal, audit-aligned, single populated list with order preservation. Do NOT add multi-list / mixed-direction-push / mutation-after-restore variants in this phase — those belong in a future test-coverage phase if needed.

### Plan Shape & Re-verification
- **D-10:** Single plan (`15-01-PLAN.md`) is sufficient — the work is small (~30 minutes per audit's recommendation summary), and all three issues touch the same general surface (Rust binding + Python tests). Per ROADMAP "Plans: 0/1 plans". The planner should produce one plan with three logical task groups: (a) NoScriptError implementation + test tightening, (b) pipeline regression tests, (c) List persistence tests (Rust + Python).
- **D-11:** Phase requirement IDs from ROADMAP are **all re-verifications**: D-08 (NoScriptError, originally Phase 12 D-08), ZSET-04, ZSET-06, PERS-01..04, LIST-01..16. The plan must list every one of these REQ-IDs in its `requirements` frontmatter field — the audit explicitly tied each issue back to its origin REQ-IDs and the plan-checker enforces 100% coverage of phase REQ-IDs.

### Claude's Discretion
- Whether to extend `test_pipeline_sorted_set_commands` in place vs. add adjacent `test_pipeline_zrangestore` / `test_pipeline_zcount` (planner picks based on test-file readability).
- Specific list contents in the Rust round-trip extension (any 3+ ordered byte values are acceptable; planner can pick).
- Specific TTL/expiration test variations for the list (none required by audit; default to a non-expiring list).
- Exact wording of the `make_noscript_error` doc comment (mirror `make_response_error`'s comment format).
- Whether to add the audit-doc update (D-07) as a separate task in the plan or fold it into the NoScriptError task's `acceptance_criteria` (cosmetic — planner picks).
- Whether to bump the Cargo / pyproject version to 0.1.6 in this phase or defer to a separate release-prep step (audit notes "current Cargo version: 0.1.5"; ROADMAP doesn't require version bump in Phase 15). Default: **defer** — Phase 15 closes audit gaps; release-prep is a separate concern.

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Phase Scope & Audit
- `.planning/ROADMAP.md` §Phase 15 — Authoritative goal, success criteria, REQ-ID list (D-08, ZSET-04, ZSET-06, PERS-01..04, LIST-01..16 — all re-verification)
- `.planning/v0.1.6-MILESTONE-AUDIT.md` — Source of ISSUE-1, ISSUE-2, ISSUE-3 with evidence + remediation; the plan must update this doc to record ISSUE-2's pre-existing fix (D-07)
- `.planning/REQUIREMENTS.md` — REQ-ID definitions for D-08 (LUA scripting / NoScriptError), ZSET-04/06 (sorted set range/count), PERS-01..04 (persistence), LIST-01..16 (list commands)

### Prior Phase Contexts (pattern origins)
- `.planning/phases/12-close-remaining-redis-py-compatibility-gaps-for-drop-in-repl/12-CONTEXT.md` — Exception class hierarchy with `redis.exceptions.*` subclassing + import-fallback pattern; D-08 origin
- `.planning/phases/14-add-support-for-the-redis-list-data-type-required-commands-l/14-CONTEXT.md` — `ValueData::List(VecDeque<Bytes>)` storage variant, value-coercion model for list pushes, persistence wiring at `src/store.rs:3476-3478`
- `.planning/phases/08-persistence/` (no CONTEXT.md — predates discuss-phase) — Persistence module structure (`src/persistence.rs`, MessagePack format, `save_to_path` / `load_from_path`)

### Codebase Integration Points
- `src/lib.rs:1860` — `evalsha` `Err` branch (the call site that needs NOSCRIPT detection)
- `src/lib.rs:82-96` — `make_response_error` helper (template for new `make_noscript_error`)
- `src/lib.rs:3110-3121` — Existing `zrangestore` pipeline dispatch arm (verify with regression tests, no source change)
- `src/lib.rs:3146-3155` — Existing `zcount` pipeline dispatch arm (same)
- `python/burner_redis/__init__.py:26-38` — `NoScriptError` class with `redis.exceptions.NoScriptError` subclass + import fallback
- `python/burner_redis/pipeline.py:195-201` — `Pipeline.zrangestore`/`zcount`/`zremrangebyscore` stub methods
- `src/persistence.rs:84` — `test_round_trip_all_types` (extend with list_key)
- `src/store.rs:118` — `ValueData` enum (List variant lives here, no change needed)
- `src/store.rs:3470` — `PersistableValueData` enum (round-trip wire format, no change needed)
- `tests/test_scripting.py:83` — `test_evalsha_unknown_sha_raises` (tighten in place)
- `tests/test_pipeline.py:80` — `test_pipeline_sorted_set_commands` (extend or add neighbors)
- `tests/test_persistence.py:18` — `test_save_and_restore` pattern (template for `test_list_persistence`)

### Compatibility References
- `redis.exceptions.NoScriptError` — must subclass for drop-in caller compatibility (e.g., Prefect / pydocket would expect to catch this exact type)

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `make_response_error()` at `src/lib.rs:82-96` — direct template for `make_noscript_error()` (try `redis.exceptions.<Class>`, fall through to `pyo3::exceptions::PyException`)
- `python/burner_redis/__init__.py:26-38` `NoScriptError` class with redis-not-installed fallback — already correctly defined and exported; just unreachable until D-02 wires it in
- `tests/test_persistence.py::test_save_and_restore` (line 18) — direct pattern for the new `test_list_persistence` (instantiate two clients with same `persistence_path`, save+restore, assert read returns the written value)
- `tests/test_pipeline.py::test_pipeline_sorted_set_commands` (line 80) — direct pattern for pipeline regression tests (queue zadd + a zrange-family command, await execute, assert results indexed correctly)
- `src/persistence.rs::test_round_trip_all_types` (line 84) — established multi-variant round-trip test; extending it is strictly additive

### Established Patterns
- Rust→Python error helpers live at the top of `src/lib.rs` and are called from per-command `match result { Err(msg) => Err(<helper>(msg)) }` arms
- `redis.exceptions.<Class>` is dynamically imported at error-construction time (not at module load) so binding compiles even without redis installed
- Round-trip persistence tests use a single comprehensive test for "all variants together" plus separate tests for narrow concerns (missing file, corrupt file, atexit semantics)
- Pipeline regression tests are `r.pipeline()` → queue commands → `await pipe.execute()` → assert against `results[i]`

### Integration Points
- `src/lib.rs` gains one new helper (`make_noscript_error`) and one branched call site (the `evalsha` Err branch). All other lib.rs code paths are untouched.
- `tests/test_scripting.py` has one test tightened (line 83 area) and gains a `from burner_redis import NoScriptError` import.
- `tests/test_pipeline.py` gains either an extended `test_pipeline_sorted_set_commands` body or two new dedicated tests (planner picks).
- `src/persistence.rs::test_round_trip_all_types` gains 1–2 new lines seeding a list and 1–2 new lines asserting the restored list matches.
- `tests/test_persistence.py` gains one new `test_list_persistence` test.
- `.planning/v0.1.6-MILESTONE-AUDIT.md` gains a note that ISSUE-2 was already wired in `de9d259`.

</code_context>

<specifics>
## Specific Ideas

- The audit's `remediation` for ISSUE-1 is the literal implementation plan: "Add make_noscript_error() in src/lib.rs that raises redis.exceptions.NoScriptError (with fallback to burner_redis.NoScriptError) and call it from the evalsha error branch when message starts with 'NOSCRIPT'. Update test to assert pytest.raises(NoScriptError)." — D-01..D-04 follow it verbatim.
- The audit's `remediation` for ISSUE-3 is also literal: "Add a Rust unit test in test_round_trip_all_types and a Python test_list_persistence covering save/load on a populated list." — D-08, D-09 follow it verbatim.
- ISSUE-2 audit text was incorrect on its premise (dispatch arms already existed at the time of the audit). D-05..D-07 reflect this finding and pivot the work to regression-test-only + audit-doc-correction.
- The phase REQ-ID list in ROADMAP is intentionally broad (`PERS-01..04`, `LIST-01..16`) because adding the persistence test exercises the persistence-of-list path that touches every list command's storage representation. The plan's `requirements` field must enumerate every one of these — the plan-checker enforces 100% coverage.

</specifics>

<deferred>
## Deferred Ideas

**To future phases (per v0.1.6-MILESTONE-AUDIT.md tech_debt):**
- Lua dispatch coverage for read-only/non-blocking ZSET commands (`ZRANGESTORE`, `ZCOUNT`) — consistent with Redis Lua semantics, undocumented; not behavioral
- Lua dispatch coverage for read-only/non-blocking stream commands (`XREADGROUP`, `XAUTOCLAIM`, `XINFO GROUPS|CONSUMERS`, `XTRIM`, `XRANGE`) — same rationale
- `PUBLISH` from Lua returning 0 subscribers — documented design tradeoff at `src/scripting.rs:1854-1857`
- VERIFICATION.md backfill for Phases 1–9 and 13
- Phase 13-03 SUMMARY.md (conda-forge staged-recipes PR submission record)
- Nyquist `wave_0_complete: true` work for the 5 phases with `draft` VALIDATION.md (Phases 1, 10, 11, 12, 14)
- v0.1.6 release-prep (Cargo + pyproject version bump from 0.1.5 → 0.1.6; PyPI publish; conda-forge feedstock update) — separate concern from audit-gap closure

**Anti-scope-creep:** Phase 15 closes the three minor audit issues only. Any additional cleanup (Nyquist, VERIFICATION backfill, release-prep) is its own phase.

</deferred>

---

*Phase: 15-close-v0.1.6-wiring-and-coverage-gaps*
*Context gathered: 2026-04-27*
