---
phase: quick-260416-gqd
plan: 01
subsystem: ci
tags: [ci, github-actions, regression-guard, packaging]
requires: []
provides:
  - no-redis-smoke-test CI job
  - regression guard against accidental unguarded `import redis` in shipped modules
affects:
  - .github/workflows/ci.yml
tech-stack:
  added: []
  patterns:
    - Clean-wheel install into isolated venv (no dev extras) for packaging-safety smoke test
key-files:
  created: []
  modified:
    - .github/workflows/ci.yml
decisions:
  - Used PyO3/maturin-action@v1 (same pattern as existing build job) instead of a separate Rust toolchain step
  - Used `uv pip install --python .venv-smoke/bin/python dist/*.whl` rather than sourcing the venv — keeps the install explicit and avoids shell-state leakage
  - Did NOT add `if: github.event_name == 'push'` guard — smoke test runs on both push and PR for fast feedback
metrics:
  duration: ~3min
  completed: "2026-04-16"
  tasks: 1
  files: 1
---

# Quick Task 260416-gqd: CI Guard Against Accidental Hard redis Dep Summary

Added a `no-redis-smoke-test` job to `.github/workflows/ci.yml` that builds the wheel, installs it alone into a clean venv (no `[dev]` extras, no test deps, no `redis` package), and imports every public symbol — failing CI fast if an unguarded `import redis` ever sneaks into shipped code.

## What Was Done

### Task 1: Add no-redis-smoke-test job to ci.yml

Appended a new job after the existing `sdist` job in `.github/workflows/ci.yml`. The job:

- Runs on `ubuntu-latest`, inheriting the workflow-level `push` and `pull_request` triggers.
- Checks out code (`actions/checkout@v4`).
- Sets up Python 3.12 (`actions/setup-python@v5`) — matches the `integration-test` job's middle-of-matrix version choice.
- Installs `uv` (`astral-sh/setup-uv@v4`).
- Builds the wheel via `PyO3/maturin-action@v1` with `command: build` / `args: --release --out dist` (same pattern as existing `build` job; the action bundles Rust toolchain setup so no separate `dtolnay/rust-toolchain@stable` step is needed).
- Creates a fresh venv at `.venv-smoke` (distinct from the shared `.venv` used by other jobs).
- Installs only the wheel with `uv pip install --python .venv-smoke/bin/python dist/*.whl` — no `[dev]` extra, no editable install.
- Runs a single-line smoke test that imports every public symbol from `burner_redis.__all__` (minus the private `_coerce_value`):
  `import burner_redis; from burner_redis import BurnerRedis, Pipeline, PubSub, Lock, LockError, ResponseError, NoScriptError, Script; print('OK: no hard redis dep')`

**Symbol coverage:** All 8 public symbols exported from `python/burner_redis/__init__.py`:
1. `BurnerRedis`
2. `Pipeline`
3. `PubSub`
4. `Lock`
5. `LockError`
6. `ResponseError`
7. `NoScriptError`
8. `Script`

**Commit:** `2647c54` — `ci(quick-260416-gqd): add no-redis smoke test to catch accidental hard deps`

## Why This Matters

`python/burner_redis/__init__.py` and `python/burner_redis/lock.py` intentionally contain `import redis` inside `try/except (ImportError, AttributeError)` blocks so the library can subclass `redis.exceptions.*` when `redis` happens to be installed alongside. Since commit `7d6240f` added `redis` to `[project.optional-dependencies].dev`, every previously existing CI job has `redis` installed in its environment — meaning a regression where a bare `import redis` lands at module top level outside the try/except would silently pass every check but break real user installs (`pip install burner-redis` with nothing else).

This new job closes that blind spot by exercising the exact install path real users hit: just the wheel, nothing else, in a fresh interpreter.

## Verification Performed

Ran the plan's PyYAML assertion block against the updated file:

```bash
.venv/bin/python -c "import yaml; doc = yaml.safe_load(open('.github/workflows/ci.yml')); \
    assert 'no-redis-smoke-test' in doc['jobs']; \
    steps = doc['jobs']['no-redis-smoke-test']['steps']; \
    names = [s.get('name', s.get('uses', '')) for s in steps]; \
    assert any('Build wheel' in n for n in names); \
    assert any('Install only the built wheel' in n for n in names); \
    assert any('Smoke-test' in n for n in names); \
    cmd = next(s['run'] for s in steps if 'Smoke-test' in s.get('name', '')); \
    assert all(sym in cmd for sym in ['BurnerRedis','Pipeline','PubSub','Lock','LockError','ResponseError','NoScriptError','Script']); \
    assert 'dev' not in doc['jobs']['no-redis-smoke-test']['steps'][-2].get('run', ''); \
    print('OK')"
# -> OK
```

All plan assertions pass:
- Job named `no-redis-smoke-test` exists at top level under `jobs:`.
- Job includes a `Build wheel` step, an `Install only the built wheel (no extras)` step, and a `Smoke-test imports with no redis package present` step.
- The smoke-test `run` command imports all 8 public symbols.
- The install step does NOT reference the `[dev]` extra.

Full YAML parses cleanly via `yaml.safe_load`.

## CI Run URL

The plan's `<output>` block requests the PR CI run URL. This executor does not push branches or open PRs — the commit `2647c54` will be part of whichever branch/PR the user (or orchestrator) surfaces next. The CI run URL should be recorded there once the PR is opened.

## Deviations from Plan

None - plan executed exactly as written. The YAML block was copied verbatim from the plan and appended after the `sdist` job's final step (preserving the trailing newline).

## Deferred Issues

None.

## Self-Check: PASSED

- `.github/workflows/ci.yml` updated (+29 lines, 1 file changed)
- Commit `2647c54` present in git log
- PyYAML structural assertions pass
- No other files modified; no existing job touched
