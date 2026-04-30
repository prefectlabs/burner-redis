# Decisions

<!-- Append-only register of architectural and pattern decisions -->

| ID | Decision | Rationale | Date |
|----|----------|-----------|------|

---

## Decisions Table

| # | When | Scope | Decision | Choice | Rationale | Revisable? | Made By |
|---|------|-------|----------|--------|-----------|------------|---------|
| D001 | Migration review | architecture | Embedded in-process Redis, not a standalone server | Keep burner-redis embedded inside the host Python process. | Eliminates infrastructure complexity for self-hosted Prefect and preserves the zero-configuration goal. | Yes | agent |
| D002 | Migration review | api | Drop-in redis-py API compatibility | Match redis.asyncio.Redis method shapes and return types. | Prefect code should not need changes to use the embedded backend. | Yes | agent |
| D003 | Migration review | architecture | Rust + PyO3 for the core engine | Implement the engine in Rust and expose it through PyO3 bindings. | Provides memory safety and performance while keeping Python integration straightforward. | Yes | agent |
| D004 | Migration review | scripting | Use a real Lua engine for EVAL/EVALSHA | Embed mlua rather than reimplementing scripts in custom code. | Prefect’s Lua scripts are complex and are better handled by a maintained interpreter. | Yes | agent |
| D005 | Migration review | concurrency | Use parking_lot RwLock over DashMap | Store the keyspace behind a single parking_lot::RwLock<HashMap>. | Atomic multi-key operations and Lua scripts are simpler and safer with consistent locking. | Yes | agent |
| D006 | Migration review | persistence | Use a custom MessagePack persistence format | Persist snapshots with serde + rmp-serde instead of RDB/AOF compatibility. | The project does not need Redis wire compatibility and benefits from a simpler, self-describing format. | Yes | agent |
| D007 | Migration review | data-structures | Represent sorted sets with a dual index | Use BTreeMap plus HashMap for sorted sets. | This gives efficient range queries and fast member lookups in a single-writer embedded setting. | Yes | agent |
| D008 | Migration review | python | Use pure-Python wrapper classes around Rust dispatch | Keep Python-side wrappers for pipeline and lock compatibility instead of expanding Rust APIs too much. | This matches redis-py’s shape while keeping the Rust core focused on storage and command semantics. | Yes | agent |
| D009 | Migration review | distribution | Target a four-platform wheel matrix | Build Linux x86_64/aarch64 and macOS x86_64/arm64 wheels first. | Covers the deployment surface needed by Prefect while deferring Windows support. | Yes | agent |
| D010 | Migration review | errors | burner_redis.NoScriptError should subclass redis.exceptions.NoScriptError when available | Preserve compatibility with redis-py exception handling by resolving burner_redis first and falling back to redis.exceptions. | Callers can catch either exception form without changing their code. | Yes | agent |
