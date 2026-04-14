# Phase 11: Close redis-py compatibility gaps for pydocket integration - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-04-14
**Phase:** 11-close-redis-py-compatibility-gaps-for-pydocket-integration
**Areas discussed:** Scope boundary, Delayed task race, Validation strategy, Missing commands

---

## Scope Boundary

### What defines 'compatibility gap'?

| Option | Description | Selected |
|--------|-------------|----------|
| Pydocket-only (Recommended) | Only fix what pydocket's test suite and usage patterns require. Tight scope. | ✓ |
| Pydocket + Prefect edge cases | Fix pydocket gaps plus Prefect edge cases (unusual argument combos, error formats). | |
| Broad redis-py surface | Add commonly-used redis-py commands even if pydocket doesn't need them yet. | |

**User's choice:** Pydocket-only
**Notes:** Tight scope — only what pydocket needs.

### Gap source of truth?

| Option | Description | Selected |
|--------|-------------|----------|
| Pydocket test suite (Recommended) | Clone/install pydocket, run its full test suite against BurnerRedis. | |
| Our integration tests only | Expand our 5 pydocket compat tests to cover more scenarios. | |
| Both | Run pydocket's test suite to discover gaps, then add key scenarios to our integration tests. | ✓ |

**User's choice:** Both
**Notes:** Use pydocket's test suite for discovery, add regression tests to ours.

### Triage missing commands?

| Option | Description | Selected |
|--------|-------------|----------|
| Implement all (Recommended) | Whatever pydocket tests require, we add. Full pydocket compat. | ✓ |
| Triage by severity | Fix test failures that block core workflows. Mark edge-case failures as known limitations. | |

**User's choice:** Implement all
**Notes:** No partial passes.

## Delayed Task Race

### Fix approach?

| Option | Description | Selected |
|--------|-------------|----------|
| Fix the root cause (Recommended) | Diagnose why last_delivered_id isn't advancing when XADD called from Lua. Fix Store-level semantics. | ✓ |
| Workaround at Python layer | Add retry/poll mechanism in Python XREADGROUP wrapper. | |
| Debug first, then decide | Reproduce and understand the exact race before committing to a strategy. | |

**User's choice:** Fix the root cause
**Notes:** Semantics must be correct in Rust, no Python workarounds.

### Fix order?

| Option | Description | Selected |
|--------|-------------|----------|
| Inventory first (Recommended) | Run pydocket's test suite first for full gap picture, then fix everything in priority order. | ✓ |
| Fix race first | Fix the known xfail immediately since we understand it. Then inventory remaining gaps. | |

**User's choice:** Inventory first
**Notes:** Avoids rework.

## Validation Strategy

### Definition of 'done'?

| Option | Description | Selected |
|--------|-------------|----------|
| Pydocket tests green (Recommended) | Full test suite passes with zero xfails/skips. Our integration tests also pass. | |
| Core workflows pass | Core workflows work, some edge-case pydocket tests can remain xfail. | |
| 100% + our regression suite | Pydocket tests all green AND comprehensive regression tests added to our suite. | ✓ |

**User's choice:** 100% + our regression suite
**Notes:** Highest bar — both pydocket and our own tests must be comprehensive.

## Missing Commands

### Implementation depth?

| Option | Description | Selected |
|--------|-------------|----------|
| Full redis-py compat (Recommended) | Each new command matches redis-py's full signature and behavior. Drop-in quality. | ✓ |
| Pydocket-sufficient | Implement only argument combinations pydocket actually uses. | |
| Stubs with NotImplementedError | Add method signatures that raise NotImplementedError for unused paths. | |

**User's choice:** Full redis-py compat
**Notes:** Every new command is a proper drop-in, not a stub.

## Claude's Discretion

- Implementation order of individual missing commands (after inventory)
- How to run pydocket's test suite (vendored, subprocess, conftest fixture, etc.)
- Rust-side architecture for new commands (follow existing patterns)

## Deferred Ideas

None — discussion stayed within phase scope.
