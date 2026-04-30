# T02: Audit the Rust dependency tree with `cargo-bundle-licenses` and commit `THIRDPARTY.

**Slice:** S13 — **Milestone:** M001

## Legacy Summary

---
phase: 13-publish-burner-redis-to-conda-forge
plan: 02
subsystem: distribution
tags: [conda-forge, cargo-bundle-licenses, license-audit, third-party, rust-deps]

# Dependency graph
requires:
  - phase: 13-publish-burner-redis-to-conda-forge
    plan: 01
    provides: "Pinned source (pinned_version=0.1.2, sha256=189698...5add) for conda-forge recipe"
provides:
  - "THIRDPARTY.yml committed at repo root (vendored license text for all 57 transitive Rust crates)"
  - "Clean-license gate passed — all SPDX IDs in the permissive set (MIT, Apache-2.0, Unlicense, Unicode-3.0, Apache-2.0 WITH LLVM-exception); no GPL/AGPL/MPL/proprietary"
  - ".planning/notes/phase-13-license-audit.md audit record (tool version, invocation, license distribution, warnings, decision)"
affects:
  - 13-03 staged-recipes PR (recipe.yaml `about.license_file` can now reference THIRDPARTY.yml; the conda-forge `cargo-bundle-licenses` build step will succeed against the same dep tree we just audited)

# Tech tracking
tech-stack:
  added:
    - "cargo-bundle-licenses 4.0.0 (dev tool; pinned from 4.2.0 due to rustc 1.85 MSRV)"
  patterns:
    - "License audit pattern: invoke `cargo bundle-licenses --format yaml --output THIRDPARTY.yml` with exit-code + grep-based license-class scan; capture output + tool version in a planning note frontmatter for traceability"

key-files:
  created:
    - THIRDPARTY.yml
    - .planning/notes/phase-13-license-audit.md
    - .planning/phases/13-publish-burner-redis-to-conda-forge/13-02-SUMMARY.md
  modified: []

key-decisions:
  - "Pinned cargo-bundle-licenses to 4.0.0 — latest 4.2.0 requires rustc 1.86 (via cargo_metadata 0.23), our floor is 1.85 (edition 2024 MSRV). 4.0.0's YAML schema uses `package_name:` instead of `- name:`; same structural information, just different field name."
  - "Documented `mlua-sys 0.6.8 text: NOT FOUND` as cosmetic (not a blocker) — SPDX ID is cleanly `MIT` in Cargo.toml; the LICENSE text lives at the mlua workspace repo root, not in the subcrate dir. Standard Rust-workspace packaging quirk. conda-forge will see the same warning and accept it."
  - "No dep upgrades or swaps required — all 57 bundled crates fall in the permissive license set on first run. Task 2 (remediation) correctly skipped."

patterns-established:
  - "Cargo-bundle-licenses schema evolution awareness: plan verification patterns (`^- name:` vs `^- package_name:`) must be checked against the installed tool version. Document in audit note so downstream plans can adapt."

requirements-completed: []

# Metrics
duration: 3min
completed: 2026-04-18
---

# Phase 13 Plan 02: Rust Dependency License Audit Summary

**cargo-bundle-licenses 4.0.0 audit of 57 transitive Rust crates — all permissive licenses (MIT / Apache-2.0 / Unlicense / Unicode-3.0), THIRDPARTY.yml committed, conda-forge submission gate cleared.**

## Performance

- **Duration:** ~3 min (3:20)
- **Started:** 2026-04-18T03:07:29Z
- **Completed:** 2026-04-18T03:10:49Z
- **Tasks:** 1 executed (Task 1); Task 2 skipped (conditional — no deps flagged)
- **Files modified:** 2 (created): THIRDPARTY.yml, .planning/notes/phase-13-license-audit.md

## Accomplishments

