---
phase: 13
title: Publish burner-redis to conda-forge
created: 2026-04-17
source: brainstorm conversation 2026-04-17
---

# Phase 13 Context — Publish burner-redis to conda-forge

Captures the pre-plan brainstorm so `/gsd-plan-phase 13` has the full decision record.

## Goal

`conda install -c conda-forge burner-redis` works on linux-64, linux-aarch64, osx-64, osx-arm64, and win-64 — unblocking conda users of Prefect who pick up burner-redis transitively through pydocket.

## Scope

**In.** A single phase covering three serial steps:

1. Verify the PyPI sdist is feedstock-ready (contains `Cargo.lock`, builds offline).
2. Audit Rust dep licenses with `cargo-bundle-licenses` and produce `THIRDPARTY.yml`.
3. Submit a new recipe to `conda-forge/staged-recipes`, iterate on CI, confirm the feedstock publishes.

**Out.** Pydocket declaring burner-redis as a conda dep (separate pydocket-side work). Ongoing autotick-PR merging (operational, not a project).

## Locked decisions (from 2026-04-17 brainstorm)

- **Execution:** strict serial (Option A). Each step gates the next. If Step 1 fails, fix and re-verify before proceeding.
- **Source pin:** decide after Step 1. Default to 0.1.2; bump to 0.1.3 only if 0.1.2's sdist is not feedstock-ready.
- **Maintainers:** `ajstreed` solo in `extra.recipe-maintainers`.
- **Build matrix:** PyPI-matched — linux-64, linux-aarch64, osx-64, osx-arm64, win-64. Skip linux-ppc64le via `build.skip:`.
- **Recipe schema:** re-check `conda-forge/staged-recipes` README at submission time; default expectation is v1 `recipe.yaml` matching pydantic-core-feedstock.

## Step-by-step flow

### Step 1 — Verify sdist is feedstock-ready

- Download `burner-redis-0.1.2.tar.gz` from PyPI (or rebuild locally via `maturin sdist`).
- Unpack; assert `Cargo.lock` present; check `[tool.maturin]` sdist include config covers it.
- Build from sdist in an offline-simulated environment (container with no network, or `CARGO_NET_OFFLINE=true` with pre-populated cargo cache).
- **Artifact:** short verification report committed as a planning note.
- **Exit branch — fail:** fix `pyproject.toml` sdist includes, land on `main`, cut 0.1.3, repeat Step 1, pin Step 3 to 0.1.3.
- **Exit branch — pass:** pin Step 3 to 0.1.2.

### Step 2 — Audit Rust dep licenses

- Install `cargo-bundle-licenses`. Run `cargo bundle-licenses --format yaml --output THIRDPARTY.yml`.
- Tool exits non-zero on ambiguous/unknown licenses. Fixes in order of preference: (a) upgrade dep; (b) swap dep; (c) explicit allowlist override only if upstream license is known-safe but badly declared.
- **Artifact:** `THIRDPARTY.yml` committed; audit summary in a planning note.

### Step 3 — Draft recipe

- Re-read `conda-forge/staged-recipes` README for current schema recommendation. Default: v1 `recipe.yaml`.
- Base on `pydantic-core-feedstock/recipe/recipe.yaml`. Key fields:
  - `package.name: burner-redis`, `version: <resolved from Step 1>`.
  - `source.url:` PyPI sdist, `sha256:` from the verified tarball.
  - `build.script: python -m pip install . -vv`.
  - `requirements.build`: `${{ compiler("c") }}`, `${{ stdlib("c") }}`, `${{ compiler("rust") }}`, `cargo-bundle-licenses`, cross-compile block.
  - `requirements.host`: `pip`, `python`, `maturin >=1,<2`.
  - `requirements.run`: `python`.
  - `build.skip:` expression excluding `linux-ppc64le`.
  - `extra.recipe-maintainers: [ajstreed]`.
- **Artifact:** `recipes/burner-redis/recipe.yaml` on a fork of `conda-forge/staged-recipes`.

