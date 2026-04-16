---
title: Maturin tag-based versioning — research findings
date: 2026-04-16
topic: release-automation
---

# Maturin tag-based versioning — research findings

## TL;DR

No clean, first-class "derive version from git tag" mechanism exists for maturin as of v1.13.1 (Apr 2026). Cargo.toml `[package].version` remains the source of truth; no CLI flag, no `[tool.maturin]` config key, no env-var override has been merged. Every major PyO3/maturin project (polars, pydantic-core, orjson) solves this the same way: **require manual Cargo.toml bump + add a CI guard that fails the release if the tag doesn't match the manifest**. Recommendation: adopt the guard pattern.

## 1. Maturin-native tag versioning

**Not supported.** Verified:

- `[tool.maturin]` config reference (maturin.rs/config) lists no `version-source`, `version-from-git`, or similar key.
- No CLI flag on `maturin build` / `maturin develop` accepts a version override.
- Changelog through v1.13.x mentions dynamic-version handling only in [PR #2391](https://github.com/PyO3/maturin/pull/2391) (v1.8.0, Dec 2024), which fixes parsing of `dynamic = ["version"]` — it still resolves from `Cargo.toml`, not git.
- [Issue #2163](https://github.com/PyO3/maturin/issues/2163) "Add a way to override the resultant Python package version" remains **open and unimplemented** as of Dec 2025 discussion. Maintainers discuss alternatives but nothing merged.
- Maintainer position ([discussion #1267](https://github.com/PyO3/maturin/discussions/1267)): "setuptools_scm … relies on extension APIs of setuptools, maturin as a pep517 build backend doesn't even use setuptools so it can't support setuptools_scm." Suggested alt: [cargo-release](https://github.com/crate-ci/cargo-release).

## 2. Dynamic backend compat

**None of the standard PEP 621 dynamic-version plugins are compatible with maturin.**

- **hatch-vcs** — is a hatchling plugin; requires `build-backend = "hatchling.build"`. Maturin *is* the backend, so it cannot be swapped in.
- **setuptools-scm** — confirmed unsupported by maturin maintainers (see above). Hooks into setuptools' extension API.
- **dunamai** — is a library for *computing* a version string at runtime; it cannot inject into maturin's build pipeline because maturin has no plugin/hook mechanism that calls out to Python during metadata resolution.
- No documented way for any external tool to rewrite `Cargo.toml` at "build-entry time" inside a PEP 517 invocation — maturin reads the file directly before any user hook runs.

## 3. Cargo-side approaches

**`build.rs` cannot modify `[package].version`** ([cargo issue #12144](https://github.com/rust-lang/cargo/issues/12144), closed Oct 2023). Cargo evaluates the manifest *before* any build script runs; build scripts run in a sandbox and cannot rewrite the manifest that spawned them. Issue closed in favor of long-running design thread #6583, still unimplemented.

## 4. How real projects handle it

All surveyed projects **bump Cargo.toml manually and verify in CI**:

- **polars** — `release-python.yml` has a "Check runtime versions" step that `tomlq`-extracts versions from pyproject.toml and all three `Cargo.toml` files and `exit 1`s on mismatch. Bumping is a manual PR.
- **pydantic-core** — Release docs explicitly say "Bump package version locally with both Cargo.toml and Cargo.lock, then make a PR for the version bump and merge it" before drafting the tag.
- **orjson** — `script/check-version` (Python) loads both TOML files with `tomllib` and `assert`s versions are identical; runs as a CI step on tag pushes.
- **tiktoken / cryptography** — use maturin but follow the same bump-then-tag convention.

**Common pattern = manual bump PR → tag → CI guards.** No project in this set uses automation to derive Cargo.toml version from the tag.

## 5. Fallback validation check

Minimal guard (fails the release job before `maturin publish` runs):

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

## Recommendation

**Adopt the fallback validation check.** It's the unanimous convention in mature maturin projects and catches exactly the failure mode that bit us (tag-without-bump → PyPI duplicate rejection). Add it as the first step of the publish job, before `maturin build`, so the wheel build never runs against a mismatched tag. Optionally pair with a `scripts/bump-version.sh` helper that edits `Cargo.toml` + `Cargo.lock` + commits, to reduce human error at bump time. Do **not** wait for maturin to add native support — issue #2163 has been open 18+ months with no movement.

## Follow-up task

Open `/gsd-quick` to add the tag-vs-Cargo.toml guard to the release workflow, plus an optional `scripts/bump-version.sh` helper.
