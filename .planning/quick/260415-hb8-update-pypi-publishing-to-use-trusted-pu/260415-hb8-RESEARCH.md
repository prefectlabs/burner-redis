# Quick Task: Update PyPI Publishing to Trusted Publishers (OIDC)

**Researched:** 2026-04-15
**Domain:** GitHub Actions CI/CD, PyPI publishing
**Confidence:** HIGH

## Summary

The current release workflow uses `PyO3/maturin-action@v1` with `command: upload` and `MATURIN_PYPI_TOKEN` secret for PyPI publishing. There are two viable paths to migrate to OIDC trusted publishing:

1. **Option A (Minimal change):** Keep `maturin upload` but remove `MATURIN_PYPI_TOKEN` -- maturin natively detects GitHub Actions OIDC and performs the token exchange automatically.
2. **Option B (Recommended):** Replace `maturin upload` with `pypa/gh-action-pypi-publish@release/v1` -- the official PyPA action that also generates Sigstore attestations by default (since v1.11.0), providing supply chain provenance at zero extra cost.

**Primary recommendation:** Use Option B (`pypa/gh-action-pypi-publish`) for attestation benefits and because it is the PyPA-blessed standard for trusted publishing. The workflow change is small and well-documented.

## How Maturin Native OIDC Works (Option A)

Maturin's credential resolution in `upload.rs` follows this priority [VERIFIED: github.com/PyO3/maturin/blob/main/src/upload.rs]:

1. `MATURIN_PYPI_TOKEN` environment variable (highest priority)
2. OIDC token exchange (if `GITHUB_ACTIONS`, `ACTIONS_ID_TOKEN_REQUEST_TOKEN`, and `ACTIONS_ID_TOKEN_REQUEST_URL` are set)
3. `.pypirc` file
4. CLI username/password
5. Keyring
6. Interactive prompt

**To enable:** Simply remove `MATURIN_PYPI_TOKEN` from the env block. Maturin will auto-detect GitHub Actions OIDC and print "Using trusted publisher for upload". The `id-token: write` permission (already present in the current workflow) provides the necessary environment variables.

**Downside:** No Sigstore attestation generation. Maturin handles the upload only.

## How pypa/gh-action-pypi-publish Works (Option B)

The official PyPA publishing action (v1.14.0 as of 2026-04-07) [VERIFIED: github.com/pypa/gh-action-pypi-publish]:

- Handles OIDC token exchange with PyPI automatically when `id-token: write` is set and no username/password is provided
- Generates and uploads PEP 740 Sigstore attestations by default (since v1.11.0) -- no extra config needed
- Uses `twine` under the hood for metadata validation and upload
- Expects distribution files in `dist/` by default (configurable via `packages-dir` input)

**Required permissions:**
```yaml
permissions:
  id-token: write   # Mandatory for OIDC token exchange
```

For attestations, the action also needs `attestations: write` permission on the GitHub repository (this is the GitHub Attestations API, separate from OIDC). However, since attestations are opt-out (enabled by default), and the action handles this gracefully, no additional explicit permission is typically needed beyond `id-token: write`.

## Recommended Workflow Change

### Current (token-based)
```yaml
release:
  needs: [build, sdist]
  runs-on: ubuntu-latest
  permissions:
    id-token: write
    contents: write
  steps:
    - name: Download all artifacts
      uses: actions/download-artifact@v4
      with:
        path: dist/
        merge-multiple: true

    - name: Publish to PyPI
      uses: PyO3/maturin-action@v1
      with:
        command: upload
        args: --non-interactive --skip-existing dist/*
      env:
        MATURIN_PYPI_TOKEN: ${{ secrets.PYPI_API_TOKEN }}

    - name: Create GitHub Release
      uses: softprops/action-gh-release@v2
      with:
        files: dist/*
        generate_release_notes: true
```

### Proposed (OIDC trusted publisher)
```yaml
release:
  needs: [build, sdist]
  runs-on: ubuntu-latest
  environment:
    name: pypi
    url: https://pypi.org/p/burner-redis
  permissions:
    id-token: write
    contents: write
  steps:
    - name: Download all artifacts
      uses: actions/download-artifact@v4
      with:
        path: dist/
        merge-multiple: true

    - name: Publish to PyPI
      uses: pypa/gh-action-pypi-publish@release/v1

    - name: Create GitHub Release
      uses: softprops/action-gh-release@v2
      with:
        files: dist/*
        generate_release_notes: true
```

Key changes:
1. Replace `PyO3/maturin-action@v1` upload step with `pypa/gh-action-pypi-publish@release/v1`
2. Remove the `MATURIN_PYPI_TOKEN` env block entirely
3. Add `environment: name: pypi` (optional but strongly recommended by PyPI docs -- adds a deployment gate in GitHub)
4. The action reads from `dist/` by default, which matches the `download-artifact` output path

