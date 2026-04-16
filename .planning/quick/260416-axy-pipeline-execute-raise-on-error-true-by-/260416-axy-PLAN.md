---
quick_id: 260416-axy
plan: 01
type: execute
wave: 1
depends_on: []
files_modified:
  - python/burner_redis/pipeline.py
  - tests/test_pipeline.py
autonomous: true
requirements:
  - "quick:260416-axy"

must_haves:
  truths:
    - "Pipeline.execute() with no args raises the first Exception present in the results list (redis-py default)."
    - "Pipeline.execute(raise_on_error=True) raises the first Exception in results."
    - "Pipeline.execute(raise_on_error=False) preserves current behavior — Exception objects returned inline at their command's index."
    - "Downstream code using `try: results = await pipe.execute() ... except ResponseError:` works correctly (the pattern that previously failed with 'ResponseError object is not iterable')."
    - "Successful pipelines (no errors) return the full results list regardless of raise_on_error value."
  artifacts:
    - path: "python/burner_redis/pipeline.py"
      provides: "Pipeline.execute(raise_on_error=True) signature and raise-first-error semantics"
      contains: "raise_on_error"
    - path: "tests/test_pipeline.py"
      provides: "TDD coverage for raise_on_error default-True and explicit-False behaviors; updated WRONGTYPE test"
      contains: "raise_on_error"
  key_links:
    - from: "python/burner_redis/pipeline.py::Pipeline.execute"
      to: "results list scan for isinstance(r, Exception)"
      via: "for-loop raising first match when raise_on_error truthy"
      pattern: "raise_on_error"
    - from: "tests/test_pipeline.py::test_pipeline_wrongtype_error"
      to: "Pipeline.execute(raise_on_error=False)"
      via: "explicit opt-out to inspect inline errors"
      pattern: "raise_on_error=False"
---

<objective>
Update `Pipeline.execute()` to accept `raise_on_error: bool = True` (default True), matching redis-py's signature. When True, raise the first Exception found in results; when False, preserve current inline-error behavior. Fixes downstream pattern where callers wrap `await pipe.execute()` in `try/except ResponseError` and currently crash with `TypeError: 'ResponseError' object is not iterable` because the error was sitting inline in the results tuple instead of being raised.

Purpose: Drop-in redis-py compatibility. Prefect/docket code written against `redis.asyncio.Redis.pipeline()` uses try/except around `execute()` and expects errors to be raised, not returned. This was the last known pipeline behavior gap.

Output: Pipeline.execute(raise_on_error=...) matching redis-py semantics, with TDD coverage and the existing inline-error test updated to opt-out explicitly.
</objective>

<execution_context>
@$HOME/.claude/get-shit-done/workflows/execute-plan.md
@$HOME/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@.planning/STATE.md
@./CLAUDE.md

# Source of the fix
@python/burner_redis/pipeline.py

# Test file to extend and partially update
@tests/test_pipeline.py

# Existing conftest: `r` fixture returns a fresh BurnerRedis()
@tests/conftest.py

# redis-py reference (installed locally via the `redis` dependency):
# `redis.client.Pipeline.execute(self, raise_on_error: bool = True)` and
# helper `raise_first_error(commands, response)` — used to confirm signature.

<interfaces>
<!-- Current Pipeline.execute signature (from python/burner_redis/pipeline.py) -->
```python
class Pipeline:
    def __init__(self, client): ...
    async def execute(self):
        if not self._commands:
            return []
        results = await self._client.execute_pipeline(self._commands)
        self._commands = []
        return list(results)
```

<!-- ResponseError exposed via burner_redis.__init__.py:
# Subclasses redis.exceptions.ResponseError when the `redis` package is importable
# (which is always the case for users hitting this bug — they installed redis-py). -->

<!-- The Rust execute_pipeline (src/lib.rs) already returns Python exception objects
# inline in the results list for per-command failures (e.g. WRONGTYPE, NOGROUP).
# No Rust changes needed — this is purely the Python Pipeline.execute wrapper. -->

<!-- Existing test that currently asserts inline-error behavior — MUST be updated
# to pass raise_on_error=False, because its explicit intent is to inspect the
# inline exception objects at specific indices: -->
```python
# tests/test_pipeline.py:200
async def test_pipeline_wrongtype_error(r):
    """Pipeline returns WRONGTYPE errors inline matching redis-py behavior."""
    await r.set("str_key", "value")
    pipe = r.pipeline()
    pipe.set("good_key", "good_value")
    pipe.hset("str_key", key="field", value="val")  # WRONGTYPE error
    pipe.get("good_key")
    results = await pipe.execute()
    # All three commands should execute; error is inline at index 1
    assert results[0] is True
    assert isinstance(results[1], Exception)
    assert "WRONGTYPE" in str(results[1])
    assert results[2] == b"good_value"
```
</interfaces>

