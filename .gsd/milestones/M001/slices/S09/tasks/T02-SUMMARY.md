# T02: Create the GitHub Actions release workflow that builds wheels and publishes to PyPI when a version tag is pushed.

**Slice:** S09 — **Milestone:** M001

## Legacy Summary

---
phase: 09-distribution
plan: 02
subsystem: release-distribution
tags: [github-actions, pypi, release, maturin, wheels]
dependency_graph:
  requires: [09-01]
  provides: [pypi-release-workflow, github-releases]
  affects: []
tech_stack:
  added: [softprops/action-gh-release, pypi-trusted-publishing]
  patterns: [tag-triggered-release, multi-artifact-merge]
key_files:
  created:
    - .github/workflows/release.yml
    - README.md
  modified:
    - .github/workflows/ci.yml
decisions:
  - "PyPI auth via MATURIN_PYPI_TOKEN secret with OIDC id-token permission for future trusted publisher migration"
  - "GitHub Release uses softprops/action-gh-release@v2 with auto-generated release notes"
  - "sdist job added to both CI and release for consistent validation"
metrics:
  duration: 3min
  completed: "2026-04-11T04:22:39Z"
  tasks: 2
  files: 3
---

# Phase 09 Plan 02: Release and PyPI Publish Workflow Summary

Release workflow triggers on v* tag push, builds wheels for 4 platforms plus sdist, publishes to PyPI via maturin upload with API token, and creates GitHub Release with all artifacts attached.

## Tasks Completed

| Task | Name | Commit | Files |
|------|------|--------|-------|
| 1 | Create release workflow with PyPI publishing | 21ab3fa | .github/workflows/release.yml |
| 2 | Add sdist build to CI workflow and verify local wheel build | 34eca3a | .github/workflows/ci.yml, README.md |

## Key Changes

### Release Workflow (Task 1)
- **Trigger:** Push of `v*` tags only (e.g., v0.1.0, v1.0.0)
- **Build job:** 4-target matrix (linux x86_64/aarch64, macOS x86_64/arm64) with QEMU for aarch64 cross-compilation
- **sdist job:** Builds source distribution on ubuntu-latest
- **Release job:** Downloads all artifacts, publishes to PyPI using MATURIN_PYPI_TOKEN, creates GitHub Release with auto-generated notes
- **Security:** id-token: write permission for OIDC trusted publisher; contents: write for release creation

### CI Workflow Update (Task 2)
- Added `sdist` job parallel to existing test and build jobs
- Validates source distribution builds correctly on every push/PR
- Ensures sdist format is consistent between CI and release workflows

### README.md (Task 2 - Rule 3 auto-fix)
- Created README.md required by pyproject.toml `readme = "README.md"` field
- Without it, maturin build failed with missing file error
- Contains project description, installation, usage example, feature list

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Created README.md for wheel build**
- **Found during:** Task 2 local wheel verification
- **Issue:** pyproject.toml declares `readme = "README.md"` but file did not exist; maturin build --release failed
- **Fix:** Created README.md with project description, usage, and features
- **Files created:** README.md
- **Commit:** 34eca3a

## Verification Results

- release.yml: valid YAML, triggers on v* tags, build job with 4 matrix entries, sdist job, release job depending on both, QEMU for aarch64, PyPI token in env, GitHub Release with generated notes, id-token permission
- ci.yml: now has test, build, and sdist jobs
- Local wheel build: produced burner_redis-0.1.0-cp39-abi3-macosx_11_0_arm64.whl
- Wheel install and import: `from burner_redis import BurnerRedis` succeeds

## Known Stubs

None.

## Threat Flags

None - no new security surface beyond what the plan's threat model covers (PyPI token in secrets, id-token permission, tag-based release trigger).

## Self-Check: PASSED

- [x] .github/workflows/release.yml exists
- [x] .github/workflows/ci.yml exists
- [x] README.md exists
- [x] Commit 21ab3fa found
- [x] Commit 34eca3a found
