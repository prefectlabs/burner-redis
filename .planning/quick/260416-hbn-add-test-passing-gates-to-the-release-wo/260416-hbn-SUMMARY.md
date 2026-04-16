---
quick_id: 260416-hbn
completed: 2026-04-16
commits:
  - dd34627 ci(quick-260416-hbn): gate release on CI + pydocket-compat green
files_modified:
  - .github/workflows/release.yml
---

# Quick 260416-hbn: Add test-passing gates to the release workflow

Prevents PyPI publish + GitHub Release creation until CI and pydocket-compat
are both `success` on the tag's commit SHA.

## What shipped

New `gate` job in `.github/workflows/release.yml`, placed first. Polls
`gh run list --commit=$GITHUB_SHA` for `ci.yml` and `pydocket-compat.yml`:

- Poll interval: 15 seconds
- Max iterations: 40 (10-minute ceiling)
- On `completed: success` → continue to next workflow
- On `completed: failure|cancelled|timed_out` → fail fast with the run URL in
  the error message
- On `status: in_progress|queued` → sleep and retry
- If no run exists for the SHA (tag-not-on-main edge case) → fail with
  "was this commit pushed to main before tagging?"

The `release` job's `needs` was widened from `[build, sdist]` to
`[gate, build, sdist]`, so build/sdist still run in parallel with the gate
but PyPI publishing waits on all three.

Uses `GH_TOKEN: ${{ github.token }}` — no PAT required for same-repo
workflow-run reads.

## Why this shape

All three gating decisions were locked in CONTEXT.md before planning:

1. **Scope:** CI + pydocket-compat (explicit list — not auto-discovery of
   future workflows, to keep release behavior predictable).
2. **Mechanism:** Query prior runs via `gh api` rather than re-run tests
   inline. Fast (seconds vs. 5–10 min), no YAML duplication. Safe because
   tag commits reach main before being tagged in this repo's workflow.
3. **Race handling:** Poll with 10-min timeout so "merge to main, push tag
   immediately" still works when CI is mid-run.

## Verification

PyYAML assertions pass:

- Jobs order: `['gate', 'build', 'sdist', 'release']`
- `release.needs == ['gate', 'build', 'sdist']`
- `ci.yml` and `pydocket-compat.yml` both referenced in the gate's bash
- `GH_TOKEN: ${{ github.token }}` present

Runtime validation will occur on the next `v*` tag push. Cannot be
rehearsed locally because the gate relies on real workflow runs on a
specific SHA.

## Downstream impact

Any release where CI or pydocket-compat isn't green on the tagged commit
will fail at the `gate` job, before wheels are built and before PyPI is
touched. No partial-publish risk.

## Self-Check: PASSED

- One commit matches plan (dd34627).
- Only `.github/workflows/release.yml` modified.
- YAML structure verified.
