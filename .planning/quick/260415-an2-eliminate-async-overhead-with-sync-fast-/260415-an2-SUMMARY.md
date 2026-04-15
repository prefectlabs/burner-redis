# Quick Task 260415-an2: Eliminate async overhead with sync fast path and native pipeline execution

**Date:** 2026-04-15
**Status:** Complete

## Changes

### Task 1: ResolvedFuture + Sync Fast Path
- Added `ResolvedFuture` pyclass that implements `__await__` + `__next__` with `StopIteration` to return pre-computed results
- Converted ~55 commands from `future_into_py` to synchronous execution with `ResolvedFuture`
- Only `xreadgroup` (blocking path) and `_subscribe_listener` remain async via `future_into_py`
- Eliminated `Arc::clone` per command on the fast path (natural consequence of removing async blocks)

### Task 2: Native Pipeline Execution
- Added `execute_pipeline` Rust method with `dispatch_pipeline_command` for single-boundary-crossing batch execution
- Updated `Pipeline.execute()` in Python to call native Rust pipeline instead of N individual awaits
- Single write lock acquisition for entire pipeline instead of N lock acquisitions

### Task 3: Verification
- All 113 Rust tests pass
- All 337 Python tests pass
- Benchmark: 34.9s -> 2.6s (13.4x improvement, well under 10s target)
- Docket test suite: 676 passed, 78 skipped

## Commits
- `de9d259` feat(quick-260415-an2): add ResolvedFuture and convert all non-blocking commands to sync fast path
- `9e1fa38` feat(quick-260415-an2): add native Rust pipeline execution and fix StopIteration tuple handling

## Files Modified
- `src/lib.rs` — ResolvedFuture, sync fast path, execute_pipeline, dispatch_pipeline_command
- `python/burner_redis/pipeline.py` — Updated execute() to use native Rust pipeline
