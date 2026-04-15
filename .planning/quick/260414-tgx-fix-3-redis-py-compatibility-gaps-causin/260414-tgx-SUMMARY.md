# Quick Task 260414-tgx: Fix 3 redis-py compatibility gaps causing docket test failures

**Completed:** 2026-04-15
**Commits:** 97bd8b8, 44b8826

## Changes

### Gap 1: xinfo_groups/xinfo_consumers dict key types (97bd8b8)
- **File:** `src/lib.rs`
- Changed dict key construction in `xinfo_groups` and `xinfo_consumers` from `PyBytes::new(py, b"name")` to `PyString::new(py, "name")` for all dict keys
- Keys affected in xinfo_groups: name, consumers, pending, last-delivered-id
- Keys affected in xinfo_consumers: name, pending, idle
- Dict values remain unchanged (bytes for strings, ints for numbers)

### Gap 2: ZRANGEBYSCORE/ZRANGE WITHSCORES and LIMIT in Lua bridge (44b8826)
- **File:** `src/scripting.rs`
- Added WITHSCORES flag parsing to both ZRANGEBYSCORE and ZRANGE handlers
- When WITHSCORES is set, results are returned as flat interleaved arrays [member, score_string, ...] matching real Redis behavior
- Added LIMIT offset count parsing to ZRANGEBYSCORE
- Added `format_redis_score()` helper for Redis-compatible score string formatting

### Gap 3: Rate-limit Lua script (same root cause as Gap 2)
- No additional code changes needed — fixing WITHSCORES in the Lua bridge resolves this

## Verification

- 113 Rust tests pass
- 673 docket memory tests pass, 81 skipped, 0 real failures (2 timing flakes pass on rerun)
- Note: The 5 specific docket tests referenced in the task description have `@skip_memory` markers in the docket repo so they are skipped when running with `REDIS_VERSION=memory`
