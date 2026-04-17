---
title: conda-forge feedstock setup for burner-redis — research findings
date: 2026-04-16
topic: distribution
---

# conda-forge feedstock setup — research findings

## TL;DR

burner-redis needs a conda-forge feedstock so that self-hosted Prefect users who install via conda can pick it up transitively through pydocket (which already has a feedstock). This is a **one-time submission** via `conda-forge/staged-recipes` followed by ~5 min/month of autotick-PR merges. The closest precedent is **pydantic-core** — its `recipe.yaml` is ~40 lines and near-identical to what burner-redis needs. All first-timer gotchas have been audited; only three of the four from the initial survey apply to us.

## Why

- **Consumer chain:** `prefect → pydocket → burner-redis`. Prefect ships to conda, pydocket already has a conda-forge feedstock, burner-redis is the missing link for conda users.
- **Status today:** PyPI-only. A Prefect conda user who tries to install pydocket with burner-redis as a dep will hit a resolution failure because burner-redis isn't on conda-forge.
- **Out of scope here:** making pydocket actually declare burner-redis as a conda dep. That's a pydocket-side change and blocked on burner-redis existing on conda-forge first.

## Submission flow (one-time)

1. Fork `conda-forge/staged-recipes`.
2. Add `recipes/burner-redis/recipe.yaml` (prefer v1 schema — same as pydantic-core).
3. Open PR. CI lints + builds across platforms.
4. Review: one approval from a staged-recipes team member + green CI = merge.
5. Bot auto-creates `conda-forge/burner-redis-feedstock`, adds submitter to maintainer team with push rights, wires Azure/GitHub Actions CI, uploads to `conda-forge` channel.
6. `staged-recipes` is out of the picture forever after merge.

## Recipe shape (adapted from pydantic-core)

```yaml
build:
  script: python -m pip install . -vv   # maturin picks up pyproject.toml automatically
requirements:
  build:
    - ${{ compiler("c") }}
    - ${{ stdlib("c") }}
    - ${{ compiler("rust") }}         # conda-forge rust toolchain
    - cargo-bundle-licenses            # REQUIRED — vendors Rust dep license text
    - if: build_platform != target_platform
      then: [python, cross-python_${{ target_platform }}, maturin >=1,<2]
  host: [pip, python, maturin >=1,<2]
  run: [python]
```

Key ingredients:
- `{{ compiler("rust") }}` for the Rust toolchain.
- `maturin` in **host** (it's the PEP 517 backend).
- Plain `pip install . -vv` as the script.
- `cargo-bundle-licenses` to vendor third-party Rust license text (conda-forge requirement, not optional).
- PyO3 `abi3-py39` feature → one wheel per platform instead of per-Python-version.

**Template to copy verbatim:** <https://github.com/conda-forge/pydantic-core-feedstock/blob/main/recipe/recipe.yaml>

## Build matrix

- No `noarch: python` (it's compiled).
- conda-forge auto-builds: linux-64, linux-aarch64, linux-ppc64le, osx-64, osx-arm64, win-64, per supported Python version (or once per platform with abi3).
- Cross-compile template handles osx-arm64 / linux-aarch64 via `crossenv` + `cross-python_${{ target_platform }}`.
- Add `skip:` selectors only for platforms you can't support.

## Release automation (regro-cf-autotick-bot)

- Polls PyPI on a loop. When a new `burner-redis` version hits PyPI, opens a version-bump PR on the feedstock with new version + updated sha256.
- **Prerequisite: PyPI release must include an sdist (`.tar.gz`), not just wheels** — the bot needs the sdist URL in `source:`. See sibling todo `verify-sdist-contains-cargo-lock.md` which also covers this.
- Merge the PR if deps didn't change; push fixups to the bot's branch if they did.
- Bot stops opening PRs if >3 are open unmerged.

## Gotchas audited for burner-redis

| # | Gotcha | Applies? | Notes |
|---|--------|----------|-------|
| 1 | sdist must include `Cargo.lock` + vendored deps (air-gapped builders) | **YES** | Check `pyproject.toml` / `Cargo.toml` manifest. Captured as todo `verify-sdist-contains-cargo-lock.md`. |
| 2 | `cargo-bundle-licenses` fails on ambiguous-license Rust deps | **YES** | Audit before submitting. Captured as todo `audit-rust-dep-licenses.md`. |
| 3 | conda-forge rust toolchain lags stable by ~weeks; Cargo.toml requires 1.85+ for edition 2024 | **NO — CLEARED** | Snapshot 2026-04-16: rust-feedstock ships **1.94.0** (bumped 2026-03-06). 9 minor versions ahead of our floor. Cadence: ~6 weeks lag, tracks upstream. |
| 4 | mlua (Lua 5.4) — vendored build path easier than conda's `lua` package | **YES** | Confirm mlua's `lua54` feature with vendored build is enabled in `Cargo.toml`. |

## Maintainer responsibilities

- List self in `extra.recipe-maintainers` on the recipe.
- Commit rights to `conda-forge/burner-redis-feedstock`, review rights on autotick PRs.
- Ongoing effort for stable packages: ~5 min/month.
- Additional bot-opened PRs to merge periodically: compiler bumps, Python version adds, rust toolchain migrations.

## Sources

- [conda-forge staged-recipes docs](https://conda-forge.org/docs/maintainer/understanding_conda_forge/staged_recipes/)
- [staged-recipes repo](https://github.com/conda-forge/staged-recipes)
- [pydantic-core-feedstock recipe.yaml](https://github.com/conda-forge/pydantic-core-feedstock/blob/main/recipe/recipe.yaml) — **primary template**
- [polars-feedstock recipe.yaml](https://github.com/conda-forge/polars-feedstock/blob/main/recipe/recipe.yaml) — only if splitting runtime crates later
- [conda-forge updating_pkgs (autotick bot)](https://conda-forge.org/docs/maintainer/updating_pkgs/)
- [conda-forge/rust-feedstock](https://github.com/conda-forge/rust-feedstock) — version snapshot: 1.94.0 as of 2026-03-06
- [maturin-feedstock](https://github.com/conda-forge/maturin-feedstock)

## Follow-ups

- Submit feedstock PR: todo `submit-conda-forge-feedstock.md`
- Verify sdist includes Cargo.lock: todo `verify-sdist-contains-cargo-lock.md`
- Audit Rust dep licenses: todo `audit-rust-dep-licenses.md`
