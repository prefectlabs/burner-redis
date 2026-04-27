---
phase: 06-lua-scripting
plan: 01
subsystem: scripting-engine
tags: [lua, scripting, mlua, eval, evalsha, dispatch]
dependency_graph:
  requires: []
  provides: [lua-engine, script-cache, redis-call-dispatch]
  affects: [store, commands]
tech_stack:
  added: [mlua-0.10-vendored-lua54, sha1-0.10]
  patterns: [refcell-interior-mutability, lua-scope-functions, fresh-vm-per-execution]
key_files:
  created:
    - src/scripting.rs
  modified:
    - Cargo.toml
    - src/store.rs
    - src/lib.rs
decisions:
  - Used mlua 0.10 with vendored feature to avoid system Lua dependency
  - RefCell pattern for mutable data access within Lua scope closures
  - dispatch_command operates directly on raw HashMap for atomicity under single write lock
  - Lock ordering enforced -- scripts lock released before data lock acquired (deadlock prevention)
metrics:
  duration: 8min
  completed: 2026-04-11
  tasks: 2
  files: 4
---

# Phase 06 Plan 01: Lua Scripting Engine Summary

Embedded Lua 5.4 via mlua with full redis.call()/redis.pcall() dispatch to all 19 supported Redis commands, script caching by SHA1, and atomic eval/evalsha execution under Store write lock.

## Commits

| Task | Name | Commit | Key Files |
|------|------|--------|-----------|
| 1 | Add mlua/sha1 dependencies and create scripting.rs with LuaEngine and redis.call() dispatch | 2e264f1 | Cargo.toml, src/scripting.rs, src/lib.rs |
| 2 | Add script cache to Store and implement eval/evalsha/script_load/script_exists methods | f4166c3 | src/store.rs |

## Implementation Details

### LuaEngine (src/scripting.rs)

- **RedisValue enum**: Protocol-level return type (BulkString, Integer, Array, Nil, Error, Status) with bidirectional Lua-Redis type conversion
- **IntoLua impl**: Converts RedisValue to Lua values (Nil maps to `false` per Redis spec, Status maps to `{ok=...}` table)
- **lua_to_redis_value()**: Converts Lua values back to RedisValue (tables with `err` key become Error, `ok` key become Status, otherwise Array)
- **LuaEngine::sha1_hex()**: SHA1 digest computation for script caching
- **LuaEngine::execute()**: Creates fresh Lua VM per execution, sets up KEYS/ARGV globals, creates redis.call/pcall scoped functions, executes script
- **dispatch_command()**: Routes 19 Redis commands directly against the write-locked HashMap with passive expiration and type checking

### Store Scripting Methods (src/store.rs)

- **scripts field**: `RwLock<HashMap<String, String>>` for SHA1-to-source cache
- **script_load()**: Computes SHA1, caches script, returns hash
- **script_exists()**: Checks multiple SHAs against cache
- **eval()**: Auto-caches script, acquires data write lock, executes via LuaEngine
- **evalsha()**: Looks up SHA in cache (NOSCRIPT error if missing), acquires data write lock, executes

### Commands Dispatched

GET, SET, DEL, EXISTS, HSET, HGET, HDEL, HVALS, SADD, SMEMBERS, SISMEMBER, SREM, ZADD, ZREM, ZRANGE, ZRANGEBYSCORE, ZREMRANGEBYSCORE, XADD, XREAD

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Added `vendored` feature to mlua dependency**
- **Found during:** Task 1
- **Issue:** mlua without `vendored` feature requires system Lua 5.4 library (`pkg-config --libs lua54` failed)
- **Fix:** Added `vendored` feature to mlua Cargo.toml entry to compile Lua from source
- **Files modified:** Cargo.toml
- **Commit:** 2e264f1

**2. [Rule 1 - Bug] Fixed mlua BorrowedStr/BorrowedBytes type handling**
- **Found during:** Task 1
- **Issue:** mlua 0.10's `LuaString::to_str()` returns `Result<BorrowedStr>` and `as_bytes()` returns `BorrowedBytes`, not compatible with std `unwrap_or("...")` or `String::from_utf8_lossy(&[u8])`
- **Fix:** Used `String::from_utf8_lossy(&s.as_bytes())` pattern with explicit borrow
- **Files modified:** src/scripting.rs
- **Commit:** 2e264f1

## Decisions Made

1. **mlua 0.10 with vendored Lua 5.4** -- Avoids system library dependency; plan suggested 0.10 or 0.11; 0.10 works correctly with vendored feature
2. **RefCell for mutable data in Lua scope** -- mlua's `scope()` creates functions that can borrow local data; RefCell enables interior mutability for dispatch_command access
3. **Direct HashMap dispatch (not Store methods)** -- dispatch_command operates on raw `&mut HashMap<Bytes, ValueEntry>` rather than calling Store methods, because Store methods acquire their own locks. The caller already holds the write lock.
4. **Lock ordering: scripts before data** -- evalsha acquires scripts read lock, clones the script, releases it, THEN acquires data write lock. Prevents deadlock.

## Known Stubs

None -- all dispatched commands have full implementations.

## Threat Surface Scan

No new threat surfaces beyond those documented in the plan's threat model. The dispatch_command function explicitly rejects unknown commands (T-06-03 mitigation), and fresh Lua VMs prevent state leakage (T-06-01 mitigation).
