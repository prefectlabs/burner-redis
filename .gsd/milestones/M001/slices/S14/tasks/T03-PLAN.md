# T03: Complete the phase by wiring list commands through the Lua scripting engine and the pipeline subsystem.

**Slice:** S14 — **Milestone:** M001

## Description

Complete the phase by wiring list commands through the Lua scripting engine and the pipeline subsystem. Three integrations:

1. **Lua dispatch** — Extend `dispatch_command` tuple with `had_list_mutation` flag; add 13 non-blocking list command arms to `dispatch_command_inner`; reject BRPOP/BLPOP/BLMOVE with canonical Redis error; wire `Store::eval` and `Store::evalsha` to fire `list_notify.notify_waiters()` after Lua mutations (Phase-11-style wake-up race fix).
2. **Pipeline integration** — Add 13 non-blocking arms to `dispatch_pipeline_command` (sync fast path) and extend `execute_pipeline` with a blocking-aware branch: when the queue contains BRPOP/BLPOP/BLMOVE, fall through to a per-command async path (preserves the sync fast-path from quick task 260415-an2 for non-blocking pipelines).
3. **Python pipeline stubs** — Add 16 new stub methods to `python/burner_redis/pipeline.py`.

Plus finalize REQUIREMENTS.md (mark LIST-01..LIST-16 as Complete in Traceability) and add LIST-16 integration tests (Lua + pipeline).

Purpose: The list subsystem is a drop-in only if commands work through every execution surface — direct Python, Lua scripts, and Pipelines. Skipping any one of these creates a compat gap. The `had_list_mutation` flag is the specific fix for the Phase-11-style lost-wakeup race (BRPOP waiter parked while Lua does LPUSH).

Output: Full 16-command coverage across all three surfaces. REQUIREMENTS.md finalized. Test suite covers the integration edge cases (Lua-to-BRPOP wake, pipeline blocking mix, pipeline fast-path non-regression).

## Legacy Source

---
phase: 14-add-support-for-the-redis-list-data-type-required-commands-l
plan: 03
type: execute
wave: 3
depends_on:
  - "14-01"
  - "14-02"
files_modified:
  - src/scripting.rs
  - src/store.rs
  - src/lib.rs
  - python/burner_redis/pipeline.py
  - tests/test_lists.py
  - .planning/REQUIREMENTS.md
autonomous: true
requirements:
  - LIST-16
tags:
  - lua
  - pipeline
  - scripting
  - integration

must_haves:
  truths:
    - "Lua scripts can call redis.call('LPUSH'/'RPUSH'/'LPOP'/'RPOP'/'LRANGE'/'LLEN'/'LINDEX'/'LINSERT'/'LREM'/'LSET'/'LTRIM'/'LMOVE'/'RPOPLPUSH', ...) and receive correct return values"
    - "Lua scripts calling BLPOP/BRPOP/BLMOVE receive 'ERR This Redis command is not allowed from scripts: <cmd>' matching real Redis"
    - "LPUSH/RPUSH/LMOVE/RPOPLPUSH/LINSERT called from a Lua script fires list_notify.notify_waiters() after script execution; BRPOP waiters wake"
    - "Pipeline with only non-blocking list commands uses the synchronous dispatch_pipeline_command fast path (preserves 260415-an2 perf win)"
    - "Pipeline with any blocking command falls through to per-command async loop; blocking commands respect their per-command timeouts"
    - "python/burner_redis/pipeline.py has 16 new stub methods (lpush, rpush, lpop, rpop, lrange, llen, lindex, linsert, lrem, lset, ltrim, lmove, rpoplpush, blpop, brpop, blmove)"
    - "REQUIREMENTS.md LIST-01..LIST-16 marked complete in Traceability"
    - "All LIST-16 tests pass in tests/test_lists.py (Lua dispatch + Lua-to-BRPOP wake + pipeline mixing)"
  artifacts:
    - path: "src/scripting.rs"
      provides: "dispatch_command returns (RedisValue, had_xadd, had_list_mutation) tuple; dispatch_command_inner has 13 non-blocking list arms + 3 blocking-reject arms"
      contains: "had_list_mutation"
    - path: "src/store.rs"
      provides: "Store::eval + Store::evalsha destructure the 3-tuple and call list_notify.notify_waiters() if had_list_mutation"
      contains: "had_list_mutation"
    - path: "src/lib.rs"
      provides: "execute_pipeline scans for blocking commands; 13 new arms in dispatch_pipeline_command"
      contains: "dispatch_pipeline_command"
    - path: "python/burner_redis/pipeline.py"
      provides: "16 new pipeline stub methods under a List Commands section"
      contains: "def lpush"
    - path: ".planning/REQUIREMENTS.md"
      provides: "LIST-01..LIST-16 marked Complete"
      contains: "Complete"
  key_links:
    - from: "src/scripting.rs dispatch_command return tuple"
      to: "src/store.rs eval/evalsha call list_notify.notify_waiters()"
      via: "had_list_mutation flag propagation"
      pattern: "had_list_mutation"
    - from: "src/lib.rs execute_pipeline"
      to: "scan for brpop/blpop/blmove then choose fast/async path"
      via: "blocking-aware branch"
      pattern: "brpop|blpop|blmove"
    - from: "src/lib.rs dispatch_pipeline_command"
      to: "13 new arms for non-blocking list commands"
      via: "match method_name"
      pattern: "lpush|rpush|lrange"
---

<objective>
Complete the phase by wiring list commands through the Lua scripting engine and the pipeline subsystem. Three integrations:

1. **Lua dispatch** — Extend `dispatch_command` tuple with `had_list_mutation` flag; add 13 non-blocking list command arms to `dispatch_command_inner`; reject BRPOP/BLPOP/BLMOVE with canonical Redis error; wire `Store::eval` and `Store::evalsha` to fire `list_notify.notify_waiters()` after Lua mutations (Phase-11-style wake-up race fix).
2. **Pipeline integration** — Add 13 non-blocking arms to `dispatch_pipeline_command` (sync fast path) and extend `execute_pipeline` with a blocking-aware branch: when the queue contains BRPOP/BLPOP/BLMOVE, fall through to a per-command async path (preserves the sync fast-path from quick task 260415-an2 for non-blocking pipelines).
3. **Python pipeline stubs** — Add 16 new stub methods to `python/burner_redis/pipeline.py`.

Plus finalize REQUIREMENTS.md (mark LIST-01..LIST-16 as Complete in Traceability) and add LIST-16 integration tests (Lua + pipeline).

