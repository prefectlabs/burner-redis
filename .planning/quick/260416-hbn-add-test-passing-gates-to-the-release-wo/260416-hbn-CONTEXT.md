---
name: 260416-hbn context
description: Discussion decisions for adding test-passing gates to release.yml
type: quick-task-context
---

# Quick Task 260416-hbn: Add test-passing gates to the release workflow - Context

**Gathered:** 2026-04-16
**Status:** Ready for planning

<domain>
## Task Boundary

Add a gate job to `.github/workflows/release.yml` that runs before the existing `release` job. The gate must confirm that CI and pydocket-compat are both green on the tag's commit SHA; if either is failing or not yet complete, block the release.

Scope is limited to `.github/workflows/release.yml`. Do not edit ci.yml or pydocket-compat.yml.
</domain>

<decisions>
## Implementation Decisions

### Gate scope
- **CI + pydocket-compat** — both workflows must be `success` on `$GITHUB_SHA` before the release job runs.
- CI already transitively covers `no-redis-smoke-test` since that's a job inside ci.yml; we gate on the workflow, not on individual jobs.
- pydocket-compat is included because it's our main downstream-consumer signal. Treating it as release-blocking matches the "release confidence" goal.
- Do NOT make the gate auto-pick-up future workflows — listing `[ci.yml, pydocket-compat.yml]` explicitly keeps release behaviour predictable when new workflows are added.

### Gating mechanism
- **Query prior runs via `gh api` / `gh run list`**, not inline re-run.
- New `gate` job runs first; `release` gets `needs: [gate, build, sdist]`.
- Uses `GH_TOKEN: ${{ github.token }}` (no PAT needed — the default token can read workflow runs in the same repo).
- Looks up the latest run per workflow for `--commit=$GITHUB_SHA`. Fails if conclusion is anything other than `success`.
- Rejected: inline re-run via reusable workflow. Slow (5–10 min added per release), duplicates YAML, and for our repo CI has already run on the merged main commit by the time a tag is pushed.
- Rejected: `workflow_run` trigger. More complex event logic; harder to debug failure modes; tag-driven releases don't map cleanly.

### Race handling (tag pushed while CI still running)
- **Poll and wait** with a ~10 minute timeout.
- Inner loop: if status is `in_progress` or `queued`, sleep and retry; if `completed` with `success` → pass; if `completed` with `failure`/`cancelled` → fail fast; if still not completed after timeout → fail with a clear message.
- Rationale: developer merges to main → tag is pushed shortly after (standard flow). CI may be in_progress when the gate starts. Polling handles this without requiring the developer to wait manually.
- Per-workflow: poll each workflow independently; short-circuit on any failure.

### Tag-not-on-main edge case (Claude's discretion)
- If a tag points at a commit that was NEVER pushed to a branch with CI configured, `gh run list --commit=$SHA` returns no runs. Treat that as a release-blocker ("no CI run found for $SHA — was this commit pushed to main before tagging?"). Don't implicitly allow un-CI'd releases.

</decisions>

<specifics>
## Specific Ideas

- Poll interval: 15s. Timeout: 10 minutes (40 iterations). These are round numbers that fit the typical CI duration (~2-3 min) and rarely-slow integration-test case.
- Error messages should name the specific workflow and its run URL so a failed release tells the user exactly where to look: `"CI workflow did not succeed on <SHA>: status=completed conclusion=failure (see <run_url>)"`.
- Single bash loop over `[ci.yml, pydocket-compat.yml]` keeps the YAML compact (~25 lines).

</specifics>

<canonical_refs>
## Canonical References

- `.github/workflows/release.yml` — target of edits (current state: builds wheels matrix, sdist, then `release` job publishes to PyPI + creates GH release; triggered on `v*` tags).
- `.github/workflows/ci.yml` — must be green (includes test × 5 Python, build × 4 platforms, integration-test, sdist, no-redis-smoke-test).
- `.github/workflows/pydocket-compat.yml` — must be green (pydocket test suite × 5 Python versions).
- GitHub `gh run list` docs: filters include `--commit` (match by SHA) and `--workflow` (filename-based). `--json status,conclusion,url` exposes what we need.

</canonical_refs>