<audit_notes>
Grepped every `.execute()` call in tests/ (21 occurrences across test_pipeline.py, test_pubsub.py, test_prefect_integration.py). Only ONE test explicitly asserts inline Exception behavior: `test_pipeline_wrongtype_error` at tests/test_pipeline.py:200. All other tests either:
- Assert successful results (no error path)
- Assert `isinstance(results[i], int)` / `bytes` / `str` — these are value-type assertions, not Exception assertions, and are unaffected.

No other call sites need updating. The switch to default `raise_on_error=True` is behaviorally safe for the existing suite after the single WRONGTYPE test is updated.
</audit_notes>
</context>

<tasks>

<task type="auto" tdd="true">
  <name>Task 1: Add raise_on_error=True default to Pipeline.execute (TDD)</name>
  <files>
    - python/burner_redis/pipeline.py
    - tests/test_pipeline.py
  </files>
  <behavior>
    New/updated tests in tests/test_pipeline.py (append after the existing "# --- Error handling ---" section):

    1. `test_pipeline_raises_on_error_by_default(r)` — RED first:
       - Build a pipeline with `pipe.xlen("nonexistent")` then `pipe.xpending_range("nonexistent", "nonexistent-group", "-", "+", count=10)`.
       - Wrap in `with pytest.raises(redis.exceptions.ResponseError, match="NOGROUP"):`.
       - Call `await pipe.execute()` (no args).
       - Must raise the NOGROUP ResponseError (first Exception in results). xlen returns 0 at index 0; the NOGROUP error appears at index 1 and is what gets raised.
       - Import `redis.exceptions` at top of file (redis-py is already a runtime dep via `ResponseError` subclassing).

    2. `test_pipeline_returns_errors_when_raise_on_error_false(r)` — RED first:
       - Same pipeline construction as above.
       - Call `results = await pipe.execute(raise_on_error=False)`.
       - Assert `results[0] == 0` (xlen on nonexistent stream returns 0).
       - Assert `isinstance(results[1], redis.exceptions.ResponseError)`.
       - Assert `"NOGROUP" in str(results[1])`.

    3. `test_pipeline_raises_on_error_true_explicit(r)` — RED first:
       - Same construction; call `await pipe.execute(raise_on_error=True)` and assert `pytest.raises(redis.exceptions.ResponseError, match="NOGROUP")`.
       - Confirms explicit True behaves identically to default.

    4. `test_pipeline_success_unaffected_by_raise_on_error(r)` — RED first (will pass even before the fix, but lock in the invariant):
       - Build a pipeline with only successful commands: `pipe.set("k1", "v1")`, `pipe.get("k1")`.
       - Call both `await pipe.execute()` and (on a fresh pipeline) `await pipe.execute(raise_on_error=False)`.
       - Assert both return `[True, b"v1"]`.

    5. UPDATE `test_pipeline_wrongtype_error` at line ~200 — change the single call:
       - From: `results = await pipe.execute()`
       - To:   `results = await pipe.execute(raise_on_error=False)`
       - Rationale: this test's explicit purpose is to inspect inline Exception objects per command, which is exactly what `raise_on_error=False` preserves. Update the docstring to mention the opt-out.
  </behavior>
  <action>
    RED → GREEN → REFACTOR cycle in one task. All commands use `uv run` from the repo root.

    RED:
    1. Add `import redis.exceptions` to the top of tests/test_pipeline.py (next to existing `import pytest`).
    2. Append the four new tests listed in <behavior> to tests/test_pipeline.py after the existing `# --- Error handling ---` block (after `test_pipeline_wrongtype_error`).
    3. Update `test_pipeline_wrongtype_error` to pass `raise_on_error=False`, and update its docstring to: `"""Pipeline returns WRONGTYPE errors inline when raise_on_error=False (opt-out of redis-py default raising behavior)."""`
    4. Run the NEW tests only, confirm they fail in the expected way (TypeError or AssertionError — NOT a collection error):
       `uv run pytest tests/test_pipeline.py::test_pipeline_raises_on_error_by_default tests/test_pipeline.py::test_pipeline_returns_errors_when_raise_on_error_false tests/test_pipeline.py::test_pipeline_raises_on_error_true_explicit -x`
       Expected: `test_pipeline_raises_on_error_by_default` FAILS because execute() currently returns the ResponseError inline and pytest.raises sees no exception. The `raise_on_error=False` test FAILS with `TypeError: execute() got an unexpected keyword argument 'raise_on_error'`. This is the RED state.

    GREEN:
    5. Edit python/burner_redis/pipeline.py::Pipeline.execute to accept and honor `raise_on_error`:

       ```python
       async def execute(self, raise_on_error: bool = True) -> list:
           """Execute all queued commands and return results in order.

           Uses native Rust pipeline execution: a single Python-to-Rust
           boundary crossing executes all commands synchronously in a tight
           loop, eliminating per-command async overhead.

           Matches redis-py behavior: all commands execute regardless of
           individual failures. When raise_on_error is True (default, matching
           redis-py), the first Exception in the results list is raised after
           execution completes. When False, Exception objects are returned
           inline at the position of the failed command, preserving per-command
           error inspection.
           """
           if not self._commands:
               return []
           results = await self._client.execute_pipeline(self._commands)
           self._commands = []
           results = list(results)
           if raise_on_error:
               for r in results:
                   if isinstance(r, Exception):
                       raise r
           return results
       ```

       Notes:
       - Do NOT reorder: clear `self._commands` BEFORE raising (redis-py also clears command stack before raising — a failed execute still consumes the queued commands). This matches the existing clear-then-return ordering.
       - Use `isinstance(r, Exception)` (broad), not `isinstance(r, ResponseError)` — redis-py's `raise_first_error` raises any `Exception` found in results, and we want identical semantics so a future NoScriptError etc. is also raised.
       - Keep the signature exactly `raise_on_error: bool = True` (positional-or-keyword). redis-py's Pipeline.execute is `execute(self, raise_on_error: bool = True)` — keyword-compatible.

    6. Run the new tests again, confirm GREEN:
       `uv run pytest tests/test_pipeline.py::test_pipeline_raises_on_error_by_default tests/test_pipeline.py::test_pipeline_returns_errors_when_raise_on_error_false tests/test_pipeline.py::test_pipeline_raises_on_error_true_explicit tests/test_pipeline.py::test_pipeline_success_unaffected_by_raise_on_error -x`

    7. Run the FULL pipeline test file to confirm the updated `test_pipeline_wrongtype_error` passes and nothing else regressed:
       `uv run pytest tests/test_pipeline.py -x`

    8. Run the full Python test suite to catch any cross-file breakage (test_pubsub.py and test_prefect_integration.py both use pipelines — grep confirmed they only assert success or value types, no inline-error assertions, but verify):
       `uv run pytest tests/ -x`

    REFACTOR (only if needed):
    9. If the execute() helper grows, extract a private `_raise_first_error(results)` to mirror redis-py's helper name. Only do this if it improves readability — a single 3-line loop doesn't justify extraction. Skip by default.

    Commit strategy (per TDD convention in planner instructions):
    - Commit A (RED): `test(260416-axy): add failing tests for Pipeline.execute raise_on_error` — tests/test_pipeline.py only, tests red.
    - Commit B (GREEN): `feat(260416-axy): add raise_on_error=True default to Pipeline.execute (redis-py compat)` — python/burner_redis/pipeline.py + the test_pipeline_wrongtype_error opt-out update, tests green.
  </action>
  <verify>
    <automated>uv run pytest tests/test_pipeline.py -x -v</automated>
    Full-suite sanity check:
    <automated>uv run pytest tests/ -x</automated>
  </verify>
  <done>
    - `Pipeline.execute` has signature `async def execute(self, raise_on_error: bool = True) -> list`.
    - `await pipe.execute()` with an erroring command raises the first Exception (redis-py default).
    - `await pipe.execute(raise_on_error=False)` returns the full list with Exception objects inline (prior behavior preserved).
    - Four new tests pass; updated `test_pipeline_wrongtype_error` passes with `raise_on_error=False`.
    - `uv run pytest tests/` is fully green — no regressions in test_pubsub.py or test_prefect_integration.py pipeline usage.
    - The downstream docket/prefect pattern `try: ... = await pipe.execute(); except ResponseError: ...` now works (no longer raises `TypeError: 'ResponseError' object is not iterable`).
  </done>