Purpose: The list subsystem is a drop-in only if commands work through every execution surface — direct Python, Lua scripts, and Pipelines. Skipping any one of these creates a compat gap. The `had_list_mutation` flag is the specific fix for the Phase-11-style lost-wakeup race (BRPOP waiter parked while Lua does LPUSH).

Output: Full 16-command coverage across all three surfaces. REQUIREMENTS.md finalized. Test suite covers the integration edge cases (Lua-to-BRPOP wake, pipeline blocking mix, pipeline fast-path non-regression).
</objective>

<execution_context>
@$HOME/.claude/get-shit-done/workflows/execute-plan.md
@$HOME/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@.planning/PROJECT.md
@.planning/ROADMAP.md
@.planning/STATE.md
@.planning/REQUIREMENTS.md
@.planning/phases/14-add-support-for-the-redis-list-data-type-required-commands-l/14-CONTEXT.md
@.planning/phases/14-add-support-for-the-redis-list-data-type-required-commands-l/14-RESEARCH.md
@.planning/phases/14-add-support-for-the-redis-list-data-type-required-commands-l/14-PATTERNS.md
@.planning/phases/14-add-support-for-the-redis-list-data-type-required-commands-l/14-VALIDATION.md
@.planning/phases/14-add-support-for-the-redis-list-data-type-required-commands-l/14-01-PLAN.md
@.planning/phases/14-add-support-for-the-redis-list-data-type-required-commands-l/14-02-PLAN.md
@src/scripting.rs
@src/store.rs
@src/lib.rs
@python/burner_redis/pipeline.py
@tests/test_lists.py

<interfaces>
src/scripting.rs key locations (pre-edit line numbers):
- line 117 — `LuaEngine::execute` returns `Result<(RedisValue, bool), String>`
- line 126 — `had_xadd: Cell<bool>` tracking
- line 184 — redis.call closure destructures `Ok((val, xadd_flag)) =>`
- line 234 — redis.pcall closure destructures `Ok((val, xadd_flag)) =>`
- line 260 — `scope_result.map(|v| (v, had_xadd.get()))`
- lines 268-279 — `dispatch_command` returns `Result<(RedisValue, bool), String>`
- lines 684-713 — `SADD` arm template for variadic non-blocking
- lines 1261-1337 — `XADD` arm template for mutating
- line ~1808 — catch-all `_ => Ok(RedisValue::Error(format!("ERR unknown command '{}'", cmd)))`

src/store.rs (pre-edit) — around lines 2387-2405 (eval) and 2410-2433 (evalsha):
```rust
let (result, had_xadd) = {
    let mut data = self.data.write();
    LuaEngine::execute(script, keys, args, &mut *data, Some(&pubsub_tx))?
};
if had_xadd { self.stream_notify.notify_waiters(); }
```

src/lib.rs (pre-edit):
- lines 2182-2197 — `execute_pipeline` sync fast path (to be extended with blocking-aware branch)
- lines 2200+ — `dispatch_pipeline_command` match on method_name
- lines 2322-2328 — `"sadd"` arm (variadic template)
- lines 2252-2269 — `"hset"` arm (kwarg template)

python/burner_redis/pipeline.py — existing stub style:
```python
def sadd(self, name, *values):
    self._commands.append(("sadd", (name, *values), {}))
    return self
```
Section headers are comments like `# ---- String Commands ----`, `# ---- Hash Commands ----`, etc.
</interfaces>
</context>

<tasks>

<task type="auto" tdd="true">
  <name>Task 1: Lua integration — extend dispatch_command tuple with had_list_mutation + add 13 non-blocking + 3 blocking-reject arms</name>
  <read_first>
    - src/scripting.rs (full file — especially lines 117, 126, 184, 234, 260, 268-279, SADD arm at 684-713, XADD arm at 1261-1337, catch-all at ~1808)
    - src/store.rs (eval/evalsha blocks around lines 2387-2405 and 2410-2433; list methods from Plan 01)
    - .planning/phases/14-add-support-for-the-redis-list-data-type-required-commands-l/14-PATTERNS.md (sections "dispatch_command tuple extension" and "dispatch_command_inner arms")
    - .planning/phases/14-add-support-for-the-redis-list-data-type-required-commands-l/14-RESEARCH.md (Lua dispatch extension; Pitfall 1: lost wake-up from Lua LPUSH; Assumptions Log A2 on which commands set had_list_mutation)
    - .planning/phases/14-add-support-for-the-redis-list-data-type-required-commands-l/14-CONTEXT.md (D-12, D-13, D-14)
  </read_first>
  <behavior>
    - `dispatch_command` returns `Result<(RedisValue, bool, bool), String>`; third bool = had_list_mutation
    - `LuaEngine::execute` returns `Result<(RedisValue, bool, bool), String>`
    - `Store::eval` and `Store::evalsha` destructure the 3-tuple; if `had_list_mutation` is true, call `self.list_notify.notify_waiters()` after dropping the data lock
    - `dispatch_command_inner` handles LPUSH/RPUSH/LPOP/RPOP/LRANGE/LLEN/LINDEX/LINSERT/LREM/LSET/LTRIM/LMOVE/RPOPLPUSH with correct RedisValue returns
    - `dispatch_command_inner` rejects BLPOP/BRPOP/BLMOVE with `RedisValue::Error("ERR This Redis command is not allowed from scripts: <cmd>")`
    - `had_list_mutation` is true when cmd is one of LPUSH/RPUSH/LMOVE/RPOPLPUSH/LINSERT AND the result is NOT a RedisValue::Error
  </behavior>
  <action>
**Part A — Extend `dispatch_command` return tuple in src/scripting.rs (lines 268-279):**

Replace with:
```rust
fn dispatch_command(
    cmd: &str,
    args: &[Bytes],
    data: &mut HashMap<Bytes, ValueEntry>,
    pubsub_tx: Option<&broadcast::Sender<PubSubMessage>>,
) -> Result<(RedisValue, bool, bool), String> {
    let is_xadd = cmd == "XADD";
    // Per Assumptions Log A2 in RESEARCH.md: only list-GROW ops wake waiters.
    // LINSERT CAN grow a non-empty list. LPOP/RPOP/LREM/LTRIM/LSET never grow.
    let is_list_write = matches!(
        cmd,
        "LPUSH" | "RPUSH" | "LMOVE" | "RPOPLPUSH" | "LINSERT"
    );
    let result = dispatch_command_inner(cmd, args, data, pubsub_tx)?;
    let success = !matches!(result, RedisValue::Error(_));
    let had_xadd = is_xadd && success;
    let had_list_mutation = is_list_write && success;
    Ok((result, had_xadd, had_list_mutation))
}
```

**Part B — Update `LuaEngine::execute` signature (line 117):** change return type from `Result<(RedisValue, bool), String>` to `Result<(RedisValue, bool, bool), String>`.

