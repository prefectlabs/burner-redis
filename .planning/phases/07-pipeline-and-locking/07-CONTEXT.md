# Phase 7: Pipeline and Locking - Context

**Gathered:** 2026-04-11
**Status:** Ready for planning

<domain>
## Phase Boundary

Implement Pipeline class for batched command execution and Lock/AsyncLock class for distributed locking with timeout, ownership tokens, and automatic expiration. Both are Python-level abstractions built on top of the existing BurnerRedis commands.

</domain>

<decisions>
## Implementation Decisions

### Pipeline Semantics
- Commands queued as `(method_name, args, kwargs)` tuples in a list. `execute()` runs all sequentially against the BurnerRedis instance, returns results list in command order.
- Pipeline supports `async with client.pipeline() as pipe: ...` — `__aenter__` returns self, `__aexit__` calls `execute()`.
- Pipeline is a separate Python class that mirrors BurnerRedis method signatures but buffers commands instead of executing immediately. Created via `client.pipeline()`.
- No actual batching optimization needed — in-process execution is already fast. The Pipeline abstraction provides API compatibility with redis-py.

### Lock Semantics
- `Lock` class with `acquire(blocking=True, blocking_timeout=None)` and `release()`. Uses SET NX with PX for atomic acquire. Token-based ownership via random UUID stored as the key's value.
- Blocking acquisition uses async sleep polling with configurable `sleep` interval (default 0.1s) — matching redis-py's Lock implementation.
- Release verifies token ownership before deleting (compare value, then delete if match). Raises `LockError` if token doesn't match (lock was stolen or expired).
- Lock supports `async with client.lock("name", timeout=10) as lock: ...` — acquires on `__aenter__`, releases on `__aexit__`.
- Lock constructor params: `name`, `timeout` (lock TTL in seconds), `sleep` (polling interval), `blocking` (whether acquire blocks), `blocking_timeout` (max time to wait).

### Implementation Notes
- Both Pipeline and Lock are pure Python classes (in `python/burner_redis/__init__.py` or separate files), not Rust — they orchestrate existing BurnerRedis async methods.
- `LockError` exception class alongside existing `ResponseError`.
- `client.pipeline()` returns `Pipeline(client)`.
- `client.lock(name, **kwargs)` returns `Lock(client, name, **kwargs)`.

### Claude's Discretion
No items deferred to Claude's discretion — all questions resolved.

</decisions>

<code_context>
## Existing Code Insights

### Reusable Assets
- `python/burner_redis/__init__.py` — BurnerRedis class, ResponseError exception.
- All BurnerRedis async methods (set, get, delete, exists, hset, etc.) — Pipeline wraps these.
- SET with NX and PX flags — Lock uses this for atomic acquisition.

### Established Patterns
- Pure Python async/await on top of the Rust core.
- pytest-asyncio for integration tests.

### Integration Points
- `python/burner_redis/__init__.py` — Add Pipeline, Lock, LockError classes.
- `src/lib.rs` — Add `pipeline()` and `lock()` factory methods.
- New `tests/test_pipeline.py` and `tests/test_locking.py`.

</code_context>

<specifics>
## Specific Ideas

No specific requirements — follow redis-py Lock and Pipeline interface patterns.

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope.

</deferred>