</task>

</tasks>

<verification>
- Unit: new tests in tests/test_pipeline.py cover default True, explicit True, explicit False, and the no-error invariant.
- Regression: `uv run pytest tests/ -x` passes across all suites (pipeline, pubsub, prefect_integration, streams, etc.).
- Behavioral parity check (manual/visual, not automated): signature matches redis-py `Pipeline.execute(self, raise_on_error: bool = True)`.
</verification>

<success_criteria>
- Pipeline.execute defaults to raise_on_error=True and raises the first Exception in results.
- Pipeline.execute(raise_on_error=False) preserves inline-error behavior for callers that want per-command inspection.
- All existing tests pass (one test updated to opt-out with raise_on_error=False — the ONLY test that asserted inline Exception objects).
- Full test suite green via `uv run pytest tests/`.
- The original downstream failure pattern (catching ResponseError around `pipe.execute()`) works end-to-end.
</success_criteria>

<output>
After completion, create `.planning/quick/260416-axy-pipeline-execute-raise-on-error-true-by-/260416-axy-SUMMARY.md` summarizing:
- The single-line fix (raise_on_error param + results scan).
- Which test was updated and why (test_pipeline_wrongtype_error opt-out).
- Test audit outcome (only 1 of 21 .execute() call sites needed changes).
- Confirmation full suite is green.
</output>
