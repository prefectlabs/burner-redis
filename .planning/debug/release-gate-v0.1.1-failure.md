---
status: awaiting_human_verify
trigger: "release-gate-v0.1.1-failure: gate job in release.yml failed on first real use (v0.1.1 tag)"
created: 2026-04-16T00:00:00Z
updated: 2026-04-16T18:35:00Z
---

## Current Focus

hypothesis: (CONFIRMED) Gate job had no repo context for gh. Added -R "$GITHUB_REPOSITORY" to gh run list in release.yml. Committed as c400620 and pushed to main.
test: next v* tag push — gate should now resolve the repo and find the pre-existing green CI + pydocket-compat runs on that SHA.
expecting: Gate succeeds, build+sdist+release jobs proceed, wheels publish to PyPI.
next_action: awaiting user choice on re-release strategy (see checkpoint — v0.1.2 recommended).

## Symptoms

expected: |
  Tag v0.1.1 at commit a234d33 should have proceeded through the gate.
  - CI run #24525114223 was success on a234d33
  - pydocket-compat run #24525114212 was success on a234d33
actual: |
  Gate job failed.
  Failure URL: https://github.com/prefectlabs/burner-redis/actions/runs/24526332891/job/71697447373
errors: "Unknown — must fetch via gh run view 24526332891 --log-failed"
reproduction: Push a v* tag → gate job runs first → exited non-zero
started: First real use = v0.1.1 tag push
timeline: |
  - Gate added today in commits dd34627 (code) and a234d33 (docs)
  - release.yml subsequently modified by linter (intentional, do not revert)
  - First real use = v0.1.1 tag push = failure

## Eliminated

- hypothesis: CI re-triggered by tag push, gate saw in_progress run with --limit=1
  evidence: ci.yml has `on: push: branches: [main]` (not tags) and `on: pull_request`. pydocket-compat.yml same. Tag push does NOT trigger fresh CI runs. Runs 24525114223 and 24525114212 remained the only runs on SHA a234d33.
  timestamp: 2026-04-16T18:30:00Z

- hypothesis: gh run list --commit filters by branch or ref
  evidence: `gh run list --commit=<full-sha> -R prefectlabs/burner-redis` returns both CI and pydocket-compat runs on headBranch=main. Flag works as documented; it's not the failure point.
  timestamp: 2026-04-16T18:30:00Z

- hypothesis: "no run found" path hit with a234d33
  evidence: Log output shows `gh` wrote "failed to determine base repo" BEFORE any script echo. The script never reached the `[ -z "$run" ]` check with a meaningful comparison — `gh` exited non-zero, command substitution propagated, and bash -e (GitHub Actions default shell) terminated the job.
  timestamp: 2026-04-16T18:30:00Z

## Evidence

- timestamp: 2026-04-16T18:30:00Z
  checked: gh run view --job 71697447373 --log-failed
  found: |
    First error line in the step output:
      "failed to determine base repo: failed to run git: fatal: not a git repository (or any of the parent directories): .git"
    Followed immediately by "##[error]Process completed with exit code 1."
  implication: gh CLI cannot determine the target repo. Gate job has no actions/checkout step, and `gh` resolves the repo from the cwd's git remote by default.

- timestamp: 2026-04-16T18:30:00Z
  checked: .github/workflows/release.yml gate job (lines 9-55)
  found: |
    - env: GH_TOKEN: ${{ github.token }}  (auth set correctly)
    - No `- uses: actions/checkout@v4` step
    - `gh run list --workflow=... --commit=$GITHUB_SHA --limit=1 --json ...`
    - No `-R` flag anywhere
  implication: The script depends on `gh` auto-detecting the repo from a local clone that doesn't exist. Fix is either add checkout or add `-R ${{ github.repository }}`. Second is faster/cheaper.

- timestamp: 2026-04-16T18:30:00Z
  checked: .github/workflows/ci.yml and .github/workflows/pydocket-compat.yml `on:` triggers
  found: |
    Both: `on: push: branches: [main]` + `on: pull_request: branches: [main]`
    Neither triggers on tags.
  implication: Tag push does not create new CI or pydocket-compat runs. The gate only needs to find the PRE-EXISTING runs from when a234d33 was pushed to main. No tag-race concern at all — poll loop is still useful for the rare case of someone tagging seconds after pushing to main, before the triggered runs have registered.

- timestamp: 2026-04-16T18:30:00Z
  checked: gh run list --commit=a234d33 -R prefectlabs/burner-redis (short SHA)
  found: "[]"  (empty)
  implication: `gh run list --commit` requires the FULL SHA. Short SHA silently returns empty. Not a failure mode here — $GITHUB_SHA in Actions is always the full SHA — but worth noting as a pitfall for manual reproduction.

- timestamp: 2026-04-16T18:30:00Z
  checked: gh run view 24525114223 and 24525114212 with --json headSha
  found: |
    CI run 24525114223: conclusion=success, headSha=a234d33dbbdeeea94933a8dc3890161d7f5bf063, headBranch=main
    pydocket-compat run 24525114212: conclusion=success, headSha=a234d33dbbdeeea94933a8dc3890161d7f5bf063, headBranch=main
  implication: Both required runs exist and are green on the exact SHA. Once gh can see them, gate will pass.

## Resolution

root_cause: |
  The `gate` job in .github/workflows/release.yml invokes `gh run list` without either
  (a) a prior `actions/checkout` step, or
  (b) a `-R <owner>/<repo>` flag.
  `gh` defaults to detecting the target repo from the cwd's git remote, and the runner's
  working directory is not a git clone at that point. `gh` exits non-zero with
  "failed to determine base repo", and because GitHub Actions runs bash steps with `-e`,
  the failed command substitution terminates the step immediately — before any of the
  script's own error-handling logic (the "no run found" echo) can run.

fix: |
  Add `-R ${{ github.repository }}` to the `gh run list` invocation in the gate job.
  This avoids cloning the repo (the gate doesn't need source — only the GitHub API).

verification: |
  Local simulation from a non-git-repo cwd with GITHUB_REPOSITORY set:
  - gh run list -R "$GITHUB_REPOSITORY" --workflow=ci.yml --commit=<full-sha> --limit=1 ...
    → {"conclusion":"success","status":"completed","url":".../24525114223"}
  - gh run list -R "$GITHUB_REPOSITORY" --workflow=pydocket-compat.yml --commit=<full-sha> --limit=1 ...
    → {"conclusion":"success","status":"completed","url":".../24525114212"}
  - Bogus SHA exits 0 with empty stdout → gate prints "no ci.yml run found" and exits 1. Security property preserved.
  - YAML parses cleanly via pyyaml.
  Full CI verification will happen when the next v* tag is pushed (see checkpoint).
files_changed:
  - .github/workflows/release.yml
