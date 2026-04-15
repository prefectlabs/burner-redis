---
phase: quick
plan: 260414-tgx
type: execute
wave: 1
depends_on: []
files_modified:
  - src/lib.rs
  - src/scripting.rs
autonomous: true
must_haves:
  truths:
    - "xinfo_groups() returns dicts with string keys (not bytes keys)"
    - "xinfo_consumers() returns dicts with string keys (not bytes keys)"
    - "ZRANGEBYSCORE in Lua scripts supports WITHSCORES flag returning interleaved member/score array"
    - "ZRANGEBYSCORE in Lua scripts supports LIMIT offset count"
    - "ZRANGE in Lua scripts supports WITHSCORES flag returning interleaved member/score array"
    - "docket test suite passes with REDIS_VERSION=memory"
  artifacts:
    - path: "src/lib.rs"
      provides: "Fixed xinfo_groups and xinfo_consumers dict key types"
      contains: "PyString::new"
    - path: "src/scripting.rs"
      provides: "WITHSCORES and LIMIT support for ZRANGEBYSCORE and ZRANGE in Lua bridge"
      contains: "WITHSCORES"
  key_links:
    - from: "src/lib.rs"
      to: "redis-py xinfo_groups/xinfo_consumers"
      via: "dict key type matching"
      pattern: "PyString::new.*name"
    - from: "src/scripting.rs"
      to: "Redis ZRANGEBYSCORE Lua behavior"
      via: "WITHSCORES/LIMIT flag parsing"
      pattern: "WITHSCORES"
---

<objective>
Fix 3 redis-py compatibility gaps causing docket test failures: (1) xinfo_groups/xinfo_consumers returning bytes dict keys instead of string keys, (2) ZRANGEBYSCORE/ZRANGE in Lua scripting bridge missing WITHSCORES and LIMIT support, (3) verify rate-limit test passes after fix 2.

Purpose: Make burner-redis pass the full docket test suite as a drop-in Redis replacement.
Output: Fixed src/lib.rs and src/scripting.rs with all docket tests passing.
</objective>

<context>
@src/lib.rs
@src/scripting.rs
@src/store.rs
</context>

<tasks>

<task type="auto">
  <name>Task 1: Fix xinfo_groups and xinfo_consumers dict key types from bytes to strings</name>
  <files>src/lib.rs</files>
  <action>
In `src/lib.rs`, both `xinfo_groups` (~line 1441) and `xinfo_consumers` (~line 1500) construct Python dicts with bytes keys using `PyBytes::new(py, b"name")`. redis-py returns string keys: `{"name": b"mygroup", "consumers": 0, ...}`.

Fix by changing all dict KEY construction from `PyBytes::new(py, b"...")` to `PyString::new(py, "...")` for both methods. Dict VALUES remain unchanged (bytes values stay as bytes, int values stay as int).

For `xinfo_groups`, change these 4 dict key insertions:
- `PyBytes::new(py, b"name")` -> `PyString::new(py, "name")`
- `PyBytes::new(py, b"consumers")` -> `PyString::new(py, "consumers")`
- `PyBytes::new(py, b"pending")` -> `PyString::new(py, "pending")`
- `PyBytes::new(py, b"last-delivered-id")` -> `PyString::new(py, "last-delivered-id")`

For `xinfo_consumers`, change these 3 dict key insertions:
- `PyBytes::new(py, b"name")` -> `PyString::new(py, "name")`
- `PyBytes::new(py, b"pending")` -> `PyString::new(py, "pending")`
- `PyBytes::new(py, b"idle")` -> `PyString::new(py, "idle")`

Ensure `pyo3::types::PyString` is accessible (it should already be imported or available via the pyo3::types module).
  </action>
  <verify>
    <automated>cd /Users/alexander/dev/prefectlabs/burner-redis && cargo build --release 2>&1 | tail -5</automated>
  </verify>
  <done>All 7 dict key constructions changed from PyBytes to PyString. Cargo build succeeds.</done>
</task>

<task type="auto">
  <name>Task 2: Add WITHSCORES and LIMIT support to ZRANGEBYSCORE and ZRANGE in Lua scripting bridge</name>
  <files>src/scripting.rs</files>
  <action>
In `src/scripting.rs`, both the ZRANGEBYSCORE handler (~line 962) and the ZRANGE handler (~line 900) only return member names without supporting WITHSCORES or LIMIT flags.

**For ZRANGEBYSCORE (line ~962):**

After parsing key, min, max from args[0..2], parse optional flags from remaining args (args[3..]):
1. Scan for "WITHSCORES" (case-insensitive) -- set a `withscores` boolean flag
2. Scan for "LIMIT" (case-insensitive) followed by two args: offset (usize) and count (i64, where -1 means unlimited) -- store as Option<(usize, i64)>