**Part C — Add parallel `Cell<bool>` (around line 126):**
```rust
let had_xadd: Cell<bool> = Cell::new(false);
let had_list_mutation: Cell<bool> = Cell::new(false);
```

**Part D — Update redis.call closure (line ~184) and redis.pcall closure (line ~234):** Each closure currently has `match dispatch_command(...) { Ok((val, xadd_flag)) => { ... } }`. Update to `Ok((val, xadd_flag, list_mut_flag)) => { if xadd_flag { had_xadd.set(true); } if list_mut_flag { had_list_mutation.set(true); } ... }`.

**Part E — Update final return (line 260):** Change `scope_result.map(|v| (v, had_xadd.get()))` to `scope_result.map(|v| (v, had_xadd.get(), had_list_mutation.get()))`.

**Part F — Update `Store::eval` and `Store::evalsha` in src/store.rs (around lines 2387-2405 and 2410-2433):**
```rust
let (result, had_xadd, had_list_mutation) = {
    let mut data = self.data.write();
    LuaEngine::execute(script, keys, args, &mut *data, Some(&pubsub_tx))?
};
if had_xadd { self.stream_notify.notify_waiters(); }
if had_list_mutation { self.list_notify.notify_waiters(); }
```
Apply to BOTH eval and evalsha.

**Part G — Add 13 non-blocking + 3 blocking-reject arms in `dispatch_command_inner`** (insert before the catch-all at line ~1808). Use the SADD arm at lines 684-713 as the variadic template. Arms to add:

1. `"LPUSH"` — variadic; passive-expire; `entry.or_insert_with(ValueEntry::new_list)`; `ValueData::List` arm pushes `push_front(v.clone())` for each arg; return `RedisValue::Integer(list.len() as i64)`; `_` arm returns `RedisValue::Error("WRONGTYPE ...")`.
2. `"RPUSH"` — same but `push_back`.
3. `"LPOP"` — parse optional count from args[1] (integer string); type-check FIRST (returns Nil on missing key regardless of count; WRONGTYPE on non-list); count=0 fast-return `RedisValue::Array(Vec::new())` ONLY after type check; otherwise pop and return `BulkString` (no count) or `Array(Vec<BulkString>)` (count). D-03: delete key if empty.
4. `"RPOP"` — mirror of LPOP with `pop_back`.
5. `"LRANGE"` — parse start/stop i64 args; use `crate::commands::lists::normalize_range_indices`; return `RedisValue::Array` of `BulkString`s.
6. `"LLEN"` — return `RedisValue::Integer`; 0 on missing key.
7. `"LINDEX"` — parse i64 index; support negative; return `BulkString` or `Nil` on out-of-range/missing.
8. `"LINSERT"` — parse where via `crate::commands::lists::parse_linsert_where`; return new length, -1 for pivot-not-found, 0 for missing key.
9. `"LREM"` — parse count i64 via `crate::commands::lists::parse_lrem_count`; follow Plan 01 Task 4 LremDirection semantics; return `RedisValue::Integer(removed)`; 0 on missing key; D-03 delete on empty.
10. `"LSET"` — parse index i64; return `RedisValue::SimpleString("OK")` (matches redis-py "OK" → True); `RedisValue::Error("ERR index out of range")` on out-of-range; `RedisValue::Error("ERR no such key")` on missing key.
11. `"LTRIM"` — parse start/stop; delete key if normalized range is empty; return `RedisValue::SimpleString("OK")`.
12. `"LMOVE"` — parse src/dst from args[0..=1], directions from args[2..=3] via `parse_list_end`; perform pop+push under the already-held write lock; D-03 delete src if empty; return `BulkString` or `Nil`.
13. `"RPOPLPUSH"` — equivalent to `LMOVE src dst RIGHT LEFT`.

**Reference the Store method bodies from Plan 01 Tasks 3-5.** Copy the match-arm bodies, removing `let mut data = self.data.write();` (caller already holds the write lock via `LuaEngine::execute`) and removing `self.list_notify.notify_waiters();` (notify fires after Lua execution via the `had_list_mutation` flag).

**Blocking-reject arms** (insert before the catch-all):
```rust
"BLPOP" | "BRPOP" | "BLMOVE" => {
    Ok(RedisValue::Error(format!(
        "ERR This Redis command is not allowed from scripts: {}",
        cmd
    )))
}
```

**NOTE on RedisValue variants:** Use whatever the existing enum offers. If the codebase uses `RedisValue::BulkString(Bytes)` vs `RedisValue::BulkString(Vec<u8>)`, match the existing call pattern shown in the SADD/XADD arms.
  </action>
  <verify>
    <automated>cargo build --lib 2>&amp;1 | tail -15; uv run maturin develop 2>&amp;1 | tail -5; cargo test --lib scripting 2>&amp;1 | tail -20; uv run python -c "$(cat <<'PYEOF'
import asyncio
from burner_redis import BurnerRedis

async def t():
    r = BurnerRedis()
    result = await r.eval(
        "redis.call('LPUSH', KEYS[1], 'a', 'b', 'c'); return redis.call('LRANGE', KEYS[1], 0, -1)",
        1, "k",
    )
    assert result == [b"c", b"b", b"a"], f"got {result}"
    try:
        await r.eval("return redis.call('BLPOP', KEYS[1], 0)", 1, "k")
        assert False, "BLPOP should have raised"
    except Exception as e:
        assert "not allowed from scripts" in str(e), f"bad error: {e}"
    print("PASS-TASK1")

asyncio.run(t())
PYEOF
)" 2>&amp;1 | tail -5</automated>
  </verify>
  <acceptance_criteria>
    - `cargo build --lib` exits 0
    - `uv run maturin develop` exits 0
    - `cargo test --lib scripting` — no regression (all prior Lua tests still pass)
    - `grep -q "had_list_mutation" src/scripting.rs` returns 0
    - `grep -q "had_list_mutation" src/store.rs` returns 0
    - `grep -c "self.list_notify.notify_waiters()" src/store.rs` returns at least 3 (one in shutdown from Plan 01, one each in eval and evalsha, plus inline notifies from list Store methods — expect >= 7 total)
    - `grep -q "This Redis command is not allowed from scripts" src/scripting.rs` returns 0
    - `grep -cE "\"LPUSH\"\\s*=>|\"RPUSH\"\\s*=>|\"LPOP\"\\s*=>|\"RPOP\"\\s*=>|\"LRANGE\"\\s*=>|\"LLEN\"\\s*=>|\"LINDEX\"\\s*=>|\"LINSERT\"\\s*=>|\"LREM\"\\s*=>|\"LSET\"\\s*=>|\"LTRIM\"\\s*=>|\"LMOVE\"\\s*=>|\"RPOPLPUSH\"\\s*=>" src/scripting.rs` returns 13
  </acceptance_criteria>
  <done>Lua dispatch extended with 13 non-blocking list arms + 3 blocking-reject arms. Tuple widened to propagate `had_list_mutation`. Store::eval/evalsha fire list_notify when flag is set. Smoke verification proves Lua LPUSH→LRANGE round-trip and BLPOP rejection.</done>
