# Phase 6: Lua Scripting - Context

**Gathered:** 2026-04-11
**Status:** Ready for planning

<domain>
## Phase Boundary

Embed mlua (Lua 5.4) for EVAL/EVALSHA support. Lua scripts can call redis.call()/redis.pcall() to execute any supported Redis command atomically. Script caching via SCRIPT LOAD/EXISTS.

</domain>

<decisions>
## Implementation Decisions

### Lua-Redis Bridge
- Register a `redis.call()` Lua function that dispatches to the Store's command handler. Store is accessible via Lua app data (mlua's `set_app_data`/`app_data_ref` or closure capture).
- `redis.call()` propagates errors (raises Lua error on failure). `redis.pcall()` returns `{err="..."}` table on failure (matching Redis behavior exactly).
- Type conversion Redis→Lua: bulk string→Lua string, integer→number, nil→false, array→table (1-indexed). Lua→Redis: string→bulk reply, number→integer reply, table with sequential keys→array, table with `err` key→error, false/nil→nil reply.
- Lua scripts hold the Store write lock for the entire script duration — atomic execution, no interleaving. This matches Redis's single-threaded execution model for scripts.

### Script Caching
- `HashMap<String, String>` mapping SHA1 hex digest → script source. Stored alongside the Store (in a separate field, not in the keyspace).
- SCRIPT LOAD computes SHA1, stores script, returns hex hash.
- EVAL also auto-caches every script it executes (computes SHA1 and stores). Subsequent EVALSHA works without explicit SCRIPT LOAD.
- SCRIPT EXISTS accepts multiple SHA1 hashes, returns `list[bool]` indicating which are cached.
- No size limit on script cache — Prefect has a fixed set of known scripts.

### Implementation Details
- Use `mlua` crate with `lua54` and `send` features.
- Create a fresh `Lua` VM per EVAL/EVALSHA call (isolation between scripts, no state leakage). Or reuse a single VM with `scope()` for performance — Claude's discretion on this tradeoff.
- The dispatch function for `redis.call()` must handle: SET, GET, DELETE, EXISTS, HSET, HGET, HDEL, HVALS, SADD, SMEMBERS, SISMEMBER, SREM, ZADD, ZREM, ZRANGE, ZRANGEBYSCORE, ZREMRANGEBYSCORE, XADD, XREAD.
- SHA1 computation via Rust's `sha1` crate or manual implementation.

### Claude's Discretion
- Whether to create a new Lua VM per call or reuse one (performance vs isolation tradeoff).
- Internal dispatch table structure for redis.call() routing.

</decisions>

<code_context>
## Existing Code Insights

### Reusable Assets
- `src/store.rs` — All data type methods already implemented (string, hash, set, sorted set, stream).
- `src/lib.rs` — BurnerRedis with `Arc<Store>`, command dispatch patterns.
- `python/burner_redis/__init__.py` — ResponseError exception for script errors.

### Established Patterns
- `Arc<Store>` for shared access.
- `parking_lot::RwLock` for thread-safe locking.
- `future_into_py` for async Python methods.
- `StoreError` for error propagation.

### Integration Points
- `Cargo.toml` needs `mlua` and `sha1` dependencies.
- New `src/scripting.rs` module for Lua VM and script execution.
- `src/store.rs` needs a `scripts: HashMap<String, String>` field.
- `src/lib.rs` needs `eval`, `evalsha`, `script_load`, `script_exists` methods.
- New `tests/test_scripting.py` file.

</code_context>

<specifics>
## Specific Ideas

No specific requirements — follow CLAUDE.md's mlua recommendation and redis-py compatibility.

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope.

</deferred>
