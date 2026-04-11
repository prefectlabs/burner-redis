---
phase: quick
plan: 260411-ipj
type: execute
wave: 1
depends_on: []
files_modified:
  - .github/workflows/ci.yml
  - pyproject.toml
  - tests/test_prefect_integration.py
autonomous: true
---

<objective>
Run integration tests in CI only on merge to main, not on pull requests.

Purpose: Integration tests (test_prefect_integration.py) are heavier and only need to gate
main branch quality. Unit tests continue running on every PR for fast feedback.

Output: Updated CI workflow with separate integration test job triggered only on push to main.
</objective>

<context>
@.github/workflows/ci.yml
@tests/test_prefect_integration.py
@pyproject.toml
</context>

<tasks>

<task type="auto">
  <name>Task 1: Mark integration tests and configure pytest to exclude them by default</name>
  <files>tests/test_prefect_integration.py, pyproject.toml</files>
  <action>
1. In pyproject.toml under [tool.pytest.ini_options], add a markers declaration and a default addopts that excludes integration tests:
   ```
   markers = ["integration: integration tests that simulate Prefect usage"]
   addopts = "-m 'not integration'"
   ```
2. In tests/test_prefect_integration.py, add a module-level pytestmark at the top (after imports):
   ```python
   pytestmark = pytest.mark.integration
   ```
   This marks all tests in the file as integration tests. They will be skipped by default `pytest` runs (due to addopts) but can be explicitly included with `pytest -m integration`.
  </action>
  <verify>
    <automated>cd /Users/desertaxle/dev/prefectlabs/burner-redis && maturin develop --release 2>/dev/null && pytest --co -q 2>&1 | grep -c "test_prefect_integration" | grep "^0$"</automated>
  </verify>
  <done>Running bare `pytest` collects zero tests from test_prefect_integration.py. Running `pytest -m integration` collects only integration tests.</done>
</task>

<task type="auto">
  <name>Task 2: Add dedicated integration test job in CI workflow for push to main only</name>
  <files>.github/workflows/ci.yml</files>
  <action>
1. In .github/workflows/ci.yml, modify the existing `test` job's "Run tests" step to make it explicit that it uses the default (which now excludes integration via addopts). No change needed since addopts handles it.

2. Add a new job `integration-test` that:
   - Only runs on push to main (use an `if: github.event_name == 'push'` condition)
   - Uses ubuntu-latest
   - Checks out code, sets up Python 3.12, installs deps, builds with maturin develop --release
   - Runs `pytest -m integration` to run only the integration tests

The new job should look like:
```yaml
  integration-test:
    if: github.event_name == 'push'
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Set up Python 3.12
        uses: actions/setup-python@v5
        with:
          python-version: "3.12"

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable

      - name: Install dependencies
        run: pip install -e ".[dev]"

      - name: Build with maturin
        run: maturin develop --release

      - name: Run integration tests
        run: pytest -m integration
```

Also add a `Install Rust toolchain` step to the existing `test` job (using `dtolnay/rust-toolchain@stable`) before the `Install dependencies` step, since `maturin develop` requires Rust. Check if it is already there -- if `pip install -e ".[dev]"` + `maturin develop` works today, Rust may be pre-installed on ubuntu-latest, but being explicit is better practice.
  </action>
  <verify>
    <automated>cd /Users/desertaxle/dev/prefectlabs/burner-redis && python -c "import yaml; y=yaml.safe_load(open('.github/workflows/ci.yml')); assert 'integration-test' in y['jobs']; assert y['jobs']['integration-test']['if'] == \"github.event_name == 'push'\"; print('OK')" 2>/dev/null || (cat .github/workflows/ci.yml | grep -A2 "integration-test:" | head -5)</automated>
  </verify>
  <done>CI workflow has an `integration-test` job that only runs on push events (merge to main). The regular `test` job continues running on both push and PR but excludes integration tests via pytest addopts.</done>
</task>

</tasks>

<verification>
- `pytest --co -q` shows no integration tests collected
- `pytest -m integration --co -q` shows only test_prefect_integration.py tests
- `.github/workflows/ci.yml` has separate `integration-test` job with `if: github.event_name == 'push'` condition
</verification>

<success_criteria>
- Integration tests are excluded from PR CI runs (fast feedback preserved)
- Integration tests run on merge to main (quality gate for main branch)
- No test is silently lost -- all tests remain runnable via explicit marker selection
</success_criteria>

<output>
After completion, create `.planning/quick/260411-ipj-run-integration-tests-in-ci-on-merge-to-/260411-ipj-SUMMARY.md`
</output>