</task>

<task type="auto" tdd="true">
  <name>Task 2: Pipeline integration — 13 non-blocking arms in dispatch_pipeline_command + blocking-aware branch in execute_pipeline + 16 Python stubs</name>
  <read_first>
    - src/lib.rs (execute_pipeline at lines 2182-2197; dispatch_pipeline_command starting at line 2200+; sadd arm at 2322-2328; hset arm at 2252-2269; xadd arm around 2340+)
    - python/burner_redis/pipeline.py (full file — existing stub pattern; section header comments)
    - .planning/phases/14-add-support-for-the-redis-list-data-type-required-commands-l/14-PATTERNS.md (sections "dispatch_pipeline_command sync arms", "execute_pipeline blocking-aware branch", "python/burner_redis/pipeline.py")
    - .planning/phases/14-add-support-for-the-redis-list-data-type-required-commands-l/14-RESEARCH.md (Pipeline stub code example; Pitfall 6: value coercion double-applied)
    - .planning/phases/14-add-support-for-the-redis-list-data-type-required-commands-l/14-CONTEXT.md (D-15, D-16, D-17)
    - .planning/quick/260415-an2-eliminate-async-overhead-with-sync-fast-/ (any SUMMARY.md — for context on the fast-path that must not regress)
  </read_first>
  <behavior>
    - `execute_pipeline` scans the command queue; if none of "brpop"/"blpop"/"blmove" are present, uses the existing sync fast path (unchanged)
    - If any blocking command is present, returns a `future_into_py` async block that iterates commands and invokes non-blocking ones via `dispatch_pipeline_command` and blocking ones via the normal awaitable pymethod on `BurnerRedis`
    - `dispatch_pipeline_command` has 13 new arms for LPUSH/RPUSH/LPOP/RPOP/LRANGE/LLEN/LINDEX/LINSERT/LREM/LSET/LTRIM/LMOVE/RPOPLPUSH
    - `python/burner_redis/pipeline.py` has 16 new stub methods buffering `(method_name, args, kwargs)`
    - Non-blocking all-list pipeline completes in <50ms (fast-path preserved)
    - Blocking-in-pipeline respects its own timeout; subsequent commands execute after the block resolves
  </behavior>
  <action>
**Part A — Add 13 new arms to `dispatch_pipeline_command` in src/lib.rs** (insert before the catch-all "unknown command" arm). Use the `sadd` arm at lines 2322-2328 as the template for variadic; `hset` at 2252-2269 for kwarg. For each list command, the arm extracts args from the `args: &Bound<PyTuple>` and `kwargs: &Bound<PyDict>`:

Template for variadic (lpush/rpush):
```rust
"lpush" => {
    let name = args.get_item(0)?;
    let name_bytes = extract_bytes(&name)?;
    let vals: Vec<Bytes> = args.iter().skip(1)
        .map(|obj| extract_bytes(&obj))
        .collect::<PyResult<Vec<_>>>()?;
    let count = self.store.lpush(name_bytes, vals).map_err(store_err_to_py)?;
    Ok(count.into_pyobject(py)?.into_any().unbind())
}
"rpush" => { /* same with store.rpush */ }
```

Template for positional-only (lrange, llen, lindex, lset, ltrim, lrem, lmove, rpoplpush):
```rust
"lrange" => {
    let name = args.get_item(0)?;
    let start: i64 = args.get_item(1)?.extract()?;
    let end: i64 = args.get_item(2)?.extract()?;
    let key = extract_bytes(&name)?;
    let elems = self.store.lrange(&key, start, end).map_err(store_err_to_py)?;
    let py_list = pyo3::types::PyList::empty(py);
    for v in elems { py_list.append(pyo3::types::PyBytes::new(py, &v))?; }
    Ok(py_list.into_any().unbind())
}
"llen" => {
    let name = args.get_item(0)?;
    let key = extract_bytes(&name)?;
    let n = self.store.llen(&key).map_err(store_err_to_py)?;
    Ok(n.into_pyobject(py)?.into_any().unbind())
}
"lindex" => {
    let name = args.get_item(0)?;
    let index: i64 = args.get_item(1)?.extract()?;
    let key = extract_bytes(&name)?;
    let result = self.store.lindex(&key, index).map_err(store_err_to_py)?;
    let py_result = match result {
        Some(b) => pyo3::types::PyBytes::new(py, &b).into_any().unbind(),
        None => py.None(),
    };
    Ok(py_result)
}
"lset" => {
    let name = args.get_item(0)?;
    let index: i64 = args.get_item(1)?.extract()?;
    let value = args.get_item(2)?;
    let key = extract_bytes(&name)?;
    let val = extract_bytes(&value)?;
    self.store.lset(&key, index, val).map_err(store_err_to_py)?;
    Ok(pyo3::types::PyBool::new(py, true).to_owned().into_any().unbind())
}
"ltrim" => {
    let name = args.get_item(0)?;
    let start: i64 = args.get_item(1)?.extract()?;
    let end: i64 = args.get_item(2)?.extract()?;
    let key = extract_bytes(&name)?;
    self.store.ltrim(&key, start, end).map_err(store_err_to_py)?;
    Ok(pyo3::types::PyBool::new(py, true).to_owned().into_any().unbind())
}
"lrem" => {
    let name = args.get_item(0)?;
    let count: i64 = args.get_item(1)?.extract()?;
    let value = args.get_item(2)?;
    let key = extract_bytes(&name)?;
    let val = extract_bytes(&value)?;
    let n = self.store.lrem(&key, count, val).map_err(store_err_to_py)?;
    Ok(n.into_pyobject(py)?.into_any().unbind())
}
"linsert" => {
    // Args shape: (name, where, refvalue, value) per redis-py
    let name = args.get_item(0)?;
    let r#where: String = args.get_item(1)?.extract()?;
    let refvalue = args.get_item(2)?;
    let value = args.get_item(3)?;
    let key = extract_bytes(&name)?;
    let pivot = extract_bytes(&refvalue)?;
    let val = extract_bytes(&value)?;
    let position = crate::commands::lists::parse_linsert_where(&r#where).map_err(store_err_to_py)?;
    let n = self.store.linsert(&key, position, &pivot, val).map_err(store_err_to_py)?;
    Ok(n.into_pyobject(py)?.into_any().unbind())
}
"lmove" => {
    // Pipeline stub buffers (first_list, second_list) positional + {"src", "dest"} kwargs
    let first = args.get_item(0)?;
    let second = args.get_item(1)?;
    let src_str: String = kwargs.get_item("src")?
        .and_then(|v| if v.is_none() { None } else { Some(v) })
        .map(|v| v.extract::<String>())
        .transpose()?
        .unwrap_or_else(|| "LEFT".to_string());
    let dest_str: String = kwargs.get_item("dest")?
        .and_then(|v| if v.is_none() { None } else { Some(v) })
        .map(|v| v.extract::<String>())
        .transpose()?
        .unwrap_or_else(|| "RIGHT".to_string());
    let src_key = extract_bytes(&first)?;
    let dst_key = extract_bytes(&second)?;
    let src_end = crate::commands::lists::parse_list_end(&src_str).map_err(store_err_to_py)?;
    let dst_end = crate::commands::lists::parse_list_end(&dest_str).map_err(store_err_to_py)?;
    let result = self.store.lmove_atomic(&src_key, &dst_key, src_end, dst_end).map_err(store_err_to_py)?;
    let py_result = match result {
        Some(b) => pyo3::types::PyBytes::new(py, &b).into_any().unbind(),
        None => py.None(),
    };
    Ok(py_result)
}
"rpoplpush" => {
    let src = args.get_item(0)?;
    let dst = args.get_item(1)?;
    let src_key = extract_bytes(&src)?;
    let dst_key = extract_bytes(&dst)?;
    let result = self.store.rpoplpush_atomic(&src_key, &dst_key).map_err(store_err_to_py)?;
    let py_result = match result {
        Some(b) => pyo3::types::PyBytes::new(py, &b).into_any().unbind(),
        None => py.None(),
    };
    Ok(py_result)
}
```