- Installed `cargo-bundle-licenses cargo:4.0.0` (pinned one minor below latest due to rustc MSRV gap — see Deviations).
- Ran `cargo bundle-licenses --format yaml --output THIRDPARTY.yml` — exit 0 on first invocation; 6,795 lines of YAML covering 57 third-party crates.
- Verified all 11 direct dependencies from `Cargo.toml [dependencies]` are represented: `pyo3`, `pyo3-async-runtimes`, `tokio`, `parking_lot`, `bytes`, `thiserror`, `ordered-float`, `mlua`, `sha1`, `serde`, `rmp-serde`.
- Confirmed license-class distribution: 39 `MIT OR Apache-2.0`, 11 `MIT`, 3 `Apache-2.0 OR MIT`, 1 `Unlicense OR MIT`, 1 `Apache-2.0 WITH LLVM-exception`, 1 `Apache-2.0`, 1 `(MIT OR Apache-2.0) AND Unicode-3.0` — 7 distinct SPDX expressions, every one in the permissive set accepted by conda-forge without approval.
- Ran the disallowed-license scan `grep -E '^\s*license:\s*.*(GPL|AGPL|NOT FOUND|AMBIGUOUS|UNKNOWN|Proprietary)'` — zero matches. No copyleft, no proprietary, no ambiguous SPDX IDs.
- Documented the one cosmetic `text: NOT FOUND` entry (mlua-sys 0.6.8) with upstream proof that its `license = "MIT"` Cargo.toml declaration is clean; the missing text is a workspace-layout tool quirk that conda-forge reviewers will recognize.
- Wrote audit record `.planning/notes/phase-13-license-audit.md` with `result: "PASS"` frontmatter and the complete invocation transcript for conda-forge reviewer cross-checking.

## Task Commits

1. **Task 1: Install cargo-bundle-licenses and generate THIRDPARTY.yml** — `6f017ce` (docs)

**Skipped:**
- **Task 2** (conditional — remediate ambiguous/disallowed licenses): skipped per the plan's "Skip this task if Task 1 passed" guidance. All deps passed the Step 1.3 (direct-dep coverage) and Step 1.4 (disallowed-license scan) checks on first invocation.

**Plan metadata:** _(this SUMMARY and STATE/ROADMAP updates commit — see final commit below)_

## Files Created/Modified

- `THIRDPARTY.yml` — Vendored third-party Rust license text (57 crates, 6,795 lines of YAML). Schema: `root_name`, `third_party_libraries[]` with `package_name`, `package_version`, `repository`, `license` (SPDX), `licenses[]` (per-SPDX license text blocks). Meets conda-forge's `cargo-bundle-licenses` supply-chain requirement; can be referenced as a `license_file[]` entry in Plan 03's `recipe.yaml` `about:` block.
- `.planning/notes/phase-13-license-audit.md` — Audit record. Frontmatter: `tool_version: "cargo-bundle-licenses cargo:4.0.0"`, `result: "PASS"`. Documents invocation, direct-dep coverage, license-class distribution, the `mlua-sys text: NOT FOUND` cosmetic issue with upstream-proof URLs, and the confidence-level warning log.

## Decisions Made

- **cargo-bundle-licenses pinned to 4.0.0.** The latest release (4.2.0) pulls `cargo_metadata@0.23.0` which requires rustc 1.86; our toolchain is 1.85 (edition 2024 MSRV floor). 4.0.0 (released 2025-03-14) compiles against 1.85, emits the same YAML schema (just with `package_name:` instead of `- name:` — this is the older schema; v4.1+ changed the field to `name:` but 4.0.0 retained `package_name:` from its 3.x ancestry). Downstream plans / conda-forge CI don't care about the specific version as long as the schema is stable enough for their own tooling pass.
- **mlua-sys `text: NOT FOUND` is not a blocker.** The SPDX ID `MIT` is declared cleanly in `mlua-sys/Cargo.toml` (verified against upstream). The missing text is because the MIT LICENSE file sits at the mlua workspace repo root, not inside the `mlua-sys/` subcrate directory. conda-forge will run `cargo-bundle-licenses` on the same sdist and see the same warning; they'll accept it because the SPDX tag is what counts. Documented with upstream URLs in the audit note.
- **No dep upgrades, swaps, or allowlist overrides needed.** All 57 crates (direct + transitive) reported permissive SPDX IDs on first run. Task 2 skipped per the plan's conditional flow.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] cargo-bundle-licenses latest (4.2.0) requires rustc 1.86 — pinned to 4.0.0**
- **Found during:** Task 1, Step 1.1 (install)
- **Issue:** `cargo install cargo-bundle-licenses --locked` failed with `rustc 1.85.0 is not supported by the following packages: cargo-platform@0.3.1 requires rustc 1.86 / cargo_metadata@0.23.0 requires rustc 1.86.0`. Our Cargo.toml pins `edition = "2024"` (MSRV 1.85), and the system toolchain is exactly 1.85.0.
- **Fix:** Installed `cargo-bundle-licenses --version 4.0.0 --locked` (the most recent version compatible with rustc 1.85, released 2025-03-14). 4.0.0 emits the same YAML structure that the plan's downstream verification depended on; only the field name differs (`package_name:` vs `name:`).
- **Files modified:** None (dev tool install, not committed to repo).
- **Verification:** `cargo bundle-licenses --version` → `cargo-bundle-licenses cargo:4.0.0`. Tool ran with exit 0. Tool version captured in audit note frontmatter.
- **Committed in:** Documentation of this deviation is part of `.planning/notes/phase-13-license-audit.md` (commit `6f017ce`).

