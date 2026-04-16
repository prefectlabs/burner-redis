---
phase: quick-260416-gqd
plan: 01
type: execute
wave: 1
depends_on: []
files_modified:
  - .github/workflows/ci.yml
autonomous: true
requirements:
  - GUARD-01
must_haves:
  truths:
    - "CI runs a job that installs only the built wheel (no dev extras, no redis package) and imports all public symbols"
    - "Job fails if any shipped module contains an unguarded `import redis` statement"
    - "Job passes on a clean push with current guarded-import code"
  artifacts:
    - path: ".github/workflows/ci.yml"
      provides: "no-redis-smoke-test job definition"
      contains: "no-redis-smoke-test"
  key_links:
    - from: ".github/workflows/ci.yml (no-redis-smoke-test)"
      to: "built wheel from dist/"
      via: "uv pip install dist/*.whl into a fresh venv with no extras"
      pattern: "uv pip install .*\\.whl"
    - from: "smoke-test python -c invocation"
      to: "python/burner_redis/__init__.py public exports"
      via: "from burner_redis import BurnerRedis, Pipeline, PubSub, Lock, LockError, ResponseError, NoScriptError"
      pattern: "from burner_redis import"
---

<objective>
Add a CI guard job to `.github/workflows/ci.yml` that proves the published wheel does not carry an accidental hard dependency on the `redis` package.

Purpose: `python/burner_redis/__init__.py` and `python/burner_redis/lock.py` contain `import redis` inside `try/except (ImportError, AttributeError)` blocks so the library can subclass `redis.exceptions.*` when available. Since commit 7d6240f added `redis` to `[project.optional-dependencies].dev`, every existing CI job now has `redis` installed in its environment — so a bare, unguarded `import redis` sneaking into shipped code would silently pass CI but break real user installs (`pip install burner-redis` with no Redis present). This job catches that regression.

Output: A new `no-redis-smoke-test` job in the existing `ci.yml` workflow that builds the wheel, installs only the wheel into a fresh venv with zero extras, and imports every public symbol. The job fails fast (<1 minute) if `import redis` is ever shipped unguarded.
</objective>

<execution_context>
@$HOME/.claude/get-shit-done/workflows/execute-plan.md
@$HOME/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@.planning/STATE.md
@.github/workflows/ci.yml
@pyproject.toml
@python/burner_redis/__init__.py

<interfaces>
<!-- Existing CI workflow conventions (from .github/workflows/ci.yml) the new job MUST match exactly -->

Action versions in use:
- `actions/checkout@v4`
- `actions/setup-python@v5`
- `dtolnay/rust-toolchain@stable`
- `astral-sh/setup-uv@v4`
- `PyO3/maturin-action@v1`

Step naming conventions (verbatim from existing jobs):
- `Set up Python <version>` (e.g., "Set up Python 3.12")
- `Install Rust toolchain`
- `Install uv`
- `Create virtualenv`
- `Install dependencies`
- `Build with maturin`

Job naming: hyphenated lowercase (`test`, `integration-test`, `build`, `sdist`).