Template for count-kwarg (lpop, rpop):
```rust
"lpop" => {
    let name = args.get_item(0)?;
    let key = extract_bytes(&name)?;
    let count: Option<i64> = kwargs.get_item("count")?
        .and_then(|v| if v.is_none() { None } else { Some(v) })
        .map(|v| v.extract::<i64>())
        .transpose()?;
    let count_opt: Option<usize> = match count {
        None => None,
        Some(n) if n < 0 => return Err(pyo3::exceptions::PyValueError::new_err("count must be non-negative")),
        Some(n) => Some(n as usize),
    };
    let result = self.store.lpop(&key, count_opt).map_err(store_err_to_py)?;
    let py_result = match result {
        crate::store::LPopResult::Nil => py.None(),
        crate::store::LPopResult::Single(b) => pyo3::types::PyBytes::new(py, &b).into_any().unbind(),
        crate::store::LPopResult::Array(vs) => {
            let py_list = pyo3::types::PyList::empty(py);
            for v in vs { py_list.append(pyo3::types::PyBytes::new(py, &v))?; }
            py_list.into_any().unbind()
        }
    };
    Ok(py_result)
}
"rpop" => { /* same with store.rpop */ }
```

**Part B — Add a blocking-aware branch to `Pipeline.execute()` in Python (per D-16):**

Rather than adding a Rust-side "slow path" to `execute_pipeline` (which would require awaiting Python coroutines from within a single Rust future — messy across the PyO3/Tokio boundary), implement the blocking-aware dispatch in Python. The Rust `execute_pipeline` stays purely synchronous and is only invoked for all-non-blocking queues, preserving the quick-task 260415-an2 fast path.

Modify `Pipeline.execute()` in `python/burner_redis/pipeline.py`:
```python
async def execute(self, raise_on_error: bool = True) -> list:
    blocking_cmds = {"brpop", "blpop", "blmove"}
    has_blocking = any(c[0] in blocking_cmds for c in self._commands)

    if not has_blocking:
        # FAST PATH: unchanged, uses Rust sync dispatch
        result = await self.client.execute_pipeline(self._commands)
        # existing raise_on_error handling stays as-is
        if raise_on_error:
            for r in result:
                if isinstance(r, Exception):
                    raise r
        self._commands = []
        return result

    # SLOW PATH: iterate and await each command individually on self.client
    results = []
    for (method_name, args, kwargs) in self._commands:
        try:
            method = getattr(self.client, method_name)
            result = await method(*args, **kwargs)
            results.append(result)
        except Exception as e:
            if raise_on_error:
                self._commands = []
                raise
            results.append(e)
    self._commands = []
    return results
```

With this approach, **no changes to Rust `execute_pipeline` are required beyond the 13 new arms in `dispatch_pipeline_command`.** The blocking-aware branch lives entirely in Python. The Rust side stays focused on the sync fast path (preserving 260415-an2 perf for the common case).


**Part C — Add 16 Python stubs to `python/burner_redis/pipeline.py`:**

Insert a `# ---- List Commands ----` section header near the other data-type sections (e.g. after `# ---- Sorted Set Commands ----` and before `# ---- Stream Commands ----`). Add methods:
```python
# ---- List Commands ----

def lpush(self, name, *values):
    self._commands.append(("lpush", (name, *values), {}))
    return self

def rpush(self, name, *values):
    self._commands.append(("rpush", (name, *values), {}))
    return self

def lpop(self, name, count=None):
    self._commands.append(("lpop", (name,), {"count": count}))
    return self

def rpop(self, name, count=None):
    self._commands.append(("rpop", (name,), {"count": count}))
    return self

def lrange(self, name, start, end):
    self._commands.append(("lrange", (name, start, end), {}))
    return self

def llen(self, name):
    self._commands.append(("llen", (name,), {}))
    return self

def lindex(self, name, index):
    self._commands.append(("lindex", (name, index), {}))
    return self

def linsert(self, name, where, refvalue, value):
    self._commands.append(("linsert", (name, where, refvalue, value), {}))
    return self

def lrem(self, name, count, value):
    self._commands.append(("lrem", (name, count, value), {}))
    return self

def lset(self, name, index, value):
    self._commands.append(("lset", (name, index, value), {}))
    return self

def ltrim(self, name, start, end):
    self._commands.append(("ltrim", (name, start, end), {}))
    return self

def lmove(self, first_list, second_list, src="LEFT", dest="RIGHT"):
    self._commands.append(("lmove", (first_list, second_list), {"src": src, "dest": dest}))
    return self

def rpoplpush(self, src, dst):
    self._commands.append(("rpoplpush", (src, dst), {}))
    return self

def blpop(self, keys, timeout=0):
    self._commands.append(("blpop", (keys,), {"timeout": timeout}))
    return self

def brpop(self, keys, timeout=0):
    self._commands.append(("brpop", (keys,), {"timeout": timeout}))
    return self

def blmove(self, first_list, second_list, timeout, src="LEFT", dest="RIGHT"):
    self._commands.append(("blmove", (first_list, second_list, timeout), {"src": src, "dest": dest}))
    return self
```

