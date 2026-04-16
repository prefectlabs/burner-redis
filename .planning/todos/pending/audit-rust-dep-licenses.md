---
title: Audit Rust dependency licenses with cargo-bundle-licenses
date: 2026-04-16
priority: medium
source: .planning/notes/conda-forge-feedstock-setup-research.md
---

# Audit Rust dependency licenses with cargo-bundle-licenses

## Why

conda-forge requires Rust-built packages to vendor third-party license text via `cargo-bundle-licenses`. The tool fails loudly if any dep has an ambiguous, missing, or non-standard license — and that failure will show up during the staged-recipes PR, not before. Catching it on our side shortens the submission feedback loop and avoids a "blocked on upstream license fix" surprise during review. Prerequisite for `submit-conda-forge-feedstock.md`.

## What

1. Install the tool: `cargo install cargo-bundle-licenses` (or use `uv tool install` equivalent if preferred).
2. Run:
   ```
   cargo bundle-licenses --format yaml --output THIRDPARTY.yml
   ```
3. Review output for:
   - Any dep flagged `NOT FOUND` or `AMBIGUOUS`.
   - Licenses incompatible with our Apache-2.0 (unlikely given our stack, but worth verifying): MPL-2.0, GPL, AGPL, or proprietary.
4. For each problem:
   - Check if a newer version of the dep has a clearer license.
   - Check if we can swap to an alternative crate.
   - Worst case: contact the crate maintainer or exclude via workspace (but this is rare).
5. Once clean, commit `THIRDPARTY.yml` to the repo for traceability (optional — conda-forge regenerates it, but having it in-tree helps future audits).

## Current dep surface (from CLAUDE.md tech stack)

- pyo3 (Apache-2.0 / MIT dual)
- pyo3-async-runtimes (Apache-2.0 / MIT)
- tokio (MIT)
- bytes (MIT)
- parking_lot (Apache-2.0 / MIT)
- mlua (MIT)
- rmp-serde (MIT / Apache-2.0)
- serde (MIT / Apache-2.0)
- thiserror (MIT / Apache-2.0)
- criterion (Apache-2.0 / MIT) — dev-dep, may not appear in release bundle

All expected to be clean. The audit confirms, doesn't diagnose a known issue.

## Acceptance

- `cargo bundle-licenses --format yaml` exits 0.
- No deps flagged `NOT FOUND` / `AMBIGUOUS`.
- All licenses are in the permissive/compatible set (MIT, Apache-2.0, BSD-*, ISC, etc.).
- (Optional) `THIRDPARTY.yml` committed to repo.

## Routing

Pick up via `/gsd-quick`. Should be a ~15 min audit assuming no surprises.
