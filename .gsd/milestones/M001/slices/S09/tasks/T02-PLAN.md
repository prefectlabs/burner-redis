# T02: Create the GitHub Actions release workflow that builds wheels and publishes to PyPI when a version tag is pushed.

**Slice:** S09 — **Milestone:** M001

## Description

Create the GitHub Actions release workflow that builds wheels and publishes to PyPI when a version tag is pushed. This completes the distribution pipeline.

Purpose: Enable one-command releases: push a `v*` tag, wheels build for all platforms, and the package appears on PyPI.
Output: `.github/workflows/release.yml` workflow file.

## Legacy Source

---
phase: 09-distribution
plan: 02
type: execute
wave: 2
depends_on: ["09-01"]
files_modified:
  - .github/workflows/release.yml
autonomous: true
requirements:
  - DIST-01

must_haves:
  truths:
    - "Pushing a v* tag triggers wheel builds and publishes to PyPI"
    - "Release workflow creates a GitHub release with wheel assets"
    - "PyPI publish uses trusted publisher (OIDC) or API token secret"
  artifacts:
    - path: ".github/workflows/release.yml"
      provides: "Release and PyPI publish workflow"
      contains: "pypi"
  key_links:
    - from: ".github/workflows/release.yml"
      to: "PyPI"
      via: "maturin publish or twine upload after wheel build"
      pattern: "pypi"
---

<objective>
Create the GitHub Actions release workflow that builds wheels and publishes to PyPI when a version tag is pushed. This completes the distribution pipeline.

Purpose: Enable one-command releases: push a `v*` tag, wheels build for all platforms, and the package appears on PyPI.
Output: `.github/workflows/release.yml` workflow file.
</objective>

<execution_context>
@$HOME/.claude/get-shit-done/workflows/execute-plan.md
@$HOME/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@.planning/PROJECT.md
@.planning/ROADMAP.md
@.planning/STATE.md
@.planning/phases/09-distribution/09-01-SUMMARY.md
@.github/workflows/ci.yml
@pyproject.toml
@Cargo.toml
</context>

<interfaces>
<!-- From Plan 01 - CI workflow structure to mirror for release builds -->
The release workflow reuses the same build matrix pattern from ci.yml:
- 4 targets: linux x86_64, linux aarch64, macos x86_64, macos arm64
- Uses PyO3/maturin-action@v1 with --release --out dist
- Uploads artifacts for collection by publish job
</interfaces>

<tasks>

<task type="auto">
  <name>Task 1: Create release workflow with PyPI publishing</name>
  <files>.github/workflows/release.yml</files>
  <read_first>.github/workflows/ci.yml, pyproject.toml, Cargo.toml</read_first>
  <action>
Create `.github/workflows/release.yml` with the following structure:

**Trigger:** `push` with tags matching `v*` (e.g., `v0.1.0`, `v1.0.0`).

**Jobs:**

1. **`build`** job (same matrix as ci.yml):
   - Matrix entries (4 targets):
     - `{os: ubuntu-latest, target: x86_64}`
     - `{os: ubuntu-latest, target: aarch64}`
     - `{os: macos-13, target: x86_64}`
     - `{os: macos-14, target: aarch64}`
   - Steps:
     - `actions/checkout@v4`
     - For aarch64 linux: `docker/setup-qemu-action@v3`
     - `PyO3/maturin-action@v1` with:
       - `command: build`
       - `args: --release --out dist`
       - `target: ${{ matrix.target }}`
       - `manylinux: auto` (for linux targets)
     - `actions/upload-artifact@v4` with:
       - `name: wheels-${{ matrix.os }}-${{ matrix.target }}`
       - `path: dist/`

2. **`sdist`** job:
   - runs-on: `ubuntu-latest`
   - Steps:
     - `actions/checkout@v4`
     - `PyO3/maturin-action@v1` with:
       - `command: sdist`
       - `args: --out dist`
     - `actions/upload-artifact@v4` with:
       - `name: sdist`
       - `path: dist/`

3. **`release`** job:
   - `needs: [build, sdist]`
   - runs-on: `ubuntu-latest`
   - `permissions:` `id-token: write` (for PyPI trusted publishing OIDC)
   - Steps:
     - `actions/download-artifact@v4` with `path: dist/` and `merge-multiple: true`
     - `PyO3/maturin-action@v1` with:
       - `command: upload`
       - `args: --non-interactive --skip-existing dist/*`
       - `env:` `MATURIN_PYPI_TOKEN: ${{ secrets.PYPI_API_TOKEN }}`
     - `softprops/action-gh-release@v2` with:
       - `files: dist/*`
       - `generate_release_notes: true`

**Important details:**
- The `release` job depends on both `build` and `sdist` completing.
- Use `secrets.PYPI_API_TOKEN` for PyPI authentication (repository secret set by user).
- Include `sdist` job so source distribution is also published alongside wheels.
- The `softprops/action-gh-release@v2` action creates a GitHub Release with all wheel files attached.
- Add `generate_release_notes: true` to auto-generate release notes from commits.
  </action>
  <verify>
    <automated>python3 -c "
