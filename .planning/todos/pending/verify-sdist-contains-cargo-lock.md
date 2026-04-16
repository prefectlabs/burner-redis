---
title: Verify sdist includes Cargo.lock and vendored Rust deps
date: 2026-04-16
priority: medium
source: .planning/notes/conda-forge-feedstock-setup-research.md
---

# Verify sdist includes Cargo.lock and vendored Rust deps

## Why

conda-forge builders can be air-gapped — they can't fetch Rust crates from the network at build time. The sdist uploaded to PyPI must carry `Cargo.lock` (and, ideally, vendored deps) so that `cargo build` inside a conda-forge CI job works offline. If the sdist is missing these, the conda-forge recipe build will fail after submission with no easy workaround. This is a prerequisite for `submit-conda-forge-feedstock.md`.

## What

1. Run `maturin sdist` locally and inspect the resulting `.tar.gz`:
   ```
   tar -tzf target/wheels/burner_redis-*.tar.gz | grep -E 'Cargo\.(toml|lock)'
   ```
2. Confirm both `Cargo.toml` AND `Cargo.lock` are present.
3. If `Cargo.lock` is missing, update `pyproject.toml`'s `[tool.maturin]` or the sdist `include`/`MANIFEST.in` equivalent to force its inclusion. (maturin includes it by default in recent versions — verify against our version.)
4. **Optional but recommended:** vendor deps via `cargo vendor` and include `vendor/` + `.cargo/config.toml` in the sdist so builds work fully offline. Check pydantic-core's approach — they vendor.
5. Add a CI gate to `release.yml` (sibling to the existing `verify-version` job): unpack the sdist and grep for `Cargo.lock` before publishing to PyPI, so this regression can't re-occur silently.

## Acceptance

- `maturin sdist` produces a tarball that contains both `Cargo.toml` and `Cargo.lock`.
- (Optional) Tarball also contains `vendor/` with offline-buildable deps.
- A CI step in `release.yml` asserts `Cargo.lock` presence in the sdist before upload, failing fast if absent.
- Manual verification: `pip install <sdist.tar.gz>` on a machine with network disabled works (if vendoring applied).

## Routing

Pick up via `/gsd-quick`. Small change — maybe 20 min if maturin already includes Cargo.lock by default; longer if vendoring is added.