**Part D — Update `Pipeline.execute()` in `python/burner_redis/pipeline.py` per Part B's final decision.** Preserve the existing fast-path call to `self.client.execute_pipeline`; add the blocking-aware Python-side branch.
  </action>
  <verify>
    <automated>cargo build --lib 2>&amp;1 | tail -10; uv run maturin develop 2>&amp;1 | tail -5; grep -cE "^    def (lpush|rpush|lpop|rpop|lrange|llen|lindex|linsert|lrem|lset|ltrim|lmove|rpoplpush|blpop|brpop|blmove)" python/burner_redis/pipeline.py; uv run python -c "
import asyncio, time
from burner_redis import BurnerRedis

async def main():
    r = BurnerRedis()
    # Non-blocking pipeline: verify fast path (< 50ms)
    pipe = r.pipeline()
    pipe.lpush('k', 'a', 'b', 'c')
    pipe.llen('k')
    pipe.lrange('k', 0, -1)
    start = time.monotonic()
    results = await pipe.execute()
    elapsed = time.monotonic() - start
    assert elapsed < 0.1, f'fast path too slow: {elapsed}'
    assert results == [3, 3, [b'c', b'b', b'a']], f'got {results}'

    # Blocking pipeline: verify blocking branch respects timeout
    pipe = r.pipeline()
    pipe.set('x', '1')
    pipe.blpop(['missing_key'], timeout=0.1)
    pipe.set('y', '2')
    start = time.monotonic()
    results = await pipe.execute()
    elapsed = time.monotonic() - start
    assert elapsed &gt;= 0.05, f'blocking pipeline did not block: {elapsed}'
    assert results[1] is None, f'blpop should return None on timeout: {results[1]}'
    print('PASS-TASK2')

asyncio.run(main())
" 2>&amp;1 | tail -5</automated>
  </verify>
  <acceptance_criteria>
    - `cargo build --lib` exits 0
    - `uv run maturin develop` exits 0
    - `grep -cE "^    def (lpush|rpush|lpop|rpop|lrange|llen|lindex|linsert|lrem|lset|ltrim|lmove|rpoplpush|blpop|brpop|blmove)" python/burner_redis/pipeline.py` returns 16
    - `grep -cE "\"lpush\"\\s*=>|\"rpush\"\\s*=>|\"lpop\"\\s*=>|\"rpop\"\\s*=>|\"lrange\"\\s*=>|\"llen\"\\s*=>|\"lindex\"\\s*=>|\"linsert\"\\s*=>|\"lrem\"\\s*=>|\"lset\"\\s*=>|\"ltrim\"\\s*=>|\"lmove\"\\s*=>|\"rpoplpush\"\\s*=>" src/lib.rs` returns 13
    - `grep -q "blocking_cmds" python/burner_redis/pipeline.py` returns 0 (blocking-aware branch present)
    - Smoke script prints `PASS-TASK2` — both fast-path timing AND blocking-branch timeout verified
  </acceptance_criteria>
  <done>13 new pipeline dispatch arms in Rust + 16 Python pipeline stubs + blocking-aware Python-side `Pipeline.execute()`. Fast path preserved for non-blocking pipelines; blocking pipelines respect per-command timeouts.</done>
</task>

<task type="auto" tdd="true">
  <name>Task 3: LIST-16 integration tests + REQUIREMENTS.md finalize + full-suite regression sweep</name>
  <read_first>
    - tests/test_lists.py (created in Plan 02 — needs extension for Lua + pipeline tests)
    - tests/test_streams.py (Lua/pipeline integration patterns)
    - tests/test_scripting.py (Lua test patterns)
    - .planning/REQUIREMENTS.md (LIST section and Traceability table — Plan 01 marked LIST-* as "In Progress"; this plan flips them to "Complete")
    - .planning/phases/14-add-support-for-the-redis-list-data-type-required-commands-l/14-PATTERNS.md (test patterns 6 "Lua-to-BRPOP wake-up", 7 "Pipeline mixing blocking + non-blocking", 8 "Pipeline all-non-blocking fast-path preservation")
    - .planning/phases/14-add-support-for-the-redis-list-data-type-required-commands-l/14-RESEARCH.md (Critical Behavioral Test Matrix — specifically Lua wake-up and pipeline mixing rows)
  </read_first>
  <behavior>
    - Lua scripts can invoke all 13 non-blocking list commands and return correct values
    - Lua scripts calling BRPOP/BLPOP/BLMOVE raise an exception containing "not allowed from scripts"
    - LPUSH from inside a Lua script wakes a parked BRPOP waiter (Phase-11-style race regression guard)
    - Non-blocking pipeline mixing list commands completes with all expected results, fast-path timing preserved
    - Blocking pipeline respects per-command timeouts, subsequent commands execute after the block resolves
    - REQUIREMENTS.md LIST-01..LIST-16 Traceability rows show "Complete"
    - `[ ]` checkboxes on LIST-01..LIST-16 in the List Commands section flipped to `[x]`
    - Full test suite (`uv run pytest tests/`) passes — zero regressions in stream/scripting/pipeline/pubsub suites
  </behavior>
  <action>
**Part A — Append LIST-16 integration tests to `tests/test_lists.py`:**

Add the following test functions at the end of the existing file:

