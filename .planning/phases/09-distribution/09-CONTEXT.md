# Phase 9: Distribution - Context

**Gathered:** 2026-04-11
**Status:** Ready for planning

<domain>
## Phase Boundary

Create GitHub Actions CI workflow for building and publishing pre-built wheels to PyPI. Build targets: manylinux (x86_64, aarch64) and macOS (x86_64, arm64). Verify local wheel build works.

</domain>

<decisions>
## Implementation Decisions

### CI System
- GitHub Actions with `maturin-action` v1 (as specified in CLAUDE.md).
- Workflow file: `.github/workflows/release.yml`.
- Trigger: tag push matching `v*` for releases. Also include a `ci.yml` for PR testing (build without publish).

### Build Matrix
- manylinux x86_64 and aarch64 (via QEMU for cross-compilation).
- macOS x86_64 and arm64 (native runners for arm64).
- Skip Windows for v1 (EDIST-01 is v2 scope).
- Use `abi3-py39` feature for single-wheel-per-platform builds (no per-Python-version matrix needed).

### Release Flow
- `ci.yml`: Runs on every PR and push to main. Builds wheels for all platforms. Runs tests. Does NOT publish.
- `release.yml`: Runs on `v*` tags. Builds wheels, creates GitHub release, publishes to PyPI via `maturin publish` or `twine upload`.
- PyPI credentials stored as GitHub repository secrets (`PYPI_API_TOKEN`).

### Local Verification
- Run `maturin build --release` locally to produce a wheel file.
- Verify wheel installs and `from burner_redis import BurnerRedis` works from the built wheel.

### Claude's Discretion
No items deferred — all questions resolved.

</decisions>

<code_context>
## Existing Code Insights

### Reusable Assets
- `pyproject.toml` — Already has maturin build backend configured.
- `Cargo.toml` — Already has abi3-py39 feature enabled on pyo3.
- CLAUDE.md — Has `maturin generate-ci github` reference.

### Integration Points
- New `.github/workflows/ci.yml` — test workflow.
- New `.github/workflows/release.yml` — release workflow.
- May need `pyproject.toml` adjustments for classifiers, URLs, description.

</code_context>

<specifics>
## Specific Ideas

No specific requirements — follow maturin-action documentation and CLAUDE.md guidance.

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope.

</deferred>
