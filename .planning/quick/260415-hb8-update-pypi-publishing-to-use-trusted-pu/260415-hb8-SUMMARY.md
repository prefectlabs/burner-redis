# Quick Task 260415-hb8: Update PyPI publishing to use trusted publishers OIDC

**Date:** 2026-04-15
**Status:** Complete

## Changes

- Replaced `PyO3/maturin-action@v1` upload step with `pypa/gh-action-pypi-publish@release/v1`
- Removed `MATURIN_PYPI_TOKEN` secret dependency
- Added `environment: name: pypi` block for trusted publisher matching
- Sigstore attestations generated automatically by the new action

## Manual Step Required

Before the first release, configure the trusted publisher on PyPI:
1. Go to https://pypi.org/manage/project/burner-redis/settings/publishing/
2. Add a GitHub Actions publisher: owner=prefectlabs, repo=burner-redis, workflow=release.yml, environment=pypi

## Files Modified
- `.github/workflows/release.yml`