```python
# ---- LIST-16: Lua integration ----

async def test_lua_lpush_rpush_lrange(r):
    """LIST-16: Lua can dispatch LPUSH/RPUSH/LRANGE correctly."""
    result = await r.eval(
        "redis.call('LPUSH', KEYS[1], 'a', 'b', 'c'); "
        "return redis.call('LRANGE', KEYS[1], 0, -1)",
        1,
        "k",
    )
    assert result == [b"c", b"b", b"a"]


async def test_lua_lpop_count(r):
    """LIST-16: Lua LPOP with count returns array."""
    await r.rpush("k", "a", "b", "c")
    result = await r.eval(
        "return redis.call('LPOP', KEYS[1], 2)",
        1,
        "k",
    )
    assert result == [b"a", b"b"]


async def test_lua_llen(r):
    await r.rpush("k", "a", "b", "c")
    result = await r.eval("return redis.call('LLEN', KEYS[1])", 1, "k")
    assert result == 3


async def test_lua_lmove(r):
    await r.rpush("src", "a", "b", "c")
    result = await r.eval(
        "return redis.call('LMOVE', KEYS[1], KEYS[2], 'LEFT', 'RIGHT')",
        2, "src", "dst",
    )
    assert result == b"a"
    assert await r.lrange("dst", 0, -1) == [b"a"]


async def test_lua_blpop_rejected(r):
    """LIST-16: Lua BLPOP must raise 'not allowed from scripts'."""
    with pytest.raises(Exception, match="not allowed from scripts"):
        await r.eval("return redis.call('BLPOP', KEYS[1], 0)", 1, "k")


async def test_lua_brpop_rejected(r):
    with pytest.raises(Exception, match="not allowed from scripts"):
        await r.eval("return redis.call('BRPOP', KEYS[1], 0)", 1, "k")


async def test_lua_blmove_rejected(r):
    with pytest.raises(Exception, match="not allowed from scripts"):
        await r.eval(
            "return redis.call('BLMOVE', KEYS[1], KEYS[2], 'LEFT', 'RIGHT', 0)",
            2, "src", "dst",
        )


async def test_brpop_wakes_on_lua_lpush(r):
    """LIST-16 regression: BRPOP must wake when LPUSH is issued from inside a Lua script.
    This is the Phase-11-style race fix guarded by the had_list_mutation flag.
    """
    async def lua_push_later():
        await asyncio.sleep(0.05)
        await r.eval("redis.call('LPUSH', KEYS[1], 'v'); return 1", 1, "k")

    task = asyncio.create_task(lua_push_later())
    start = time.monotonic()
    result = await r.brpop(["k"], timeout=2.0)
    elapsed = time.monotonic() - start
    await task
    assert elapsed < 1.0, f"BRPOP did not wake promptly on Lua LPUSH: {elapsed}s"
    assert result == (b"k", b"v")


async def test_blpop_wakes_on_lua_rpush(r):
    """Mirror for RPUSH — also marked had_list_mutation."""
    async def lua_push_later():
        await asyncio.sleep(0.05)
        await r.eval("redis.call('RPUSH', KEYS[1], 'v'); return 1", 1, "k")

    task = asyncio.create_task(lua_push_later())
    result = await asyncio.wait_for(r.blpop(["k"], timeout=2.0), timeout=3.0)
    await task
    assert result == (b"k", b"v")


# ---- LIST-16: Pipeline integration ----

async def test_pipeline_list_commands_non_blocking(r):
    """All non-blocking list commands in a pipeline — verify results + fast-path timing."""
    pipe = r.pipeline()
    pipe.lpush("k", "a", "b", "c")
    pipe.llen("k")
    pipe.lrange("k", 0, -1)
    pipe.lindex("k", 0)
    pipe.lpop("k")
    start = time.monotonic()
    results = await pipe.execute()
    elapsed = time.monotonic() - start
    assert elapsed < 0.1, f"fast path too slow: {elapsed}s"
    assert results[0] == 3  # lpush count
    assert results[1] == 3  # llen
    assert results[2] == [b"c", b"b", b"a"]  # lrange
    assert results[3] == b"c"  # lindex 0
    assert results[4] == b"c"  # lpop


async def test_pipeline_with_blocking_command(r):
    """Pipeline mixing blocking + non-blocking commands respects per-command timeout."""
    pipe = r.pipeline()
    pipe.set("x", "1")
    pipe.blpop(["missing"], timeout=0.1)
    pipe.set("y", "2")
    start = time.monotonic()
    results = await pipe.execute()
    elapsed = time.monotonic() - start
    assert elapsed >= 0.05, f"blocking pipeline did not block: {elapsed}s"
    assert results[0] is True  # set
    assert results[1] is None  # blpop timeout
    assert results[2] is True  # set after blpop


async def test_pipeline_blocking_wakes_on_push(r):
    """Pipeline BLPOP wakes when another task pushes."""
    await r.rpush("k", "pre-existing")  # guarantees first poll succeeds

    pipe = r.pipeline()
    pipe.set("x", "1")
    pipe.blpop(["k"], timeout=2.0)
    pipe.set("y", "2")

    start = time.monotonic()
    results = await pipe.execute()
    elapsed = time.monotonic() - start

    assert elapsed < 1.0, f"BLPOP in pipeline blocked unnecessarily: {elapsed}s"
    assert results[0] is True
    assert results[1] == (b"k", b"pre-existing")
    assert results[2] is True


async def test_pipeline_non_blocking_fast_path_timing(r):
    """Regression guard for quick task 260415-an2: non-blocking pipelines must stay sync-fast."""
    pipe = r.pipeline()
    for _ in range(50):
        pipe.lpush("k", "v")
    pipe.llen("k")
    start = time.monotonic()
    results = await pipe.execute()
    elapsed = time.monotonic() - start
    assert elapsed < 0.1, f"50-cmd non-blocking pipeline too slow: {elapsed}s (fast path may have regressed)"
    assert results[-1] == 50


async def test_pipeline_lrem_ltrim_lset(r):
    """Pipeline coverage for in-place list mutations."""
    pipe = r.pipeline()
    pipe.rpush("k", "a", "b", "a", "c")
    pipe.lrem("k", 0, "a")
    pipe.lset("k", 0, "B")
    pipe.ltrim("k", 0, 0)
    pipe.lrange("k", 0, -1)
    results = await pipe.execute()
    assert results[0] == 4  # rpush count
    assert results[1] == 2  # lrem removed 2 a's
    assert results[2] is True  # lset
    assert results[3] is True  # ltrim
    assert results[4] == [b"B"]
```

**Part B — Finalize `.planning/REQUIREMENTS.md`:**

1. In the `### List Commands` section (added by Plan 01), flip each `- [ ] **LIST-NN**:` to `- [x] **LIST-NN**:` for all 16.
2. In the Traceability table, update each of the 16 LIST rows from `In Progress` to `Complete`.
3. Update the Coverage block at the bottom:
```
**Coverage:**
- v1 requirements: 69 total
- Mapped to phases: 69
- Unmapped: 0
```
(If Plan 01 already bumped these numbers, ensure the count stays correct — 53 + 16 = 69.)

**Part C — Full-suite regression verification:**

Run `uv run pytest tests/ -x 2>&1 | tail -20` and verify: zero failures in `test_streams.py`, `test_scripting.py`, `test_pipeline.py`, `test_graceful_shutdown.py`, `test_pubsub.py`, `test_hashes.py`, `test_sets.py`, `test_sorted_sets.py`, `test_strings.py`, `test_coercion.py`, `test_expiration.py`, `test_locking.py`, `test_persistence.py`, `test_prefect_integration.py`, `test_pydocket_compat.py`, plus all new tests in `test_lists.py`.

