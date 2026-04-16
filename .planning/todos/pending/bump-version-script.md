---
title: Add scripts/bump-version.sh helper
date: 2026-04-16
priority: low
source: .planning/notes/maturin-tag-versioning-research.md
---

# Add scripts/bump-version.sh helper

## Why

Bumping a release requires editing two files in lockstep: `Cargo.toml` `[package].version` and the `[[package]] name = "burner-redis"` block in `Cargo.lock`. Easy to forget one (orjson, polars, pydantic-core all ship helper scripts for exactly this). Paired with the tag↔Cargo.toml guard (sibling todo), it reduces the manual-bump footprint to "run the script, commit, tag".

## What

Add `scripts/bump-version.sh` that:

1. Takes a target version as positional arg (e.g. `scripts/bump-version.sh 0.1.2`).
2. Validates it's a valid semver string (regex: `^[0-9]+\.[0-9]+\.[0-9]+(-[A-Za-z0-9.]+)?$`).
3. Edits `Cargo.toml` `[package].version`.
4. Edits the `[[package]] name = "burner-redis"` block in `Cargo.lock` (first match only — env_home also has 0.1.x by coincidence).
5. Prints `git diff --stat` of the change.
6. Does NOT auto-commit or auto-tag — developer reviews the diff, commits, pushes, then tags.

Keep it POSIX shell (or Python if easier). Add README section showing usage:
```
./scripts/bump-version.sh 0.1.2
git add Cargo.toml Cargo.lock && git commit -m "chore(release): bump version to 0.1.2"
git push && git tag -a v0.1.2 -m "0.1.2 - <name>" && git push origin v0.1.2
```

## Acceptance

- Running with a valid version updates both files and exits 0.
- Running with an invalid version exits non-zero with a clear error.
- Running against a non-semver-format existing version (shouldn't happen, but) fails gracefully.
- Re-running for the same version is a no-op (idempotent).

## Priority

Lower than the CI guard. The guard is the seatbelt; this script is the power window. Do the guard first.

## Routing

Pick up via `/gsd-quick` when ready. Small one-file addition.