After collecting the base result vector of members in score order:
- If LIMIT is present, apply `.skip(offset).take(count)` (if count is -1, take all after skip)
- If WITHSCORES is present, instead of collecting just `RedisValue::BulkString(member)`, interleave: for each member, emit `[member, score_as_string]` flat in the array. Use `score.0.to_string()` for the score string (OrderedFloat's Display gives minimal representation matching Redis, e.g. "1234.5" not "1234.500000")

The result should be a flat `RedisValue::Array` like Redis returns to Lua: `[member1, score1_str, member2, score2_str, ...]`

**For ZRANGE (line ~900):**

After parsing key, start, stop from args[0..2], parse optional WITHSCORES flag from remaining args (args[3..]):
1. Scan for "WITHSCORES" (case-insensitive) -- set a `withscores` boolean flag

After collecting the base result (members in index order):
- If WITHSCORES is present, interleave scores the same way: emit `[member, score_as_string, ...]` flat in the array using `score.0.to_string()`

Change the `.map(|((_, member), _)| ...)` closure to `.flat_map(|((score, member), _)| { ... })` that returns either `vec![BulkString(member)]` or `vec![BulkString(member), BulkString(score_string)]` depending on the withscores flag.

**Flag parsing pattern (for both commands):**
```rust
let mut withscores = false;
let mut limit: Option<(usize, i64)> = None;
let mut i = 3; // or wherever optional args start
while i < args.len() {
    let flag = String::from_utf8_lossy(&args[i]).to_uppercase();
    match flag.as_str() {
        "WITHSCORES" => { withscores = true; i += 1; }
        "LIMIT" => {
            if i + 2 < args.len() {
                let offset: usize = String::from_utf8_lossy(&args[i+1]).parse().unwrap_or(0);
                let count: i64 = String::from_utf8_lossy(&args[i+2]).parse().unwrap_or(-1);
                limit = Some((offset, count));
                i += 3;
            } else {
                i += 1;
            }
        }
        _ => { i += 1; }
    }
}
```

For ZRANGE, only parse WITHSCORES (LIMIT is not standard for ZRANGE by index).
  </action>
  <verify>
    <automated>cd /Users/alexander/dev/prefectlabs/burner-redis && cargo build --release 2>&1 | tail -5 && cargo test 2>&1 | tail -10</automated>
  </verify>
  <done>ZRANGEBYSCORE in scripting.rs supports WITHSCORES (returns flat interleaved array) and LIMIT (offset+count). ZRANGE in scripting.rs supports WITHSCORES. All existing tests pass.</done>
</task>

<task type="auto">
  <name>Task 3: Build wheel, install into docket, and run full docket test suite</name>
  <files></files>
  <action>
Build the release wheel and install into docket's environment, then run the full verification sequence:

1. Build and install:
```
cd /Users/alexander/dev/prefectlabs/burner-redis && maturin develop --release
cd /Users/alexander/dev/chrisguidry/docket && uv pip install --reinstall /Users/alexander/dev/prefectlabs/burner-redis
```

2. Run Task 1 verification (xinfo tests):
```
cd /Users/alexander/dev/chrisguidry/docket && REDIS_VERSION=memory uv run pytest tests/worker/test_bootstrap.py::test_consumer_group_created_on_first_worker_read tests/worker/test_bootstrap.py::test_worker_handles_nogroup_in_xreadgroup tests/test_docket_clear.py::test_ensure_stream_and_group_is_idempotent -v --timeout=30 -o "addopts=--import-mode=importlib"
```

3. Run Task 2 verification (ZRANGEBYSCORE WITHSCORES/LIMIT):
```
cd /Users/alexander/dev/chrisguidry/docket && REDIS_VERSION=memory uv run pytest tests/test_redelivery.py::test_lease_renewal_recovers_from_redis_error -v --timeout=30 -o "addopts=--import-mode=importlib"
```

4. Run Task 3 verification (rate-limit test):
```
cd /Users/alexander/dev/chrisguidry/docket && REDIS_VERSION=memory uv run pytest tests/test_ratelimit.py::test_drop_false_excess_eventually_executes -v --timeout=30 -o "addopts=--import-mode=importlib"
```

5. If the rate-limit test fails, investigate the error output to identify the remaining compatibility gap and fix it. Likely candidates: another missing Lua command flag, a response format mismatch, or a type coercion issue.

6. Run full docket test suite:
```
cd /Users/alexander/dev/chrisguidry/docket && REDIS_VERSION=memory uv run pytest --timeout=30 -o "addopts=--import-mode=importlib"
```

If any individual test fails, investigate the specific error and fix the root cause before proceeding to the full suite run.
  </action>
  <verify>
    <automated>cd /Users/alexander/dev/chrisguidry/docket && REDIS_VERSION=memory uv run pytest --timeout=30 -o "addopts=--import-mode=importlib" 2>&1 | tail -20</automated>
  </verify>
  <done>All targeted docket tests pass. Full docket test suite with REDIS_VERSION=memory shows no regressions from these changes.</done>
</task>

</tasks>

<verification>
1. `cargo build --release` succeeds with no errors
2. `cargo test` -- all existing Rust tests pass
3. docket xinfo tests pass (test_consumer_group_created_on_first_worker_read, test_worker_handles_nogroup_in_xreadgroup, test_ensure_stream_and_group_is_idempotent)
4. docket ZRANGEBYSCORE test passes (test_lease_renewal_recovers_from_redis_error)
5. docket rate-limit test passes (test_drop_false_excess_eventually_executes)
6. Full docket test suite passes with REDIS_VERSION=memory
</verification>

<success_criteria>
All 3 compatibility gaps fixed. Full docket test suite passes with REDIS_VERSION=memory and no regressions in the burner-redis Rust test suite.
</success_criteria>