**2. [Rule 1 - Bug] Plan verification grep pattern `^- name:` didn't match cargo-bundle-licenses 4.0.0 output format**
- **Found during:** Task 1, Step 1.3 (direct-dep coverage check)
- **Issue:** The plan's Step 1.3 and Step 1.4 grep patterns assumed the YAML schema uses `- name: <crate>`, but v4.0.0 emits `- package_name: <crate>`. On first run, the check flagged every direct dep as "MISSING" even though every dep was actually present.
- **Fix:** Updated the grep pattern locally to `^- package_name: ${dep}$`; all 11 direct deps confirmed present. Documented the schema discrepancy in the audit note so Plan 03 / future audits know to adapt the pattern based on the installed tool version.
- **Files modified:** `.planning/notes/phase-13-license-audit.md` (documents the schema-field correction).
- **Verification:** Adapted verification pattern (`test -s THIRDPARTY.yml && grep -qE '^- package_name: pyo3$' THIRDPARTY.yml && ! grep -qE '^\s*license:\s*.*(GPL|AGPL|NOT FOUND|AMBIGUOUS|UNKNOWN|Proprietary)' THIRDPARTY.yml && echo PASS`) → PASS.
- **Committed in:** `6f017ce`.

---

**Total deviations:** 2 auto-fixed (1 blocking install dep, 1 schema-field adaptation)
**Impact on plan:** No scope creep. Both deviations were necessary to complete Task 1 as specified; neither changed the plan's objective, acceptance criteria, or output artifacts. The audit still produced a clean PASS with the expected THIRDPARTY.yml artifact.

## Issues Encountered

- Initial `cargo install cargo-bundle-licenses --locked` attempt (latest 4.2.0) failed due to transitive dep rustc 1.86 requirement — resolved via version pin (see Deviation #1).
- `cargo-bundle-licenses` 4.0.0 emitted SEMI/UNSURE license-confidence warnings for 22 crates and one "No license found" warning for mlua-sys. All are text-matching confidence levels, not SPDX-ID ambiguity. Documented in the audit note's "Warnings (informational — not blockers)" section with upstream-proof URLs for the mlua-sys edge case. No impact on audit outcome.

## User Setup Required

None — this plan is a local license audit; no external services, secrets, or env vars introduced.

## Next Plan Readiness

**Plan 03 (draft recipe on conda-forge/staged-recipes, open PR, iterate CI) is unblocked.**

The hard gate from CONTEXT.md ("Do not open the staged-recipes PR until Steps 1 AND 2 both pass") is now cleared:

- Step 1 (sdist feedstock-readiness): PASS (Plan 13-01).
- Step 2 (Rust dep license audit): PASS (this plan).

Plan 03 can now consume:
- `pinned_version: "0.1.2"` (from Plan 13-01 report frontmatter) → `recipe.yaml` `package.version:`
- `sha256: "189698190835809f73fdb5af9ead4962975181c7fc8297045a5d831c0d465add"` → `recipe.yaml` `source.sha256:`
- `sdist_url: "https://files.pythonhosted.org/packages/a4/30/..."` → `recipe.yaml` `source.url:`
- `THIRDPARTY.yml` at repo root → `recipe.yaml` `about.license_file[]` entry alongside `LICENSE`
- `cargo-bundle-licenses` confirmed as a valid `requirements.build` entry — conda-forge's same tool pass will succeed against the same source tree.

No blockers for Plan 03.

## Self-Check: PASSED

- `THIRDPARTY.yml` — FOUND
- `.planning/notes/phase-13-license-audit.md` — FOUND
- `.planning/phases/13-publish-burner-redis-to-conda-forge/13-02-SUMMARY.md` — FOUND (this file)
- Commit `6f017ce` (Task 1) — FOUND in `git log --all --oneline`
- Audit note frontmatter `result: "PASS"` — present
- All 11 direct deps confirmed via `grep -qE '^- package_name: <dep>$' THIRDPARTY.yml` — present
- Disallowed-license scan returned zero matches — clean

---
*Phase: 13-publish-burner-redis-to-conda-forge*
*Completed: 2026-04-18*
