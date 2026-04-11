---
phase: quick
plan: 260411-ipj
subsystem: ci
tags: [ci, testing, integration-tests, pytest]
dependency_graph:
  requires: [260411-b8i]
  provides: [integration-test-ci-job]
  affects: [.github/workflows/ci.yml, pyproject.toml, tests/test_prefect_integration.py]
tech_stack:
  added: []
  patterns: [pytest-markers, conditional-ci-jobs]
key_files:
  created: []
  modified:
    - .github/workflows/ci.yml
    - pyproject.toml
    - tests/test_prefect_integration.py
decisions:
  - Used pytest markers with addopts exclusion pattern for clean separation of unit and integration tests
  - Added explicit Rust toolchain step to both test and integration-test CI jobs for reproducibility
metrics:
  duration: 2min
  completed: "2026-04-11"
  tasks: 2
  files: 3
---

# Quick Task 260411-ipj: Run Integration Tests in CI on Merge to Main Summary

**Separate integration tests from PR CI for fast feedback; run them only on merge to main as a quality gate.**

## What Was Done

### Task 1: Mark integration tests and configure pytest exclusion
- Added `pytestmark = pytest.mark.integration` to `tests/test_prefect_integration.py`
- Added `markers` and `addopts = "-m 'not integration'"` to `[tool.pytest.ini_options]` in `pyproject.toml`
- Result: bare `pytest` collects 250 unit tests (0 integration), `pytest -m integration` collects 24 integration tests
- Commit: 7bac8dc

### Task 2: Add integration-test CI job for push to main
- Added `integration-test` job to `.github/workflows/ci.yml` with `if: github.event_name == 'push'` condition
- Job: checkout, Python 3.12, Rust toolchain, install deps, maturin build, `pytest -m integration`
- Added explicit `Install Rust toolchain` step (dtolnay/rust-toolchain@stable) to existing `test` job for reproducibility
- Commit: 577c6e8

## Verification Results

- `pytest --co -q` collects 0 tests from test_prefect_integration.py (confirmed)
- `pytest -m integration --co -q` collects 24 integration tests (confirmed)
- `pytest` runs 250 passed, 24 deselected (confirmed)
- CI workflow has `integration-test` job with `if: github.event_name == 'push'` condition (confirmed)

## Deviations from Plan

None - plan executed exactly as written.

## Self-Check: PASSED

- [x] pyproject.toml modified with markers and addopts
- [x] tests/test_prefect_integration.py has pytestmark
- [x] .github/workflows/ci.yml has integration-test job
- [x] Commit 7bac8dc exists
- [x] Commit 577c6e8 exists
