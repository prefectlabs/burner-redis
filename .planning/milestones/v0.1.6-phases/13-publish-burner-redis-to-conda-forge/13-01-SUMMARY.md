---
phase: 13-publish-burner-redis-to-conda-forge
plan: 01
subsystem: distribution
tags: [conda-forge, pypi, sdist, maturin, cargo-lock, offline-build]

# Dependency graph
requires:
  - phase: 09-distribution
    provides: PyPI package published via maturin (abi3 wheels + sdist)
provides:
  - Verified contract fields for Plan 03 recipe.yaml (pinned_version, sha256, sdist_url)
  - Proof that the PyPI 0.1.2 sdist builds offline against a pre-populated cargo cache
  - Audit record at `.planning/notes/phase-13-sdist-verification-report.md`
affects:
  - 13-02 license audit (runs against the pinned 0.1.2 source tree)
  - 13-03 staged-recipes PR (consumes pinned_version and sha256 directly)

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "sdist feedstock-readiness audit (curl + tar + shasum + grep) captured as a planning note with frontmatter contract fields"
    - "Offline build verification via `CARGO_NET_OFFLINE=true pip install --no-index --no-build-isolation --no-deps <sdist>` against a pre-populated `$CARGO_HOME/registry/cache`"

key-files:
  created:
    - .planning/notes/phase-13-sdist-verification-report.md
  modified: []

key-decisions:
  - "0.1.2 passed feedstock-readiness audit — no pyproject.toml fix, no 0.1.3 release cut"
  - "maturin 1.x ships Cargo.lock in the sdist by default — no explicit `[tool.maturin].include` entry needed"
  - "vendoring Rust deps into the sdist is NOT required for conda-forge; CARGO_NET_OFFLINE+cargo cache is a realistic proxy for the builder environment"

patterns-established:
  - "Verification contract pattern: audit report frontmatter carries `pinned_version` and `sha256` so downstream plans can grep-pin without re-running the audit"

requirements-completed: []

# Metrics
duration: 3min
completed: 2026-04-18
---

# Phase 13 Plan 01: Verify burner-redis PyPI sdist is conda-forge-feedstock-ready Summary

**PyPI burner-redis 0.1.2 sdist ships Cargo.lock and builds end-to-end offline — pinned_version=0.1.2, sha256 locked in for Plan 03 recipe.yaml.**

## Performance

- **Duration:** 3 min
- **Started:** 2026-04-18T03:01:24Z
- **Completed:** 2026-04-18T03:04:14Z
- **Tasks:** 2 executed (Task 1 audit + Task 4 offline build); Tasks 2 and 3 skipped (conditional, 0.1.2 passed)
- **Files modified:** 1 (created)

## Accomplishments

- Pulled `burner_redis-0.1.2.tar.gz` (260KB, 190 entries) from PyPI and verified sha256 matches `189698190835809f73fdb5af9ead4962975181c7fc8297045a5d831c0d465add`.
- Confirmed `Cargo.lock`, `Cargo.toml`, `pyproject.toml`, `src/lib.rs`, `LICENSE`, and `README.md` are all present at the tarball root — the sdist is complete.
- Pre-populated `$CARGO_HOME/registry/cache` via `cargo fetch --locked` (641 crates).
- Built the wheel from the sdist with `CARGO_NET_OFFLINE=true pip install --no-index --no-build-isolation --no-deps burner_redis-0.1.2.tar.gz` — exit 0, produced `burner_redis-0.1.2-cp310-abi3-macosx_11_0_arm64.whl`.
- Smoke-tested: `python -c "import burner_redis; burner_redis.BurnerRedis()"` from the offline-built wheel exits 0.
- Locked in contract fields for Plan 03 via the report frontmatter: `pinned_version`, `sha256`, and `sdist_url` are now the single source of truth for `recipe.yaml`'s `source` block.

## Task Commits

1. **Task 1: Download and inspect the PyPI 0.1.2 sdist** — `59317e7` (docs)
2. **Task 4: Offline build verification against the pinned sdist** — `2c84269` (docs)

