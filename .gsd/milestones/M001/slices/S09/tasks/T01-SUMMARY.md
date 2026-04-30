# T01: Create the GitHub Actions CI workflow that builds cross-platform wheels for burner-redis on every PR and push to main.

**Slice:** S09 — **Milestone:** M001

## Legacy Summary

---
phase: 09-distribution
plan: 01
subsystem: ci-distribution
tags: [ci, github-actions, maturin, pypi, wheels]
dependency_graph:
  requires: []
  provides: [ci-workflow, pypi-metadata]
  affects: [09-02]
tech_stack:
  added: [maturin-action, github-actions]
  patterns: [cross-platform-matrix, abi3-single-wheel]
key_files:
  created:
    - .github/workflows/ci.yml
  modified:
    - pyproject.toml
decisions:
  - "4-target build matrix: linux x86_64/aarch64 + macOS x86_64/arm64 (no Windows)"
  - "QEMU for aarch64 cross-compilation on Linux runners"
  - "No caching or sccache -- keep workflow simple for initial version"
metrics:
  duration: 1min
  completed: "2026-04-11T04:17:48Z"
  tasks: 2
  files: 2
---

# Phase 09 Plan 01: CI Build Pipeline Summary

CI workflow builds cross-platform wheels (manylinux x86_64/aarch64, macOS x86_64/arm64) using maturin-action with abi3-py39 for single wheel per platform; pyproject.toml updated with full PyPI metadata.

## Tasks Completed

| Task | Name | Commit | Files |
|------|------|--------|-------|
| 1 | Update pyproject.toml with PyPI metadata | 4504f9b | pyproject.toml |
| 2 | Create CI workflow with cross-platform wheel builds | 77776ef | .github/workflows/ci.yml |

## Key Changes

### pyproject.toml (Task 1)
- Added `license = {text = "MIT"}`
- Added `readme = "README.md"` for PyPI long description
- Added 8 classifiers (Rust, CPython, Python 3, Beta, Developers, MIT, Linux, macOS)
- Added `[project.urls]` with Homepage, Repository, Issues links

### CI Workflow (Task 2)
- **Test job**: Ubuntu runner, Python 3.12, maturin develop --release, pytest
- **Build job**: 4-target matrix strategy
  - `ubuntu-latest` / `x86_64` (native)
  - `ubuntu-latest` / `aarch64` (cross-compile via QEMU)
  - `macos-13` / `x86_64` (Intel runner)
  - `macos-14` / `aarch64` (Apple Silicon runner)
- Uses `PyO3/maturin-action@v1` with `--release --out dist`
- abi3-py39 (from Cargo.toml) produces single wheel per platform
- Uploads artifacts as `wheels-{os}-{target}`
- No publish/release steps (deferred to Plan 02)

## Deviations from Plan

None - plan executed exactly as written.

## Verification Results

- pyproject.toml: valid TOML, license present, 8 classifiers, URLs with Homepage/Repository/Issues
- ci.yml: valid YAML structure, push+PR triggers on main, test job with pytest, build job with 4 matrix entries, maturin-action, QEMU for aarch64 linux, artifact upload, no publish steps

## Known Stubs

None.

## Self-Check: PASSED

- [x] .github/workflows/ci.yml exists
- [x] pyproject.toml exists
- [x] Commit 4504f9b found
- [x] Commit 77776ef found