### Step 4 — Open PR, iterate on CI

- Push fork branch. Open PR to `conda-forge/staged-recipes`.
- Respond to reviewer comments + CI failures. Typical first-timer fixes: license summary, compiler ranges, skip-selector syntax.
- **Artifact:** merged PR.

### Step 5 — Post-merge verification

- Confirm `conda-forge/burner-redis-feedstock` repo exists and `ajstreed` is on the maintainer team.
- Confirm first feedstock build succeeded; packages visible via `conda search -c conda-forge burner-redis`.
- Smoke test on one platform: `conda install -c conda-forge burner-redis` + `import burner_redis`.
- **Artifact:** confirmation note in `.planning/notes/`.

## Risks & branch points

| # | Risk | Detection | Response |
|---|------|-----------|----------|
| 1 | sdist missing `Cargo.lock` / vendored deps | Step 1 offline build fails | Fix `pyproject.toml`, cut 0.1.3, re-verify, pin to 0.1.3 |
| 2 | `cargo-bundle-licenses` fails on ambiguous dep | Step 2 non-zero exit | Upgrade dep → swap dep → manual allowlist override (document in commit) |
| 3 | CI fails on cross-compile (osx-arm64 / linux-aarch64) | staged-recipes PR CI red on platform | Mirror pydantic-core's cross block; add to `build.skip:` if fundamentally unsupported |
| 4 | Reviewer pushes back on v1 schema or build script | PR review comment | Defer to reviewer — they're the conda-forge expert. Rewrite to match guidance |
| 5 | `maturin >=1,<2` constraint rejected | Unusual | Align with current pydantic-core-feedstock / polars-feedstock pins |
| 6 | Autotick bot misconfigured post-merge | No PR within ~24h of next PyPI release | Manual feedstock edit; open issue on `regro/cf-scripts` if persistent |

**Hard gate:** do not open the staged-recipes PR until Steps 1 and 2 both pass.

## Absorbed todos

These three pending todos are superseded by this phase and should be moved to `completed/` as each step lands:

- `.planning/todos/pending/verify-sdist-contains-cargo-lock.md` — Step 1.
- `.planning/todos/pending/audit-rust-dep-licenses.md` — Step 2.
- `.planning/todos/pending/submit-conda-forge-feedstock.md` — Steps 3–5.

## Acceptance criteria

1. PR to `conda-forge/staged-recipes` merges.
2. `conda-forge/burner-redis-feedstock` exists; `ajstreed` is on the maintainer team with push rights.
3. First feedstock build publishes to the `conda-forge` channel for the 5-platform matrix (linux-64, linux-aarch64, osx-64, osx-arm64, win-64).
4. Smoke test: `conda create -n bt-smoke -c conda-forge burner-redis python=3.12 && conda run -n bt-smoke python -c "import burner_redis; burner_redis.BurnerRedis()"` succeeds on at least one platform.

## References

- `.planning/notes/conda-forge-feedstock-setup-research.md` — recipe template, gotcha audit, full submission flow (primary source).
- `.planning/todos/pending/{verify-sdist-contains-cargo-lock,audit-rust-dep-licenses,submit-conda-forge-feedstock}.md` — absorbed todos.
- [pydantic-core-feedstock recipe.yaml](https://github.com/conda-forge/pydantic-core-feedstock/blob/main/recipe/recipe.yaml) — primary template to copy.
- [conda-forge staged-recipes docs](https://conda-forge.org/docs/maintainer/understanding_conda_forge/staged_recipes/).

## Gotchas already cleared (from research note, no action needed)

- **Rust toolchain lag.** conda-forge rust-feedstock is at 1.94.0 (2026-03-06 snapshot). Our floor is 1.85+. No risk.
- **mlua Lua 5.4 vendored build.** `Cargo.toml` already has `mlua = { ..., features = ["lua54", "send", "vendored"] }`.
- **abi3 wheel.** `pyproject.toml` already has `features = ["pyo3/extension-module", "pyo3/abi3-py310"]` — one wheel per platform.