If any prior test regresses, the most likely causes are:
- `LuaEngine::execute` tuple widening broke an unexpected caller (grep src/ for all callers of `LuaEngine::execute` and `dispatch_command`)
- Stream notify or pubsub call-sites got crossed with list notify — double-check that `stream_notify` sites in Plan 01's edits were not accidentally replaced
- Pipeline `execute` Python-side branch accidentally covers a non-list blocking command (none exist, but verify)
  </action>
  <verify>
    <automated>uv run pytest tests/test_lists.py -x 2>&amp;1 | tee /tmp/phase14-task3-lists.log | tail -20; uv run pytest tests/ -x 2>&amp;1 | tee /tmp/phase14-task3-full.log | tail -20; grep -c "LIST-.*Phase 14.*Complete" .planning/REQUIREMENTS.md; grep -c "\\[x\\] \\*\\*LIST-" .planning/REQUIREMENTS.md; echo "---done"</automated>
  </verify>
  <acceptance_criteria>
    - `uv run pytest tests/test_lists.py -x` exits 0 with the full test count (Plan 02's ~35 + this plan's ~13 = ~48 total)
    - `uv run pytest tests/ -x` exits 0 (no regression across the entire test suite)
    - `grep -c "LIST-.*Phase 14.*Complete" .planning/REQUIREMENTS.md` returns 16
    - `grep -c "\[x\] \*\*LIST-" .planning/REQUIREMENTS.md` returns 16
    - `grep -q "v1 requirements: 69" .planning/REQUIREMENTS.md` returns 0
    - `test_brpop_wakes_on_lua_lpush` is in the passing list (critical had_list_mutation regression guard)
    - `test_pipeline_non_blocking_fast_path_timing` is in the passing list (260415-an2 regression guard)
    - `test_lua_blpop_rejected`, `test_lua_brpop_rejected`, `test_lua_blmove_rejected` are all in the passing list
  </acceptance_criteria>
  <done>All LIST-16 integration tests pass. Full test suite green (no regressions). REQUIREMENTS.md finalized: LIST-01..LIST-16 marked Complete in Traceability and the List Commands section. Phase ready for /gsd-verify-work.</done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|----------|-------------|
| Lua script → Store keyspace (via dispatch_command_inner) | Scripts execute atomically under an already-acquired write lock; must not deadlock or silently fail to wake external waiters |
| Pipeline queue → command dispatch | Queue may contain heterogeneous commands including blocking; must detect and route correctly |
| asyncio task → Pipeline.execute | Python-side coroutine iterating commands must propagate CancelledError cleanly |

## STRIDE Threat Register

| Threat ID | Category | Component | Disposition | Mitigation Plan |
|-----------|----------|-----------|-------------|-----------------|
| T-14-03 | Denial of Service / Deadlock | `dispatch_command_inner` BLPOP/BRPOP/BLMOVE arms | mitigate | Per D-13: dispatch returns `RedisValue::Error` with canonical Redis wording. Tested by `test_lua_blpop_rejected`, `test_lua_brpop_rejected`, `test_lua_blmove_rejected`. |
| T-14-04 | Tampering / Race Condition | `dispatch_command` tuple extension + Store::eval notify | mitigate | `had_list_mutation` flag set in `dispatch_command` for LPUSH/RPUSH/LMOVE/RPOPLPUSH/LINSERT on success; Store::eval/evalsha fire `list_notify.notify_waiters()` after data lock drops. Regression-tested by `test_brpop_wakes_on_lua_lpush`. |
| T-14-06 | Performance (not strictly STRIDE) | `execute_pipeline` fast-path preservation | mitigate | D-16: the fast path is preserved by making blocking-detection a Python-side decision in `Pipeline.execute()`. Rust `execute_pipeline` remains synchronous for the non-blocking-only queue. Regression-tested by `test_pipeline_non_blocking_fast_path_timing`. |
| T-14-11 (input-validation) | Tampering | `dispatch_pipeline_command` argument extraction | mitigate | `extract_bytes`, `.extract::<i64>()`, `parse_list_end`, `parse_linsert_where` all surface invalid inputs as PyValueError / StoreError. Pipeline errors are captured per-command and returned in the results list (matches redis-py `raise_on_error=True` default behavior). |

No threats with severity=high remain unmitigated. ASVS L1 compliance maintained.
</threat_model>

<verification>
End-to-end verification for the full phase:

```bash
# Rust regression check
cargo test --lib 2>&1 | tail -10

# Full Python test suite (includes all of Phase 14 plus regressions)
uv run pytest tests/ -x 2>&1 | tail -10

# Requirements finalization
grep -c "LIST-.*Phase 14.*Complete" .planning/REQUIREMENTS.md  # must be 16
grep -c "\[x\] \*\*LIST-" .planning/REQUIREMENTS.md  # must be 16
```

Manual spot-check (optional, documented in VALIDATION.md Manual-Only section):
- Shutdown-during-blpop race: start `asyncio.create_task(r.blpop('k', timeout=0))`, call `await r.aclose()` within 100ms, assert task completes with `None`.
- Benchmark non-blocking pipeline: 1000 LPUSH + LRANGE operations via pipeline; confirm total elapsed stays within 5% of Phase 7 baseline.
</verification>

<success_criteria>
- All 16 list commands work from Python direct calls, Lua scripts (13 non-blocking + 3 rejected), and Pipelines (13 non-blocking + 3 blocking through Python-side branch)
- `had_list_mutation` flag propagates through the Lua tuple; BRPOP wakes on Lua LPUSH
- Pipeline fast path preserved for all-non-blocking queues (regression test passes)
- REQUIREMENTS.md LIST-01..LIST-16 marked Complete in both List Commands section and Traceability table
- Full test suite green: `uv run pytest tests/ -x` exits 0
- No regressions in existing Lua, pipeline, stream, pubsub test suites
</success_criteria>

<output>
After completion, create `.planning/phases/14-add-support-for-the-redis-list-data-type-required-commands-l/14-03-SUMMARY.md` with:
- Lua dispatch arm count (13 non-blocking + 3 rejected)
- `had_list_mutation` call sites in eval + evalsha
- Pipeline arm count (13 non-blocking in Rust + 16 Python stubs)
- Python-side blocking-aware branch summary
- Full-suite test count and regression check result
- REQUIREMENTS.md finalization status
- Any deviations from the plan (e.g., if the Rust-side execute_pipeline slow-path ended up being implemented differently)
- Phase completion readiness: ready for /gsd-verify-work or gap-closure needed
</output>
