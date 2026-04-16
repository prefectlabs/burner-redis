---
phase: quick-260416-hbn
plan: 01
type: execute
wave: 1
depends_on: []
files_modified:
  - .github/workflows/release.yml
autonomous: true
requirements:
  - 260416-hbn-gate
must_haves:
  truths:
    - "A `gate` job runs before `release` on every `v*` tag push"
    - "`gate` fails the workflow if CI is not `success` on `$GITHUB_SHA`"
    - "`gate` fails the workflow if pydocket-compat is not `success` on `$GITHUB_SHA`"
    - "`gate` waits (polls) up to 10 minutes if a required workflow is still in_progress/queued"
    - "`gate` fails with a clear message if no run exists for `$GITHUB_SHA` (tag-not-on-main case)"
    - "`release` only runs when `gate`, `build`, and `sdist` have all succeeded"
    - "Failure messages name the workflow and include the run URL"
    - "release.yml is valid YAML"
  artifacts:
    - path: ".github/workflows/release.yml"
      provides: "gate job + updated release.needs list"
      contains: "gate:"
  key_links:
    - from: "release job"
      to: "gate job"
      via: "`needs: [gate, build, sdist]`"
      pattern: "needs:\\s*\\[\\s*gate,\\s*build,\\s*sdist\\s*\\]"
    - from: "gate job"
      to: "GitHub API"
      via: "`gh run list --commit=$GITHUB_SHA --workflow=<name>`"
      pattern: "gh run list.*--commit.*--workflow"
---

<objective>
Add a `gate` job to `.github/workflows/release.yml` that blocks the `release` job from publishing to PyPI until both `ci.yml` and `pydocket-compat.yml` have a `success` run on the tag's commit SHA.

Purpose: Prevent accidental publication of releases where CI or the pydocket-compat downstream-consumer check is failing or still running.

Output: Updated `.github/workflows/release.yml` with a new `gate` job and `release.needs` expanded to include `gate`.
</objective>

<execution_context>
@$HOME/.claude/get-shit-done/workflows/execute-plan.md
@$HOME/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@.planning/quick/260416-hbn-add-test-passing-gates-to-the-release-wo/260416-hbn-CONTEXT.md
@.github/workflows/release.yml
@.github/workflows/ci.yml
@.github/workflows/pydocket-compat.yml

<interfaces>
<!-- Key facts extracted from the codebase the executor needs. No exploration required. -->

**Current `.github/workflows/release.yml` structure (as of plan-time):**

Jobs (in order): `build` (matrix wheels), `sdist`, `release` (publish to PyPI + GH release).

`release` job currently declares: `needs: [build, sdist]`.

Trigger: `on: push: tags: ['v*']`.

**Workflows being gated on:**

- `ci.yml` — workflow file name used with `--workflow=ci.yml`. Jobs include `test` (matrix), `integration-test`, `build`, `sdist`, `no-redis-smoke-test`. Per CONTEXT.md, we gate on the workflow, not individual jobs.
- `pydocket-compat.yml` — workflow file name used with `--workflow=pydocket-compat.yml`. Single `pydocket-compat` job (matrix over Python versions).

**`gh` CLI behavior (GitHub-hosted runner, `gh` pre-installed):**

- `gh run list --workflow=<file.yml> --commit=<sha> --limit=1 --json status,conclusion,url -q '.[0]'` returns a single JSON object for the latest run on that SHA, or empty string / "null" if none.
- `jq` is pre-installed on `ubuntu-latest`.
- Requires `GH_TOKEN` env var; `${{ github.token }}` is sufficient for read-only queries on the same repo.

**`$GITHUB_SHA` in a tag push:** equals the commit the tag points at. If that commit was never pushed to main, `ci.yml` / `pydocket-compat.yml` will have no run for it (both workflows trigger on `push: branches: [main]`, not on tag pushes). This is the tag-not-on-main edge case — the gate must fail with a clear message in that situation.
</interfaces>
</context>

<tasks>

<task type="auto">
  <name>Task 1: Add gate job to release.yml and wire release.needs</name>
  <files>.github/workflows/release.yml</files>
  <action>
Edit `.github/workflows/release.yml` to make exactly two changes:

**Change A — Insert a new `gate` job as the FIRST job under `jobs:` (before `build`).**

The `gate` job YAML to insert (match the indentation of the existing `build:` / `sdist:` / `release:` siblings — 2-space top-level jobs, 4-space for job fields):

```yaml
  gate:
    runs-on: ubuntu-latest
    env:
      GH_TOKEN: ${{ github.token }}
    steps:
      - name: Wait for required workflows to succeed on ${{ github.sha }}
        run: |
          set -u
          required_workflows=(ci.yml pydocket-compat.yml)
          for wf in "${required_workflows[@]}"; do
            echo "::group::Checking $wf on $GITHUB_SHA"
            status=""
            conclusion=""
            url=""
            for i in $(seq 1 40); do
              run=$(gh run list \
                --workflow="$wf" \
                --commit="$GITHUB_SHA" \
                --limit=1 \
                --json status,conclusion,url \
                -q '.[0]')
              if [ -z "$run" ] || [ "$run" = "null" ]; then
                echo "ERROR: no $wf run found for $GITHUB_SHA"
                echo "       was this commit pushed to main before tagging?"
                exit 1
              fi
              status=$(echo "$run" | jq -r .status)
              conclusion=$(echo "$run" | jq -r .conclusion)
              url=$(echo "$run" | jq -r .url)
              if [ "$status" = "completed" ]; then
                if [ "$conclusion" = "success" ]; then
                  echo "OK: $wf succeeded on $GITHUB_SHA ($url)"
                  break
                fi
                echo "ERROR: $wf did not succeed on $GITHUB_SHA: conclusion=$conclusion (see $url)"
                exit 1
              fi
              echo "  $wf status=$status — waiting 15s... ($i/40)"
              sleep 15
            done
            if [ "$status" != "completed" ]; then
              echo "ERROR: $wf did not complete within 10 minutes on $GITHUB_SHA (last status=$status, see $url)"
              exit 1
            fi
            echo "::endgroup::"
          done
          echo "All required workflows green on $GITHUB_SHA."
```

