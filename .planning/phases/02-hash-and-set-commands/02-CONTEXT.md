# Phase 2: Hash and Set Commands - Context

**Gathered:** 2026-04-10
**Status:** Ready for planning

<domain>
## Phase Boundary

Add hash and set data type support to the store engine, implement HSET/HGET/HDEL/HVALS and SADD/SMEMBERS/SISMEMBER/SREM as async Python methods matching `redis.asyncio.Redis` signatures. Introduce WRONGTYPE error handling now that multiple data types exist.

</domain>

<decisions>
## Implementation Decisions

### Hash Command Semantics
- HSET returns integer count of *new* fields added — `hset("k", mapping={"a": "1", "b": "2"})` returns `2` on first call, `0` on repeat (matching redis-py).
- HGET returns `None` for missing field (matching redis-py `Optional[bytes]`).
- HVALS returns `list[bytes]` (matching redis-py) — order is not guaranteed.
- HDEL returns integer count of fields actually deleted (matching redis-py).

### Set Command Semantics
- SADD returns integer count of *new* members added (matching redis-py).
- SMEMBERS returns `set[bytes]` (Python `set` type, matching redis-py).
- SISMEMBER returns `bool` (`True` if member exists, `False` otherwise — matching redis-py).
- SREM returns integer count of members actually removed (matching redis-py).

### WRONGTYPE Error Handling
- With multiple data types, implement `ResponseError("WRONGTYPE Operation against a key holding the wrong kind of value")` when a command targets the wrong type (e.g., HGET on a string key).
- Define custom Python exception class with conditional `redis.exceptions` subclassing: import from `redis.exceptions.ResponseError` if available, otherwise define standalone class.
- This resolves the Phase 1 deferred decision (Q1 from RESEARCH.md Open Questions).

### Claude's Discretion
No items deferred to Claude's discretion — all questions resolved.

</decisions>

<code_context>
## Existing Code Insights

### Reusable Assets
- `src/store.rs` — Store engine with `HashMap<Bytes, ValueEntry>` and passive expiration. Needs extension for Hash and Set value types.
- `src/commands/strings.rs` — `extract_bytes` helper for str/bytes conversion. Reusable for hash/set commands.
- `src/lib.rs` — BurnerRedis pyclass with `Arc<Store>` pattern and `future_into_py` async bridge.
- `tests/conftest.py` — Shared BurnerRedis fixture for pytest.

### Established Patterns
- All command methods are async via `future_into_py` with `Arc<Store>` clone pattern.
- Accept both `str` and `bytes` for keys/values, auto-encode to UTF-8 via `extract_bytes`.
- Single crate, modules in `src/commands/`.
- One pytest file per command group with pytest-asyncio auto mode.

### Integration Points
- `src/store.rs` `ValueEntry` enum needs Hash and Set variants.
- `src/lib.rs` needs new `#[pymethods]` for hash and set commands.
- New `src/commands/hashes.rs` and `src/commands/sets.rs` modules.
- New `tests/test_hashes.py` and `tests/test_sets.py` files.

</code_context>

<specifics>
## Specific Ideas

No specific requirements — follow established Phase 1 patterns and redis-py compatibility.

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope.

</deferred>
