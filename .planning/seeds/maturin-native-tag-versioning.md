---
title: Revisit maturin native tag-based versioning
trigger_condition: "maturin releases a version that resolves PyO3/maturin#2163 (version override mechanism)"
planted_date: 2026-04-16
source: .planning/notes/maturin-tag-versioning-research.md
---

# Revisit maturin native tag-based versioning

## Trigger

Watch [PyO3/maturin#2163](https://github.com/PyO3/maturin/issues/2163). When a maturin release ships native support for overriding the wheel version (CLI flag, `[tool.maturin]` key, or env var), surface this seed.

## Why parked

As of maturin v1.13.1 (Apr 2026), there is no way to derive the Python wheel version from git tags in a maturin build. Every mature PyO3/maturin project (polars, pydantic-core, orjson) works around this with a manual `Cargo.toml` bump + CI guard. We've adopted the same pattern.

If/when maturin adds a mechanism like `--version=$GITHUB_REF_NAME` or `[tool.maturin] version-source = "git-tag"`, we can:

- Drop the manual Cargo.toml bump step from the release process
- Drop the tag↔Cargo.toml CI guard (if maturin enforces the constraint natively)
- Keep the `bump-version.sh` helper only for intentional dev-mode bumps, not for releases

## What to do when triggered

1. Read the maturin changelog / release notes for the relevant version
2. Evaluate: does the new mechanism cover our use case (tag-triggered release workflow on GitHub Actions)?
3. If yes: open a `/gsd-quick` to migrate `release.yml` — remove the bump step, wire up the native mechanism, keep the guard as a belt-and-braces until we've seen one successful release
4. If no (incomplete or constrained): update this seed with the new state and re-plant

## Out of scope while parked

- Don't build custom workarounds trying to anticipate the feature
- Don't switch away from maturin — it's a hard constraint
