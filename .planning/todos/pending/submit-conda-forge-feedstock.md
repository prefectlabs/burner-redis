---
title: Submit burner-redis to conda-forge/staged-recipes
date: 2026-04-16
priority: medium
source: .planning/notes/conda-forge-feedstock-setup-research.md
---

# Submit burner-redis to conda-forge/staged-recipes

## Why

Self-hosted Prefect users who install via conda pick up burner-redis transitively through pydocket (already on conda-forge). Without a burner-redis feedstock, their install fails dependency resolution. This is the core distribution gap blocking the conda-install path.

## What

Submit a new recipe to `conda-forge/staged-recipes`:

1. Fork `conda-forge/staged-recipes`.
2. Add `recipes/burner-redis/recipe.yaml` (v1 schema — same as pydantic-core).
3. Base on pydantic-core's recipe: <https://github.com/conda-forge/pydantic-core-feedstock/blob/main/recipe/recipe.yaml>
4. Key ingredients (from research note):
   - `build.script: python -m pip install . -vv`
   - `requirements.build`: `{{ compiler("c") }}`, `{{ stdlib("c") }}`, `{{ compiler("rust") }}`, `cargo-bundle-licenses`, cross-compile block
   - `requirements.host`: `pip`, `python`, `maturin >=1,<2`
   - `requirements.run`: `python`
   - `extra.recipe-maintainers`: [self]
5. Open PR to `conda-forge/staged-recipes`. Wait for one approval + green CI.
6. On merge, bot auto-creates `conda-forge/burner-redis-feedstock` and adds submitter to maintainer team.

## Prerequisites

- **Sibling todos must be done first:**
  - `verify-sdist-contains-cargo-lock.md` — the autotick bot and conda-forge builders need the sdist to include `Cargo.lock` + vendored deps.
  - `audit-rust-dep-licenses.md` — `cargo-bundle-licenses` will fail loud on ambiguous-license deps; better to fix before submission than in CI.
- A PyPI release with an sdist (`.tar.gz`), not wheels-only.
- Stable PyPI URL for the `source:` block and a verified sha256.

## Cleared (not blockers)

- **Rust toolchain version:** conda-forge rust-feedstock ships 1.94.0 (2026-03-06 snapshot); our floor is 1.85+ for edition 2024. No risk.

## Acceptance

- PR to `conda-forge/staged-recipes` merges.
- `conda-forge/burner-redis-feedstock` exists and has published a first build to the conda-forge channel.
- `conda install -c conda-forge burner-redis` works on linux-64, osx-arm64, and win-64.
- Maintainer team includes at least one member of our org.

## Routing

Pick up via `/gsd-quick` when the two prerequisite todos are done. First submission is usually a ~1-hour focused session once prereqs are clear.
