---
title: Add tag↔Cargo.toml version guard to release workflow
date: 2026-04-16
priority: medium
source: .planning/notes/maturin-tag-versioning-research.md
---

# Add tag↔Cargo.toml version guard to release workflow

## Why

On v0.1.1, `git tag v0.1.1` was pushed without bumping `Cargo.toml`, and PyPI rejected the upload as a duplicate of 0.1.0. Maturin has no git-tag-driven versioning (see research note); the unanimous convention in polars / pydantic-core / orjson is a CI guard that fails the release if the tag doesn't match `Cargo.toml`.

## What

Add a new job `verify-version` at the top of `.github/workflows/release.yml` (in parallel with or ahead of `gate`), gating `build`, `sdist`, and `release` via `needs:`. Fails fast when `${GITHUB_REF_NAME#v}` ≠ `Cargo.toml [package].version`.

Reference snippet (from research):

```yaml
- name: Verify tag matches Cargo.toml version
  if: startsWith(github.ref, 'refs/tags/v')
  run: |
    TAG_VERSION="${GITHUB_REF_NAME#v}"
    CARGO_VERSION=$(grep -m1 '^version = ' Cargo.toml | sed -E 's/version = "(.*)"/\1/')
    if [ "$TAG_VERSION" != "$CARGO_VERSION" ]; then
      echo "::error::Tag $TAG_VERSION != Cargo.toml version $CARGO_VERSION"
      exit 1
    fi
    echo "Version OK: $CARGO_VERSION"
```

Consider also asserting on `Cargo.lock` (the `[[package]] name = "burner-redis"` block) to catch partial bumps.

## Acceptance

- Pushing a `v*` tag where `Cargo.toml` doesn't match fails at the guard within seconds, before any wheel is built.
- Pushing a matching tag passes and proceeds to `gate` + `build` + `sdist` + `release` as today.
- The guard is in release.yml only — no changes to ci.yml or pydocket-compat.yml.

## Routing

Pick up via `/gsd-quick` when ready. Small change, ~15 lines YAML.
