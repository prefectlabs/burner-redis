---
quick_id: 260416-axy
plan: 01
subsystem: python-bindings / pipeline
tags: [redis-py-compat, pipeline, error-handling, tdd]
requires: []
provides:
  - Pipeline.execute(raise_on_error=True) redis-py-compatible default
  - Pipeline.execute(raise_on_error=False) opt-out for inline error inspection
affects:
  - python/burner_redis/pipeline.py
  - tests/test_pipeline.py
tech-stack:
  added: []
  patterns:
    - "raise-first-error scan after results list materialized"
key-files:
  created: []
  modified:
    - python/burner_redis/pipeline.py
    - tests/test_pipeline.py
decisions:
  - "Kept clear-then-raise ordering (self._commands cleared before the raise) to match redis-py's own command-stack-clear-on-failed-execute semantics."
  - "Used broad isinstance(r, Exception) (not ResponseError) so future NoScriptError/ConnectionError objects inserted inline by the Rust layer also raise."
  - "Updated only test_pipeline_wrongtype_error to opt out via raise_on_error=False — audit confirmed it is the ONLY test asserting inline Exception behavior."
metrics:
  duration: 2min
  completed: 2026-04-16
  tasks: 1 (TDD: 2 commits)
  files: 2
---

# Quick 260416-axy: Pipeline.execute raise_on_error Summary

Added `raise_on_error: bool = True` to `Pipeline.execute()` for drop-in redis-py signature parity; when True (default), the first Exception in the results list is raised after the pipeline runs, eliminating the downstream "ResponseError object is not iterable" crash in code that wraps `await pipe.execute()` in `try/except ResponseError`.

## What changed

**The fix itself (python/burner_redis/pipeline.py):**

```python
async def execute(self, raise_on_error: bool = True) -> list:
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

- New kwarg `raise_on_error: bool = True` — exact signature parity with `redis.client.Pipeline.execute`.
- Results list is still fully materialized before the scan, so all queued commands execute regardless of individual failures (matches redis-py).
- `self._commands = []` runs BEFORE the raise, so a failed execute still consumes the queued commands (matches redis-py's command-stack-clear-on-failed-execute behavior).
- `isinstance(r, Exception)` is broad by design — any Exception-object the Rust layer inserts inline (WRONGTYPE, NOGROUP, NoScriptError, etc.) is raised.

## Test changes

**New tests (4) in tests/test_pipeline.py:**

1. `test_pipeline_raises_on_error_by_default` — confirms `await pipe.execute()` raises the first Exception (NOGROUP from xpending_range on a missing group).
2. `test_pipeline_returns_errors_when_raise_on_error_false` — confirms the opt-out returns `results[0] == 0` and `isinstance(results[1], redis.exceptions.ResponseError)`.
3. `test_pipeline_raises_on_error_true_explicit` — confirms explicit `True` behaves identically to the default.
4. `test_pipeline_success_unaffected_by_raise_on_error` — locks in the invariant that successful pipelines return the full results list regardless of the flag.

Also added `import redis.exceptions` to the top of the test module.

**Updated test (1):**

- `test_pipeline_wrongtype_error` — changed `await pipe.execute()` → `await pipe.execute(raise_on_error=False)` and updated the docstring to "Pipeline returns WRONGTYPE errors inline when raise_on_error=False (opt-out of redis-py default raising behavior)." The test's explicit purpose is to inspect inline Exception objects per command, which is exactly what `raise_on_error=False` preserves.

## Test audit outcome

Audit scope: all `.execute()` call sites in tests/ (21 occurrences across test_pipeline.py, test_pubsub.py, test_prefect_integration.py).

- Only ONE call site asserted inline Exception behavior: `test_pipeline_wrongtype_error`. That test was updated.
- All other call sites assert either successful results or value-type assertions (`isinstance(results[i], int|bytes|str)`) — unaffected by the default-True switch.
- The planner's prediction (1 of 21 call sites needs updating) held exactly. No additional call-site changes were needed.

## Verification

- `uv run pytest tests/test_pipeline.py -x` — 27/27 pass.
- `uv run pytest tests/` — 354 passed, 1 skipped, 30 deselected. No regressions.
- Signature parity confirmed via `inspect.signature`:
  - burner-redis: `(self, raise_on_error: bool = True) -> list`
  - redis-py:     `(self, raise_on_error: bool = True) -> List[Any]`
- NOGROUP phrasing from the prior quick-260415-vor fix still flows through cleanly: the raised error is a `redis.exceptions.ResponseError` with message `"NOGROUP No such key 'nonexistent' or consumer group 'nonexistent-group' in XPENDING"`.

## Deviations from Plan

None — plan executed exactly as written. TDD RED→GREEN flow worked on the first pass; no refactor needed (3-line loop didn't justify helper extraction).

## Commits

- `522cd3c` — test(quick-260416-axy): add failing tests for Pipeline.execute raise_on_error (RED)
- `a15cfad` — fix(quick-260416-axy): add raise_on_error=True default to Pipeline.execute (GREEN)

## Self-Check: PASSED

- FOUND: python/burner_redis/pipeline.py (modified, contains `raise_on_error`)
- FOUND: tests/test_pipeline.py (modified, contains `raise_on_error` in 5 tests)
- FOUND commit: 522cd3c (RED)
- FOUND commit: a15cfad (GREEN)
