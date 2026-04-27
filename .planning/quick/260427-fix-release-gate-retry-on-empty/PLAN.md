---
slug: fix-release-gate-retry-on-empty
date: 2026-04-27
type: quick
priority: P2
---

# Fix release gate to retry on empty `gh run list` results

## Problem

The release workflow's `gate` job exits immediately when `gh run list --workflow=$wf --commit=$SHA` returns `[]`, even though the underlying CI run exists and succeeded. Two pushes of the v0.1.6 tag failed in ~7 seconds because of this.

Root cause: GitHub's `?head_sha=` filter on `/actions/runs` is unreliable — it sometimes returns `total_count: 0` for valid SHAs, especially shortly after run creation. The current gate logic treats an empty result as a fatal error rather than a transient condition:

```bash
if [ -z "$run" ] || [ "$run" = "null" ]; then
  echo "ERROR: no $wf run found for $GITHUB_SHA"
  exit 1   # ← bug: exits on first empty poll, never retries
fi
```

The 40-iteration polling loop only retries when status != completed, never when the run isn't found.

## Fix

Change the empty-result branch from `exit 1` to `continue` (with an informative log), and add a fallback API call that lists recent runs via `?per_page=20` and filters by `head_sha == SHA && path == .github/workflows/$wf` in jq. Track whether a run was ever found across the polling loop with `run_found=true|false`; only fail with "no run found" if all 40 iterations come up empty.

This preserves the safety semantics (still fails if CI didn't run, still fails if CI didn't succeed) while making the query robust to the GitHub API's `head_sha` filter quirk.
