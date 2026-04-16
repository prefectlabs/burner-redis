# Quick Task 260415-tc2: Add pydocket test suite CI workflow

**Status:** Complete
**Date:** 2026-04-16

## Changes

### Task 1: Create pydocket compatibility workflow
- **File:** `.github/workflows/pydocket-compat.yml`
- **Commits:** `3ad029c`, `b4739d7`
- Created new workflow that runs on push/PR to main
- Builds burner-redis from source, checks out pydocket at `replace-fakeredis-with-burner-redis` branch
- Runs pydocket tests with `REDIS_VERSION=memory` to use burner-redis in-memory backend
- Overrides pydocket's default addopts (coverage requirements not relevant for compat check)
- Uses pytest-xdist for parallel test execution with 30s timeout