import yaml
with open('.github/workflows/release.yml') as f:
    wf = yaml.safe_load(f)
on_key = wf.get(True) or wf.get('on', {})
push = on_key if isinstance(on_key, dict) and 'tags' in on_key.get('push', {}) else on_key.get('push', {})
jobs = wf.get('jobs', {})
assert 'build' in jobs, 'missing build job'
assert 'sdist' in jobs, 'missing sdist job'
assert 'release' in jobs, 'missing release job'
release = jobs['release']
needs = release.get('needs', [])
assert 'build' in needs and 'sdist' in needs, f'release needs wrong: {needs}'
# Check build matrix has 4 targets
build = jobs['build']
matrix = build.get('strategy', {}).get('matrix', {}).get('include', [])
assert len(matrix) == 4, f'expected 4 targets, got {len(matrix)}'
print('OK')
"</automated>
  </verify>
  <acceptance_criteria>
- `.github/workflows/release.yml` exists
- Workflow triggers on push of `v*` tags only
- Contains `build` job with 4 matrix entries (same targets as ci.yml)
- Contains `sdist` job that builds source distribution
- Contains `release` job that depends on build and sdist
- Release job downloads all artifacts and uploads to PyPI using MATURIN_PYPI_TOKEN secret
- Release job creates GitHub Release with wheel files attached
- Linux aarch64 matrix entry uses QEMU setup
  </acceptance_criteria>
  <done>Release workflow publishes to PyPI and creates GitHub Release when a v* tag is pushed</done>
</task>

<task type="auto">
  <name>Task 2: Add sdist build to CI workflow and verify local wheel build</name>
  <files>.github/workflows/ci.yml</files>
  <read_first>.github/workflows/ci.yml</read_first>
  <action>
1. Add an `sdist` job to ci.yml (parallel with build and test) to verify source distribution builds:
   - runs-on: `ubuntu-latest`
   - Steps:
     - `actions/checkout@v4`
     - `PyO3/maturin-action@v1` with `command: sdist` and `args: --out dist`
     - `actions/upload-artifact@v4` with name `sdist` and path `dist/`

2. Verify the local wheel build works by running:
   ```
   maturin build --release --out dist
   ```
   Then confirm the wheel file was produced in `dist/` directory and verify it can be installed:
   ```
   pip install dist/burner_redis-*.whl
   python -c "from burner_redis import BurnerRedis; print('import OK')"
   ```

This ensures the sdist job pattern is consistent between ci.yml and release.yml, and proves the wheel-build pipeline works end-to-end locally.
  </action>
  <verify>
    <automated>python3 -c "
import yaml
with open('.github/workflows/ci.yml') as f:
    wf = yaml.safe_load(f)
jobs = wf.get('jobs', {})
assert 'sdist' in jobs, 'missing sdist job in ci.yml'
assert 'test' in jobs, 'missing test job'
assert 'build' in jobs, 'missing build job'
print('OK: ci.yml has test, build, and sdist jobs')
"</automated>
  </verify>
  <acceptance_criteria>
- ci.yml contains `sdist` job alongside existing `test` and `build` jobs
- sdist job uses `PyO3/maturin-action@v1` with `command: sdist`
- Local `maturin build --release` produces a wheel file in dist/
- Wheel is installable and `from burner_redis import BurnerRedis` succeeds from the installed wheel
  </acceptance_criteria>
  <done>CI workflow validates sdist builds; local wheel build verified end-to-end</done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|----------|-------------|
| GitHub Secrets -> PyPI | PYPI_API_TOKEN used to authenticate publish |
| Tag push -> Release | Anyone with push access can trigger a release |

## STRIDE Threat Register

| Threat ID | Category | Component | Disposition | Mitigation Plan |
|-----------|----------|-----------|-------------|-----------------|
| T-09-03 | Elevation of Privilege | release.yml | mitigate | Only tag pushes trigger publish; branch protection prevents unauthorized tags |
| T-09-04 | Information Disclosure | PYPI_API_TOKEN | mitigate | Stored as GitHub encrypted secret; never printed in logs; id-token permission scoped to release job only |
| T-09-05 | Tampering | Published wheels | accept | Wheels built from tagged commit; PyPI is append-only (can't overwrite); --skip-existing prevents re-publish |
</threat_model>

<verification>
- `release.yml` is valid YAML with correct trigger (v* tags)
- Release workflow builds all 4 platforms + sdist, then publishes
- `ci.yml` now also builds sdist for validation
- Local wheel build produces installable package
</verification>

<success_criteria>
- Release workflow exists and would publish to PyPI on v* tag push
- CI workflow comprehensively validates all build artifacts (wheels + sdist)
- Local build produces a working wheel (verified by import test)
</success_criteria>

<output>
After completion, create `.planning/phases/09-distribution/09-02-SUMMARY.md`
</output>