Triggers (shared workflow-level, already present at top of file):
```yaml
on:
  push:
    branches: [main]
  pull_request:
    branches: [main]
```
No per-job `on:` override needed — new job inherits workflow triggers. (Note: the existing `integration-test` job has `if: github.event_name == 'push'` to skip on PRs. The smoke-test should run on BOTH push and PR — it's a fast signal — so do NOT add that `if:` guard.)

Indent style: 2 spaces, YAML block-style. Jobs sit at column 2 under `jobs:`; steps are a list under `steps:` with `- uses:` or `- name:` at column 6.

Public exports to import in smoke test (from `python/burner_redis/__init__.py` `__all__`):
`BurnerRedis, Lock, LockError, NoScriptError, Pipeline, PubSub, ResponseError, Script`

Build pattern — existing jobs use `maturin develop --release` (editable install from source tree), but that's unsuitable here: we need a CLEAN wheel install to simulate a real user. The `build` job already demonstrates the correct pattern using `PyO3/maturin-action@v1` with `command: build` and `args: --release --out dist`. Use that same action for consistency — it handles Rust toolchain setup internally, so a separate `Install Rust toolchain` step is NOT needed when using `maturin-action`.

Python version: pick 3.12 (middle-ground supported version — matches `integration-test` job).
</interfaces>
</context>

<tasks>

<task type="auto">
  <name>Task 1: Add no-redis-smoke-test job to ci.yml</name>
  <files>.github/workflows/ci.yml</files>
  <action>
Append a new job named `no-redis-smoke-test` to `.github/workflows/ci.yml`, after the existing `sdist` job (keep jobs in the existing top-to-bottom order: `test`, `integration-test`, `build`, `sdist`, `no-redis-smoke-test`). Match the existing file's 2-space indentation, hyphenated job naming, and step-naming style exactly.

The job MUST:
1. Inherit the workflow-level triggers (push to main + PR to main) — do not add a per-job `on:` block and do not add an `if:` guard (unlike `integration-test`, this should run on PRs too for fast feedback).
2. Use `ubuntu-latest`.
3. Use `actions/checkout@v4`.
4. Set up Python 3.12 via `actions/setup-python@v5`.
5. Install uv via `astral-sh/setup-uv@v4`.
6. Build the wheel with `PyO3/maturin-action@v1` using `command: build` and `args: --release --out dist` (same pattern as the existing `build` job — this action handles Rust toolchain setup internally, so no separate `dtolnay/rust-toolchain@stable` step is needed).
7. Create a fresh venv via `uv venv .venv-smoke` (use a distinct path from `.venv` to make it unambiguous this is a clean environment).
8. Install ONLY the built wheel — no `[dev]` extra, no editable install, no test deps: `uv pip install --python .venv-smoke/bin/python dist/*.whl` (use `--python` flag rather than sourcing the venv, to keep the install explicit and avoid any shell-state leakage from the runner).
9. Run the smoke-test via the venv's Python interpreter directly (again, no `source .venv-smoke/bin/activate` needed):
   ```
   .venv-smoke/bin/python -c "import burner_redis; from burner_redis import BurnerRedis, Pipeline, PubSub, Lock, LockError, ResponseError, NoScriptError, Script; print('OK: no hard redis dep')"
   ```
   Note the import list MUST include every symbol in `__all__` from `python/burner_redis/__init__.py` EXCEPT `_coerce_value` (private). Currently that's 8 public names: `BurnerRedis, Lock, LockError, NoScriptError, Pipeline, PubSub, ResponseError, Script`.
10. The `python -c` command exits non-zero on any `ImportError` (including `ImportError: No module named 'redis'` from a hypothetical unguarded import), which fails the job.

Use exactly this YAML block, appended to the end of the file (preserve the single trailing newline at EOF):

```yaml

  no-redis-smoke-test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Set up Python 3.12
        uses: actions/setup-python@v5
        with:
          python-version: "3.12"

      - name: Install uv
        uses: astral-sh/setup-uv@v4

      - name: Build wheel
        uses: PyO3/maturin-action@v1
        with:
          command: build
          args: --release --out dist

      - name: Create clean virtualenv
        run: uv venv .venv-smoke

      - name: Install only the built wheel (no extras)
        run: uv pip install --python .venv-smoke/bin/python dist/*.whl

      - name: Smoke-test imports with no redis package present
        run: |
          .venv-smoke/bin/python -c "import burner_redis; from burner_redis import BurnerRedis, Pipeline, PubSub, Lock, LockError, ResponseError, NoScriptError, Script; print('OK: no hard redis dep')"
```

Do NOT modify any existing job. Do NOT change workflow-level `name:` or `on:` fields. Do NOT add caching, sccache, or matrix strategy — keep it minimal per scope.
  </action>
  <verify>
    <automated>python -c "import yaml; doc = yaml.safe_load(open('.github/workflows/ci.yml')); assert 'no-redis-smoke-test' in doc['jobs'], 'job missing'; steps = doc['jobs']['no-redis-smoke-test']['steps']; names = [s.get('name', s.get('uses', '')) for s in steps]; assert any('Build wheel' in n for n in names), 'build step missing'; assert any('Install only the built wheel' in n for n in names), 'clean install step missing'; assert any('Smoke-test' in n for n in names), 'smoke-test step missing'; cmd = next(s['run'] for s in steps if 'Smoke-test' in s.get('name', '')); assert 'BurnerRedis' in cmd and 'Pipeline' in cmd and 'PubSub' in cmd and 'Lock' in cmd and 'LockError' in cmd and 'ResponseError' in cmd and 'NoScriptError' in cmd and 'Script' in cmd, 'smoke-test does not import all public symbols'; assert 'dev' not in doc['jobs']['no-redis-smoke-test']['steps'][-2].get('run', ''), 'install step must not reference [dev] extra'; print('OK')"</automated>
  </verify>
  <done>
- `.github/workflows/ci.yml` parses as valid YAML.
- Contains a new `no-redis-smoke-test` job at top level under `jobs:`.
- Job has exactly the 7 steps listed above, in order, with names matching the existing file's style.
- Smoke-test `python -c` command imports all 8 public symbols from `burner_redis`.
- No existing job was modified.
- When pushed to a PR: the job runs, builds the wheel cleanly, installs it alone into `.venv-smoke`, imports succeed, and the job reports green.
- If a future commit introduces an unguarded `import redis` at module top level in any shipped file, this job fails with `ModuleNotFoundError: No module named 'redis'`.
  </done>
</task>

</tasks>

<verification>
After CI runs on the PR:
1. `no-redis-smoke-test` job appears in the GitHub Actions checks list alongside `test`, `integration-test`, `build`, `sdist`.
2. It completes in under 90 seconds (wheel build ~45s + venv/install/import ~10s).
3. It reports green on a clean HEAD.
4. Locally reproducible sanity check (optional): from a clean checkout, run `maturin build --release --out dist && uv venv .venv-smoke && uv pip install --python .venv-smoke/bin/python dist/*.whl && .venv-smoke/bin/python -c "import burner_redis; from burner_redis import BurnerRedis, Pipeline, PubSub, Lock, LockError, ResponseError, NoScriptError, Script; print('OK')"` — exits 0.
5. Regression simulation (do not commit): temporarily edit `python/burner_redis/__init__.py` to add a bare `import redis` at line 1 (outside the try/except), push; the new job MUST fail with `ModuleNotFoundError: No module named 'redis'` while other jobs pass. Revert before merging.
</verification>

<success_criteria>
- A `no-redis-smoke-test` job exists in `.github/workflows/ci.yml`.
- The job installs ONLY the built wheel — no `[dev]` extra, no editable install, no test dependencies.
- The smoke-test imports every public symbol in `burner_redis.__all__` (minus the private `_coerce_value`).
- The job runs on both push-to-main and pull_request-to-main.
- The job passes green on the PR that introduces it.
- No existing job (`test`, `integration-test`, `build`, `sdist`) is modified.
</success_criteria>

<output>
After completion, create `.planning/quick/260416-gqd-add-ci-guard-against-accidental-hard-dep/260416-gqd-SUMMARY.md` summarizing:
- The job that was added (name, trigger, Python version).
- Confirmation that the smoke-test covers all 8 public symbols.
- The CI run URL showing the new job passing green on the PR.
</output>
