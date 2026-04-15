# Quick Task 260415-gtu: Add MIT LICENSE file and set up dynamic versioning

**Date:** 2026-04-15
**Status:** Complete

## Changes

1. Created MIT LICENSE file in repo root (copyright Prefect Technologies, Inc.)
2. Replaced static `version = "0.1.0"` with `dynamic = ["version"]` in pyproject.toml
3. Cargo.toml (version = "0.1.0") is now the single source of truth — maturin reads it automatically

## Note on hatch-vcs

hatch-vcs is incompatible with maturin (it requires hatchling as build backend). For maturin projects, the standard approach is:
- `dynamic = ["version"]` in pyproject.toml delegates to Cargo.toml
- Update Cargo.toml version before tagging releases
- CI release workflow fires on `v*` tags

## Files Modified
- `LICENSE` (new)
- `pyproject.toml` — static version replaced with dynamic