Notes on the gate script (do not remove or reword — these are locked in CONTEXT.md):
- Explicit list `(ci.yml pydocket-compat.yml)` — do NOT auto-discover workflows (per CONTEXT.md decision).
- Poll interval 15s, max 40 iterations = 10 minute timeout (per CONTEXT.md decision).
- No run for SHA → fail with tag-not-on-main message (per CONTEXT.md Claude's-discretion item).
- Every error path names the workflow and (when a run exists) includes the run URL (per CONTEXT.md specifics).

**Change B — Update the `release` job's `needs:` line.**

Exact diff:

```
-  release:
-    needs: [build, sdist]
+  release:
+    needs: [gate, build, sdist]
```

(That is the ONLY change to the `release` job — do not touch its `runs-on`, `environment`, `permissions`, or `steps`.)

**Do not change anything else** — the `build` and `sdist` jobs stay exactly as they are. Do not add `needs: gate` to `build` or `sdist`; they can run in parallel with `gate` (CONTEXT.md wires `gate` as a `release` dependency only, and there is no reason to block the wheel/sdist build on the gate — if the gate fails, `release` never runs and the artifacts are discarded).

After editing, run the YAML syntax check specified in `<verify>`.
  </action>
  <verify>
    <automated>python -c "import yaml; d = yaml.safe_load(open('.github/workflows/release.yml')); assert 'gate' in d['jobs'], 'gate job missing'; assert d['jobs']['release']['needs'] == ['gate', 'build', 'sdist'], f\"release.needs wrong: {d['jobs']['release']['needs']}\"; assert list(d['jobs'].keys())[0] == 'gate', f\"gate is not first job: {list(d['jobs'].keys())}\"; assert 'ci.yml' in d['jobs']['gate']['steps'][0]['run'], 'ci.yml not referenced in gate'; assert 'pydocket-compat.yml' in d['jobs']['gate']['steps'][0]['run'], 'pydocket-compat.yml not referenced in gate'; assert d['jobs']['gate']['env']['GH_TOKEN'] == '${{ github.token }}', 'GH_TOKEN env missing or wrong'; print('OK')"</automated>
  </verify>
  <done>
- `.github/workflows/release.yml` parses as valid YAML.
- `jobs.gate` exists and is the first job under `jobs:`.
- `jobs.gate.env.GH_TOKEN` is `${{ github.token }}`.
- `jobs.gate.steps[0].run` script references both `ci.yml` and `pydocket-compat.yml`, polls with 15s sleep and 40 iterations, fails on missing run / non-success conclusion / timeout, and each failure message includes the workflow name and (where available) the run URL.
- `jobs.release.needs` is exactly `[gate, build, sdist]`.
- `jobs.build` and `jobs.sdist` are unchanged.
- No other job (`build`, `sdist`, `release`'s steps) is modified.
  </done>
</task>

</tasks>

<verification>
Overall phase checks (run after task 1 completes):

1. YAML validity:
   ```bash
   python -c "import yaml; yaml.safe_load(open('.github/workflows/release.yml'))"
   ```
2. Gate job present and first:
   ```bash
   python -c "import yaml; d = yaml.safe_load(open('.github/workflows/release.yml')); print(list(d['jobs'].keys()))"
   # Expected: ['gate', 'build', 'sdist', 'release']
   ```
3. release.needs wiring:
   ```bash
   python -c "import yaml; d = yaml.safe_load(open('.github/workflows/release.yml')); print(d['jobs']['release']['needs'])"
   # Expected: ['gate', 'build', 'sdist']
   ```
4. `gh` command present in gate script:
   ```bash
   grep -n "gh run list" .github/workflows/release.yml
   # Expected: one hit inside the gate job
   ```

Runtime behavior cannot be fully verified locally — it is exercised on the next `v*` tag push. Acceptable because the edit is config-only and the next release will surface any issue.
</verification>

<success_criteria>
- release.yml parses as valid YAML (automated check passes).
- `jobs.gate` is defined, runs on `ubuntu-latest`, has `GH_TOKEN: ${{ github.token }}`, and polls both `ci.yml` and `pydocket-compat.yml` with a 15s/40-iter loop.
- `jobs.release.needs` includes `gate` as the first entry: `[gate, build, sdist]`.
- No other jobs or steps are modified.
- `python -c "import yaml; ..."` validation command from Task 1's `<verify>` returns `OK`.
</success_criteria>

<output>
After completion, create `.planning/quick/260416-hbn-add-test-passing-gates-to-the-release-wo/260416-hbn-SUMMARY.md` describing:
- What was added (gate job) and what was wired (`release.needs`).
- The exact list of required workflows (`ci.yml`, `pydocket-compat.yml`).
- Polling parameters (15s × 40 = 10 min).
- Note that runtime behavior will be exercised on the next `v*` tag push.
</output>
