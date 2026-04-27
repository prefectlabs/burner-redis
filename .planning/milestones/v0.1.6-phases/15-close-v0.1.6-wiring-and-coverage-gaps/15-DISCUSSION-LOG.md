# Phase 15: Close v0.1.6 wiring and coverage gaps - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-04-27
**Phase:** 15-close-v0.1.6-wiring-and-coverage-gaps
**Areas discussed:** ISSUE-2 reverify approach, NoScriptError mapping, List persistence test scope, Test-tightening style for evalsha

---

## Pre-discussion finding (codebase scout)

| Audit issue | Audit-claimed status | Live-codebase status (as of 2026-04-27) |
|-------------|---------------------|------------------------------------------|
| ISSUE-1: NoScriptError never raised | Unfixed | **Confirmed unfixed** — `src/lib.rs:1860` still routes through `make_response_error` |
| ISSUE-2: Pipeline zrangestore/zcount no dispatch arm | Unfixed | **Already fixed in commit `de9d259` (2026-04-15)** — arms exist at `src/lib.rs:3110` and `src/lib.rs:3146`; audit referenced stale line numbers (`3823-3824`) |
| ISSUE-3: List persistence has no round-trip test | Unfixed | **Confirmed unfixed** — `test_round_trip_all_types` (`src/persistence.rs:84`) covers str/hash/set/zset/stream/scripts but not list; `tests/test_persistence.py` has no list tests |

This finding shaped the gray-area discussion below.

---

## ISSUE-2 reverify approach

| Option | Description | Selected |
|--------|-------------|----------|
| Regression tests only | Add zrangestore + zcount cases to tests/test_pipeline.py. No src/lib.rs changes — the arms already exist. Update audit doc to note ISSUE-2 was wired in de9d259. | ✓ |
| Tests + source comment | Same, plus brief comment in src/lib.rs cross-referencing audit + Phase 15 verification commit. | |
| Skeptical reaudit | Treat audit as authoritative — re-walk dispatch_pipeline_command end-to-end before adding tests. | |

**User's choice:** Regression tests only
**Notes:** Aligns with the codebase scout finding that arms are already correctly wired. Phase 15 prevents regression and corrects the audit doc rather than redoing already-done work.

---

## NoScriptError mapping mechanism

| Option | Description | Selected |
|--------|-------------|----------|
| Dedicated make_noscript_error() helper | Mirror make_response_error at top of src/lib.rs: try redis.exceptions.NoScriptError → burner_redis.NoScriptError → Exception. Call only from evalsha's Err branch when msg starts with "NOSCRIPT". | ✓ |
| Generalize make_response_error to sniff prefixes | Add NOSCRIPT prefix check inside make_response_error itself for whole-binding-layer coverage. | |
| Inline branch at evalsha site only | Inline if-statement at lib.rs:1860 with no helper. | |

**User's choice:** Dedicated make_noscript_error() helper
**Notes:** Audit's remediation literal text matches this option. Keeps routing logic isolated, mirror-style symmetric with make_response_error, single call site.

## NoScriptError detection layer

| Option | Description | Selected |
|--------|-------------|----------|
| Rust evalsha Err branch | Detect at src/lib.rs:1860 — if msg starts with "NOSCRIPT", call make_noscript_error(msg); else make_response_error(msg). | ✓ |
| Python monkey-patch wrapper | Wrap BurnerRedis.evalsha in __init__.py to catch ResponseError and re-raise NoScriptError on prefix match. | |

**User's choice:** Rust evalsha Err branch
**Notes:** Single source of truth, no per-call try/except overhead, no split exception logic across layers.

---

## List persistence test scope (Rust)

| Option | Description | Selected |
|--------|-------------|----------|
| Extend test_round_trip_all_types | Add list_key alongside existing str/hash/set/zset/stream entries; rpush ordered values; assert lrange returns same order after reload. | ✓ |
| Add separate test_round_trip_list | Keep test_round_trip_all_types untouched; add dedicated #[test] for List-only. | |
| Extend + add one edge case | Extend AND add small extra test for a list edge case (binary value bytes / non-UTF8). | |

**User's choice:** Extend test_round_trip_all_types
**Notes:** Audit remediation says literally "in test_round_trip_all_types". Matches existing 'one comprehensive round-trip' test pattern, keeps test count flat, no precedent break.

## List persistence test scope (Python)

| Option | Description | Selected |
|--------|-------------|----------|
| Single populated list, order preserved | Mirror test_save_and_restore: client1 rpush ['a','b','c'] + save; client2 lrange asserts [b'a',b'b',b'c']. | ✓ |
| Multiple lists + mixed insertion order | Two lists, one lpush + one rpush, both saved, both restored. | |
| Single list + one mutation-after-restore check | Restore the list AND do an lpush/rpop on the restored client. | |

**User's choice:** Single populated list, order preserved
**Notes:** Minimal, audit-aligned, parity with test_save_and_restore. Multi-variant tests deferred to a future test-coverage phase if needed.

---

## Test-tightening style for tests/test_scripting.py

| Option | Description | Selected |
|--------|-------------|----------|
| Tighten in place to NoScriptError | Change existing test_evalsha_unknown_sha_raises to pytest.raises(NoScriptError). | ✓ |
| Tighten in place + add redis.exceptions subclass assertion | Tighten as above AND add second test asserting it's catchable as redis.exceptions.NoScriptError directly. | |
| Keep old test, add new strict test | Keep legacy match='NOSCRIPT' assertion + add new strict-typed test. | |

**User's choice:** Tighten in place to NoScriptError
**Notes:** Audit remediation matches verbatim ("Update test to assert pytest.raises(NoScriptError)"). Single test, no bloat, no double-coverage of the same code path.

---

## Claude's Discretion

- Whether to extend test_pipeline_sorted_set_commands in place vs. add adjacent dedicated tests
- Specific list contents in the Rust round-trip extension (any 3+ ordered byte values)
- Exact wording of make_noscript_error doc comment (mirror make_response_error)
- Whether the audit-doc update is its own task or folded into the NoScriptError task's acceptance_criteria
- Whether to bump Cargo/pyproject version 0.1.5 → 0.1.6 in this phase (default: defer to release-prep step)

## Deferred Ideas (noted for future phases)

- Lua dispatch coverage for ZRANGESTORE/ZCOUNT/XREADGROUP/XAUTOCLAIM/XINFO/XTRIM/XRANGE (consistent with real Redis; documentation-only gap)
- PUBLISH-from-Lua returning 0 subscribers (documented design tradeoff)
- VERIFICATION.md backfill for Phases 1–9, 13 (procedural)
- Phase 13-03 SUMMARY.md (conda-forge staged-recipes PR record)
- Nyquist wave_0_complete: true work for Phases 1, 10, 11, 12, 14
- v0.1.6 release-prep (version bump, PyPI publish, conda-forge feedstock update)