## PyPI-Side Configuration Required

Before the first OIDC release, the project owner must configure the trusted publisher on PyPI [CITED: docs.pypi.org/trusted-publishers/adding-a-publisher/]:

1. Go to https://pypi.org/manage/project/burner-redis/settings/publishing/
2. Add a new GitHub Actions publisher with:
   - **Owner:** `prefectlabs`
   - **Repository:** `burner-redis`
   - **Workflow name:** `release.yml`
   - **Environment name:** `pypi` (if using the `environment` block; leave blank if not)
3. Save

If the package has not been published to PyPI yet, you can configure a "pending publisher" at https://pypi.org/manage/account/publishing/ which pre-authorizes the first upload via OIDC.

## GitHub-Side Configuration (Optional but Recommended)

Create a GitHub Actions environment named `pypi` in the repository settings:

1. Go to Settings > Environments > New environment
2. Name it `pypi`
3. Optionally add protection rules (e.g., require approval for releases, restrict to `main` branch or tag patterns)

This provides an extra layer of security -- even if the workflow file is modified in a branch, the environment protection rules prevent unauthorized publishing.

## Common Pitfalls

### Pitfall 1: MATURIN_PYPI_TOKEN Still Set
**What goes wrong:** If `MATURIN_PYPI_TOKEN` is still in the environment (even as an empty string), maturin will try to use it instead of OIDC. The `pypa/gh-action-pypi-publish` approach avoids this entirely since it doesn't use maturin for the upload step.
**How to avoid:** Completely remove the env block, do not just comment it out.

### Pitfall 2: Workflow Filename Mismatch
**What goes wrong:** The trusted publisher configuration on PyPI must exactly match the workflow filename. If the file is `release.yml` but you entered `Release.yml` on PyPI, it will fail with "invalid-publisher" error.
**How to avoid:** Use exact filename match: `release.yml`.

### Pitfall 3: Environment Name Mismatch
**What goes wrong:** If the PyPI trusted publisher is configured with environment name `pypi` but the workflow does not specify `environment: name: pypi`, the OIDC claim will not match.
**How to avoid:** Either use the environment block in the workflow AND set the environment name on PyPI, or leave both blank.

### Pitfall 4: dist/ Directory Structure
**What goes wrong:** `pypa/gh-action-pypi-publish` expects wheel and sdist files directly in `dist/`, not in subdirectories.
**How to avoid:** The current workflow already uses `merge-multiple: true` on `actions/download-artifact@v4`, which merges all artifact contents into a flat `dist/` directory. This is correct.

### Pitfall 5: `skip-existing` Behavior
**What goes wrong:** The current workflow uses `--skip-existing` with maturin. If you need this behavior with the PyPA action, you must set `skip-existing: true` as an input.
**How to avoid:** Add `with: skip-existing: true` if re-uploads of the same version should be tolerated. For tag-triggered releases this is usually unnecessary since each tag is a new version.

## Attestation Benefits (Free with Option B)

Using `pypa/gh-action-pypi-publish` automatically provides [CITED: blog.pypi.org/posts/2024-11-14-pypi-now-supports-digital-attestations/]:

- **PEP 740 digital attestations** signed with Sigstore via the workflow's OIDC identity
- **Verifiable provenance** linking each published file to the exact GitHub commit, workflow, and repository
- **Transparency log entries** in Sigstore's Rekor for independent audit
- **No configuration needed** -- enabled by default since v1.11.0

To opt out (not recommended): set `attestations: false` in the action inputs.

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | Package name on PyPI is `burner-redis` | PyPI-Side Configuration | Wrong URL for trusted publisher setup; user should verify actual package name |
| A2 | The `environment: pypi` block works with tag-triggered workflows | Recommended Workflow | If GitHub requires environment to be created first, the publish step would fail until environment exists |

## Sources

### Primary (HIGH confidence)
- [PyO3/maturin upload.rs source](https://github.com/PyO3/maturin/blob/main/src/upload.rs) -- OIDC detection logic verified
- [pypa/gh-action-pypi-publish README](https://github.com/pypa/gh-action-pypi-publish) -- v1.14.0, attestations default-on
- [Maturin distribution docs](https://www.maturin.rs/distribution.html) -- trusted publisher instructions

### Secondary (MEDIUM confidence)
- [PyPI trusted publishers overview](https://docs.pypi.org/trusted-publishers/) -- setup steps
- [GitHub OIDC for PyPI docs](https://docs.github.com/actions/deployment/security-hardening-your-deployments/configuring-openid-connect-in-pypi) -- permissions reference
- [PyPI attestations blog post](https://blog.pypi.org/posts/2024-11-14-pypi-now-supports-digital-attestations/) -- attestation benefits
