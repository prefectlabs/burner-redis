---
slug: fix-release-gate-retry-on-empty
date: 2026-04-27
status: complete
commit: 2776590
priority: P2
---

# Fix release gate to retry on empty `gh run list` results

## What changed

`.github/workflows/release.yml` gate step:

- Empty-result branch now retries with an informative log instead of `exit 1` on the first poll.
- Added a fallback `gh api repos/.../actions/runs?per_page=20` call that filters by `head_sha + path` in jq when the gh CLI shorthand (`gh run list --workflow=$wf --commit=$SHA`) returns nothing.
- New `run_found` flag tracks whether any run was indexed across the 40-iteration loop. Only fails with "no run found" if all 40 iterations come up empty (10-minute ceiling preserved).

## Why

Two v0.1.6 tag pushes failed in ~7 seconds because `gh run list --commit=$SHA` returned `[]` for a SHA where CI had clearly succeeded. Confirmed via direct API: `?head_sha=` filter returned `total_count: 0` for the same SHA where `gh api ?per_page=20` and manual jq filter found 6 runs at that SHA. The GitHub API's `head_sha` filter is unreliable around fresh runs; the gate's old logic treated this transient state as a fatal error.

## Outcome

Release run `25008872546` on commit `2776590` (now the v0.1.6 tag commit) — all 12 jobs green:

- verify-version ✓
- gate ✓ (the actual fix being validated — gate completed and passed)
- sdist ✓
- 8 wheel builds ✓ (manylinux x86_64/aarch64, musllinux x86_64/aarch64, macOS x86_64/arm64, Windows x86_64/arm64)
- release ✓ (PyPI publish + GitHub Release created)

GitHub Release `v0.1.6` published 2026-04-27T17:11:49Z with 18 assets.

## Lesson

Polling loops that gate on external state must distinguish "not found yet" from "definitively absent." Treating an empty response as fatal is brittle; the loop body itself should be idempotent and retry until either the data appears, contradicts itself, or the timeout expires.