**Skipped:**
- **Task 2** (conditional — pyproject.toml fix for Cargo.lock inclusion): skipped because Task 1 showed Cargo.lock already present in the sdist.
- **Task 3** (conditional — human-action checkpoint to cut 0.1.3): skipped because Task 2 did not execute.

**Plan metadata:** _(this SUMMARY and STATE/ROADMAP updates commit — see final commit below)_

## Files Created/Modified

- `.planning/notes/phase-13-sdist-verification-report.md` — Audit record with frontmatter contract fields (`pinned_version: "0.1.2"`, `sha256: "189698190835809f73fdb5af9ead4962975181c7fc8297045a5d831c0d465add"`, `sdist_url: <PyPI URL>`) plus full command transcripts for Step 1.1-1.3 (sdist audit) and Step 4.1-4.3 (offline build + import smoke test). Decision line: `PASS — pinned_version = 0.1.2; proceed to Plan 02 (license audit).`

## Decisions Made

- **Pinned version = 0.1.2.** The 0.1.2 sdist passed the feedstock-readiness audit on the first run (Cargo.lock present, offline build succeeds, import smoke test succeeds). No 0.1.3 release cut. This saves one CI cycle and preserves the existing version history.
- **No vendor/ directory required.** The research note's original concern (air-gapped builders) was defensive; conda-forge builders have crates.io reachability in practice, and `CARGO_NET_OFFLINE=true` + pre-populated cache is a strictly harder test than what the feedstock CI will actually face. Skipping `cargo vendor` keeps the sdist small (~260KB) and the release workflow simple.
- **No pyproject.toml change to `[tool.maturin]`.** maturin 1.x already includes Cargo.lock in sdists by default, as observed directly in the tarball. Adding an explicit `include = [{ path = "Cargo.lock", format = "sdist" }]` would be redundant.

## Deviations from Plan

None — plan executed as written. The plan's CONDITIONAL flow (Tasks 2 and 3 gated on Task 1's outcome) triggered the "happy path": 0.1.2 sdist passed audit, so Tasks 2 and 3 were correctly skipped per the `<done>` clause on Task 1.

## Issues Encountered

None. All command exits were 0 on first run.

## User Setup Required

None — this plan is a verification-only gate; no external services or environment variables are introduced.

## Next Plan Readiness

**Plan 02 (Rust dependency license audit with `cargo-bundle-licenses`) is unblocked.**

Plan 02 runs against the pinned 0.1.2 source tree (already on `main` at tag `v0.1.2`). Plan 03 (recipe drafting + staged-recipes PR) consumes these two frontmatter fields from `.planning/notes/phase-13-sdist-verification-report.md`:

- `pinned_version: "0.1.2"` → `recipe.yaml` `package.version:`
- `sha256: "189698190835809f73fdb5af9ead4962975181c7fc8297045a5d831c0d465add"` → `recipe.yaml` `source.sha256:`
- `sdist_url: "https://files.pythonhosted.org/packages/a4/30/8b219fc8863c652ef294d9a6075752cf14eade2f050e956410873f6f0270/burner_redis-0.1.2.tar.gz"` → `recipe.yaml` `source.url:`

No blockers for Plan 02. The CONTEXT.md hard gate ("staged-recipes PR is NOT yet touched") is respected — we've only inspected PyPI and built locally.

## Self-Check: PASSED

- `.planning/notes/phase-13-sdist-verification-report.md` — FOUND
- `.planning/phases/13-publish-burner-redis-to-conda-forge/13-01-SUMMARY.md` — FOUND
- Commit `59317e7` (Task 1) — FOUND
- Commit `2c84269` (Task 4) — FOUND
- Report frontmatter `pinned_version: "0.1.2"` — present
- Report frontmatter `sha256: "189698190835809f73fdb5af9ead4962975181c7fc8297045a5d831c0d465add"` — present (64-char lowercase hex)

