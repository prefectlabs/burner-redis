# Phase 14: List data type - Pattern Map

**Mapped:** 2026-04-24
**Files analyzed:** 9 (1 new Rust module, 1 new Python test, 5 Rust files modified, 2 Python files modified, 1 planning file modified)
**Analogs found:** 9 / 9

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `src/commands/lists.rs` (NEW) | Rust helper module (arg parsing, index normalization) | transform | `src/commands/streams.rs` | exact (same role: per-command-group helper module in the `commands/` tree) |
| `src/store.rs` (MOD) | Keyspace + notify fan-out | CRUD + event-driven | existing `xadd` / `stream_notify` sections at lines 1214-1267 and 271-312 | self-analog (in-file replay of XADD/stream_notify pattern) |
| `src/lib.rs` (MOD, non-blocking pymethods) | PyO3 binding, request-response | request-response | `fn sadd` (lines 595-611) and `fn hset` (lines 467-493) | exact for fan-in variadic (`sadd`) and for `key=..., value=...` kwargs (`hset`) |
| `src/lib.rs` (MOD, blocking pymethods `brpop`/`blpop`/`blmove`) | PyO3 binding with tokio async loop | streaming + event-driven | `fn xread` blocking branch at lines 979-1038 and `fn xreadgroup` at lines 1284-1341 | exact (`xread` is the direct template — both first-poll-then-select loop and deadline handling) |
| `src/lib.rs` (MOD, `dispatch_pipeline_command` non-blocking arms) | Sync pipeline dispatch | request-response | `"sadd"` arm at lines 2322-2328, `"hset"` arm at lines 2252-2269 | exact |
| `src/lib.rs` (MOD, `execute_pipeline` blocking-detection branch) | Pipeline router | batch | existing `execute_pipeline` at lines 2182-2197 (to be extended, not replaced) | self-analog |
| `src/scripting.rs` (MOD, `dispatch_command` tuple) | Lua return-tuple adapter | transform | lines 268-279 (add `had_list_mutation` flag parallel to `had_xadd`) | self-analog |
| `src/scripting.rs` (MOD, `dispatch_command_inner` arms) | Lua command dispatcher | CRUD | `"SADD"` at lines 684-713 (non-blocking template), `"XADD"` at lines 1261-1337 (uses-data-mut template) | exact |
| `src/commands/mod.rs` (MOD, register `lists` module) | module registry | config | all 6 lines of existing file | exact |
| `python/burner_redis/__init__.py` (MOD) | Python value coercion | transform | `_coerced_set` at lines 64-80 (monkey-patch wrapping original pymethod + `_coerce_value`) | exact |
| `python/burner_redis/pipeline.py` (MOD) | Pipeline command stubs | batch | `sadd` stub at line 91-93, stream stubs at lines 135-163 | exact |
| `tests/test_lists.py` (NEW) | pytest integration coverage | request-response + streaming | `tests/test_streams.py` (non-blocking: lines 13-120; blocking: lines 1064-1194) | exact |
| `.planning/REQUIREMENTS.md` (MOD) | requirements spec | config | lines 116-131 (Out of Scope), lines 133-189 (Traceability) | exact |

## Pattern Assignments

### `src/store.rs` — `ValueData::List(VecDeque<Bytes>)` variant and helpers

**Analog:** `src/store.rs` itself (lines 116-185, the `ValueData` enum and `ValueEntry` constructors).

**Enum variant pattern** (lines 116-129):
```rust
#[derive(Clone, Debug)]
pub enum ValueData {
    String(Bytes),
    Hash(HashMap<Bytes, Bytes>),
    Set(HashSet<Bytes>),
    SortedSet(SortedSet),
    Stream(Stream),
    // ADD: List(VecDeque<Bytes>),
}
```

**Constructor pattern** (lines 149-178) — add a sibling `new_list`:
```rust
pub fn new_hash() -> Self {
    ValueEntry { data: ValueData::Hash(HashMap::new()), expires_at: None }
}
// ... same style for new_set / new_sorted_set / new_stream ...
// ADD:
// pub fn new_list() -> Self {
//     ValueEntry { data: ValueData::List(VecDeque::new()), expires_at: None }
// }
```

**Don't forget:** top of file `use std::collections::{BTreeMap, HashMap, HashSet};` at line 5 — extend with `VecDeque`.

---

### `src/store.rs` — `list_notify: Arc<Notify>` field and shutdown wake

**Analog:** `stream_notify` field at lines 271-317.

**Field + constructor + accessor + shutdown wake** (lines 271-317, copy verbatim substituting `stream` → `list`):
```rust
// Line 271-277 — struct field:
pub struct Store {
    data: RwLock<HashMap<Bytes, ValueEntry>>,
    scripts: RwLock<HashMap<String, String>>,
    pub(crate) pubsub: RwLock<PubSubRegistry>,
    stream_notify: Arc<Notify>,
    // ADD: list_notify: Arc<Notify>,
    shutdown: AtomicBool,
}

// Line 280-288 — constructor:
impl Store {
    pub fn new() -> Self {
        Store {
            data: RwLock::new(HashMap::new()),
            scripts: RwLock::new(HashMap::new()),
            pubsub: RwLock::new(PubSubRegistry::new()),
            stream_notify: Arc::new(Notify::new()),
            // ADD: list_notify: Arc::new(Notify::new()),
            shutdown: AtomicBool::new(false),
        }
    }

    // Line 290-293 — accessor:
    pub fn stream_notify(&self) -> Arc<Notify> {
        self.stream_notify.clone()
    }
    // ADD a parallel `pub fn list_notify(&self) -> Arc<Notify> { self.list_notify.clone() }`

    // Line 298-312 — shutdown wake:
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Release);
        self.stream_notify.notify_waiters();
        // ADD: self.list_notify.notify_waiters();
        // ... existing pubsub stop code ...
    }
}
```

---

### `src/store.rs` — per-command Store methods (e.g. `lpush`, `rpush`, `lpop`, `rpop`, etc.)

**Analog for mutating-create-or-update:** `pub fn sadd` at lines 697-723 (variadic insertion; `or_insert_with(ValueEntry::new_set)`).

**Template** (lines 697-723):
```rust
pub fn sadd(&self, key: Bytes, members: Vec<Bytes>) -> Result<i64, StoreError> {
    let mut data = self.data.write();

    // Passive expiration
    if let Some(entry) = data.get(&key) {
        if entry.is_expired() {
            data.remove(&key);
        }
    }

    let entry = data.entry(key).or_insert_with(ValueEntry::new_set);

    match entry.data {
        ValueData::Set(ref mut set) => {
            let mut new_count = 0i64;
            for member in members {
                if set.insert(member) {
                    new_count += 1;
                }
            }
            Ok(new_count)
        }
        _ => Err(StoreError::WrongType),
    }
}
```

For **`lpush`**, swap `ValueEntry::new_set` → `ValueEntry::new_list`, `ValueData::Set` → `ValueData::List`, and call `list.push_front(v)` in the loop. **CRITICAL:** call `self.list_notify.notify_waiters();` inside the write-lock scope (inside the `ValueData::List` arm, after inserts, before returning `Ok(len)`). This mirrors the XADD notify-inside-write-lock idiom.

**Analog for notify-inside-write-lock (the load-bearing pattern):** `xadd` at lines 1217-1267.

**Copy this exact ordering** (lines 1259-1263):
```rust
stream.entries.insert(new_id, fields);
stream.last_id = new_id;
// Wake any blocking XREADGROUP waiters
self.stream_notify.notify_waiters();  // <-- inside write lock, after mutation
Ok(new_id)
```

Apply this pattern (inside write lock, after the mutation, before returning) to: **LPUSH, RPUSH, LMOVE (on destination-push side), RPOPLPUSH (on destination-push side), BLMOVE destination-write**. Do NOT fire notify on pop-only commands (LPOP, RPOP, LREM, LTRIM, LSET, LINSERT) — they don't add elements.

**Analog for read-only lookups:** `pub fn smembers` at lines 728-746 (passive expiration + `None` → empty + `WrongType` arm). Use for LRANGE, LLEN, LINDEX.

**Analog for delete-empty-after-mutation:** `pub fn srem` at lines 774+ — follows the "mutate then check emptiness then remove key" pattern. LPOP/RPOP/LREM/LTRIM must apply D-03 (empty list after final pop deletes the key).

---

### `src/lib.rs` — Non-blocking `#[pymethods]` (LPUSH, RPUSH, LPOP, RPOP, LRANGE, LLEN, LINDEX, LINSERT, LREM, LSET, LTRIM, LMOVE, RPOPLPUSH)

**Analog for variadic fan-in (LPUSH/RPUSH):** `fn sadd` at lines 595-611.

**Template** (lines 595-611):
```rust
#[pyo3(signature = (name, *values))]
fn sadd<'py>(
    &self,
    py: Python<'py>,
    name: &Bound<'py, PyAny>,
    values: &Bound<'py, pyo3::types::PyTuple>,
) -> PyResult<Bound<'py, PyAny>> {
    let name_bytes = extract_bytes(name)?;
    let members: Vec<Bytes> = values
        .iter()
        .map(|obj| extract_bytes(&obj))
        .collect::<PyResult<Vec<_>>>()?;
    let count = self.store.sadd(name_bytes, members).map_err(store_err_to_py)?;
    resolved(py, count.into_pyobject(py)?.into_any().unbind())
}
```

For `lpush`/`rpush`, keep signature identical (`name, *values`), route to `store.lpush` / `store.rpush`.

**Analog for single-return or None (LPOP without count, LINDEX):** `fn hget` at lines 497-511.

**Template** (lines 497-511):
```rust
fn hget<'py>(
    &self,
    py: Python<'py>,
    name: &Bound<'py, PyAny>,
    key: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyAny>> {
    let name_bytes = extract_bytes(name)?;
    let field_bytes = extract_bytes(key)?;
    let result = self.store.hget(&name_bytes, &field_bytes).map_err(store_err_to_py)?;
    let py_result = match result {
        Some(b) => PyBytes::new(py, &b).into_any().unbind(),
        None => py.None(),
    };
    resolved(py, py_result)
}
```

**Analog for list-of-bytes return (LRANGE):** `fn hvals` at lines 533-542.

---

### `src/lib.rs` — Blocking `#[pymethods]` (BRPOP, BLPOP, BLMOVE)

**Analog:** `fn xread` blocking branch at lines 979-1038 — this is the load-bearing template.

**Template** (lines 979-1038, verbatim skeleton):
```rust
let store = self.store.clone();
let block_ms = block.unwrap();

pyo3_async_runtimes::tokio::future_into_py(py, async move {
    let notify = store.stream_notify();           // <-- for BRPOP/BLPOP: store.list_notify()
    let mut waiter = Box::pin(notify.notified());
    waiter.as_mut().enable();                     // arm permit BEFORE first poll

    // First non-blocking attempt
    let results = store.xread(&keys, &ids, count).map_err(store_err_to_py)?;
    if !results.is_empty() {
        return format_xread_result(results);
    }

    let deadline_opt = if block_ms == 0 {
        None                                      // block=0 → forever
    } else {
        Some(tokio::time::Instant::now() + Duration::from_millis(block_ms))
    };

    loop {
        if store.is_shutdown() {                  // graceful teardown
            break format_xread_result(Vec::new());
        }

        let remaining = match deadline_opt {
            Some(d) => {
                let r = d.saturating_duration_since(tokio::time::Instant::now());
                if r.is_zero() { break format_xread_result(Vec::new()); }
                r
            }
            None => Duration::from_secs(3600),    // block=0 long slice
        };

        tokio::select! {
            _ = waiter.as_mut() => {
                waiter.set(notify.notified());    // <-- RE-ARM (Phase 11 critical fix)
                waiter.as_mut().enable();
                let results = store.xread(&keys, &ids, count).map_err(store_err_to_py)?;
                if !results.is_empty() {
                    break format_xread_result(results);
                }
                // else: loop — notify was for unrelated key
            }
            _ = tokio::time::sleep(remaining) => {
                if deadline_opt.is_some() {
                    break format_xread_result(Vec::new());
                }
                // block=0: sleep completed, keep looping
            }
        }
    }
})
```

**For BRPOP/BLPOP:**
- Replace `store.xread(&keys, &ids, count)` with `store.brpop_poll(&keys)` / `store.blpop_poll(&keys)` (returns `Option<(Bytes, Bytes)>`).
- Replace `format_xread_result(results)` with a `(key, value)` tuple builder or `py.None()`.
- redis-py callback shape: `lambda r: r and tuple(r) or None`, so on success return a 2-tuple of `bytes`, on timeout/shutdown return `py.None()`.

**For BLMOVE:**
- Replace the `xread` call with `store.lmove_atomic(&src, &dst, src_from, dst_to)` (returns `Option<Bytes>`).
- Return `PyBytes` on success, `py.None()` on timeout/shutdown.
- Note that the `src_from` / `dst_to` enum (`ListEnd::Left` / `ListEnd::Right`) lives in `src/commands/lists.rs` per D-05.

**The re-arm idiom (`waiter.set(notify.notified()); waiter.as_mut().enable();`) at lines 1021-1023 and 1324-1325 is non-negotiable.** Skipping it causes lost-wakeup bugs. This is documented both in the Phase 11 CONTEXT and in the code comments at lines 973-976, 1279-1283, and 1322-1325.

**Secondary analog:** `fn xreadgroup` blocking loop at lines 1284-1341 — use this for BLMOVE since it has the same "simple deadline, no block=0 long-slice" shape (BLMOVE in redis-py still supports timeout=0 for forever; choose the xread shape if you need block=0 support, otherwise the simpler xreadgroup shape).

---

### `src/lib.rs` — `dispatch_pipeline_command` sync arms (13 non-blocking commands)

**Analog for variadic:** `"sadd"` arm at lines 2322-2328.

**Template** (lines 2322-2328):
```rust
"sadd" => {
    let name = &args.get_item(0)?;
    let name_bytes = extract_bytes(name)?;
    let members: Vec<Bytes> = args.iter().skip(1).map(|obj| extract_bytes(&obj)).collect::<PyResult<Vec<_>>>()?;
    let count = self.store.sadd(name_bytes, members).map_err(store_err_to_py)?;
    Ok(count.into_pyobject(py)?.into_any().unbind())
}
```

**Analog for kwargs-heavy:** `"hset"` arm at lines 2252-2269, `"zadd"` arm at lines 2351-2368. Use `kwargs.get_item("foo")?.and_then(|v| if v.is_none() { None } else { ... })` for optional args. Example extraction idiom (line 2214):
```rust
let ex: Option<Bound<'py, PyAny>> = kwargs.get_item("ex")?.and_then(|v| if v.is_none() { None } else { Some(v) });
```

Apply to: `lpush`, `rpush`, `lpop` (count kwarg), `rpop` (count kwarg), `lrange`, `llen`, `lindex`, `linsert` (where/pivot/value positional), `lrem` (count/value positional), `lset` (index/value positional), `ltrim` (start/end positional), `lmove` (src/dst/src_end/dst_end positional), `rpoplpush` (src/dst positional).

---

### `src/lib.rs` — `execute_pipeline` blocking-aware branch

**Analog:** existing `execute_pipeline` at lines 2182-2197 (extend, don't replace).

**Current structure** (lines 2182-2197):
```rust
fn execute_pipeline<'py>(&self, py: Python<'py>, commands: &Bound<'py, PyList>) -> PyResult<Bound<'py, PyAny>> {
    let results = pyo3::types::PyList::empty(py);
    for item in commands.iter() {
        let tuple = item.downcast::<PyTuple>()?;
        let method_name: String = tuple.get_item(0)?.extract()?;
        let args = tuple.get_item(1)?.downcast::<PyTuple>()?.clone();
        let kwargs = tuple.get_item(2)?.downcast::<PyDict>()?.clone();

        let result = self.dispatch_pipeline_command(py, &method_name, &args, &kwargs);
        match result {
            Ok(val) => results.append(val)?,
            Err(e) => results.append(e.value(py))?,
        }
    }
    resolved(py, results.into_any().unbind())
}
```

**Extension strategy (per D-16):**
1. **Before the loop**, scan `commands` for any method name in `{"brpop", "blpop", "blmove"}`.
2. **If none present**, keep the existing sync fast path unchanged (preserves quick task `260415-an2`).
3. **If any present**, return `future_into_py(py, async move { ... })` that awaits each command sequentially. For non-blocking commands, call `dispatch_pipeline_command` under `Python::try_attach`; for blocking commands, invoke the normal `BurnerRedis.brpop/blpop/blmove` async pymethod via `Python::try_attach + getattr + call`.

**The test that locks this:** `test_xread_block_none_is_non_blocking` at `tests/test_streams.py:1100-1109` — any regression of the sync fast path shows up here as a latency jump.

---

### `src/scripting.rs` — `dispatch_command` tuple extension (`had_list_mutation` flag)

**Analog:** lines 268-279 (`had_xadd` pattern).

**Current** (lines 268-279):
```rust
fn dispatch_command(
    cmd: &str,
    args: &[Bytes],
    data: &mut HashMap<Bytes, ValueEntry>,
    pubsub_tx: Option<&broadcast::Sender<PubSubMessage>>,
) -> Result<(RedisValue, bool), String> {
    let is_xadd = cmd == "XADD";
    let result = dispatch_command_inner(cmd, args, data, pubsub_tx)?;
    let had_xadd = is_xadd && !matches!(result, RedisValue::Error(_));
    Ok((result, had_xadd))
}
```

**Extension:**
```rust
fn dispatch_command(...) -> Result<(RedisValue, bool, bool), String> {
    let is_xadd = cmd == "XADD";
    let is_list_mutation = matches!(cmd, "LPUSH" | "RPUSH" | "LMOVE" | "RPOPLPUSH");
    let result = dispatch_command_inner(cmd, args, data, pubsub_tx)?;
    let had_xadd = is_xadd && !matches!(result, RedisValue::Error(_));
    let had_list_mutation = is_list_mutation && !matches!(result, RedisValue::Error(_));
    Ok((result, had_xadd, had_list_mutation))
}
```

**Call sites that must update:** lines 184, 234 in `scripting.rs` (`create_function_mut` closures for `redis.call` / `redis.pcall`) destructure `Ok((val, xadd_flag)) =>` — must become `Ok((val, xadd_flag, list_mut_flag)) =>`. Use a `Cell<bool>` for `had_list_mutation` alongside `had_xadd` at line 126.

**Return tuple from `execute`:** line 117 returns `Result<(RedisValue, bool), String>`. Extend to `Result<(RedisValue, bool, bool), String>`. Final `scope_result.map(|v| (v, had_xadd.get()))` at line 260 becomes `.map(|v| (v, had_xadd.get(), had_list_mutation.get()))`.

**`Store::eval` / `Store::evalsha` update** (lines 2387-2405, 2410-2433 in store.rs):
```rust
let (result, had_xadd) = {
    let mut data = self.data.write();
    LuaEngine::execute(script, keys, args, &mut *data, Some(&pubsub_tx))?
};
if had_xadd {
    self.stream_notify.notify_waiters();
}
```

Becomes:
```rust
let (result, had_xadd, had_list_mutation) = {
    let mut data = self.data.write();
    LuaEngine::execute(script, keys, args, &mut *data, Some(&pubsub_tx))?
};
if had_xadd {
    self.stream_notify.notify_waiters();
}
if had_list_mutation {
    self.list_notify.notify_waiters();
}
```

---

### `src/scripting.rs` — `dispatch_command_inner` arms (13 non-blocking + 3 blocking-reject)

**Non-blocking analog:** `"SADD"` at lines 684-713.

**Template** (lines 684-713):
```rust
"SADD" => {
    if args.len() < 2 {
        return Ok(RedisValue::Error(
            "ERR wrong number of arguments for 'sadd' command".to_string(),
        ));
    }
    let key = args[0].clone();

    // Passive expiration
    if let Some(entry) = data.get(&key) {
        if entry.is_expired() {
            data.remove(&key);
        }
    }

    let entry = data.entry(key).or_insert_with(ValueEntry::new_set);
    match entry.data {
        ValueData::Set(ref mut set) => {
            let mut new_count = 0i64;
            for member in &args[1..] {
                if set.insert(member.clone()) {
                    new_count += 1;
                }
            }
            Ok(RedisValue::Integer(new_count))
        }
        _ => Ok(RedisValue::Error(
            "WRONGTYPE Operation against a key holding the wrong kind of value".to_string(),
        )),
    }
}
```

Apply verbatim to `"LPUSH"` / `"RPUSH"` (swap `ValueEntry::new_set` → `ValueEntry::new_list`, `ValueData::Set` → `ValueData::List`, `set.insert(member.clone())` → `list.push_front(member.clone())` / `list.push_back(...)`).

**Mutation-with-atomic-dual-side analog:** `"XADD"` at lines 1261-1337 (operates on `data` mutably, returns `RedisValue::BulkString` constructed from the write's result). Use as the template for LMOVE / RPOPLPUSH inside Lua.

**Blocking-reject pattern:** the catch-all at line 1808 is `_ => Ok(RedisValue::Error(format!("ERR unknown command '{}'", cmd)))`. For BRPOP/BLPOP/BLMOVE, add explicit arms **before** the catch-all, returning the Redis canonical error wording (per D-13):
```rust
"BRPOP" | "BLPOP" | "BLMOVE" => Ok(RedisValue::Error(format!(
    "ERR This Redis command is not allowed from scripts: {}", cmd
))),
```

**Error wording verification:** per research note in 14-RESEARCH.md (LIST-16, flagged ASSUMED), verify against real Redis server source before freezing. The redis-py test suite will catch a mismatch.

---

### `src/commands/lists.rs` (NEW)

**Analog:** `src/commands/streams.rs` (the whole file, 37 lines).

**Pattern** (streams.rs in full):
```rust
use bytes::Bytes;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::collections::HashMap;

use super::strings::extract_bytes;

/// A stream entry ID: (milliseconds_since_epoch, sequence_number).
pub type StreamId = (u64, u64);

pub fn format_stream_id(id: StreamId) -> String { ... }
pub fn parse_stream_id(s: &str) -> Option<StreamId> { ... }
pub fn extract_stream_fields(dict: &Bound<'_, PyDict>) -> PyResult<HashMap<Bytes, Bytes>> { ... }
```

**What goes in `lists.rs`** (per D-05 and Claude's Discretion):
- `pub enum ListEnd { Left, Right }` — used as argument to `Store::lmove_atomic` for both source and destination end parameters
- `pub fn parse_list_end(s: &str) -> Result<ListEnd, StoreError>` — parse "LEFT" / "RIGHT" (case-insensitive) from an LMOVE/BLMOVE argument
- `pub fn normalize_range_indices(start: i64, stop: i64, len: usize) -> Option<(usize, usize)>` — LRANGE / LTRIM negative-index normalization, returning `None` if the clamped range is empty
- `pub fn parse_lrem_count(count: i64) -> LremDirection` — positive=head-to-tail, negative=tail-to-head, 0=all occurrences
- `pub fn parse_linsert_where(s: &str) -> Result<InsertPosition, StoreError>` — "BEFORE" / "AFTER" parsing

**Module header convention** (from `src/commands/sets.rs`):
```rust
//! List command helpers for the Python binding layer.
//!
//! This module provides helper functions for Redis list commands.
//! The actual Python method implementations live in lib.rs following
//! the established pattern (via #[pymethods] on BurnerRedis).
//!
//! List commands implemented:
//! - LPUSH, RPUSH, LPOP, RPOP, LRANGE, LLEN, LINDEX, LINSERT,
//! - LREM, LSET, LTRIM, LMOVE, RPOPLPUSH, BRPOP, BLPOP, BLMOVE
//!
//! The core list logic lives in Store (src/store.rs).
```

---

### `src/commands/mod.rs`

**Analog:** the entire 6-line file (lines 1-6).

**Current** (all 6 lines):
```rust
pub mod strings;
pub mod hashes;
pub mod sets;
pub mod sorted_sets;
pub mod streams;
pub mod pubsub;
```

**Add** one line alongside the existing data-type modules (alphabetical or adjacent to `streams` per project convention):
```rust
pub mod lists;
```

---

### `python/burner_redis/__init__.py` — value-coercion monkey-patches

**Analog:** `_coerced_set` at lines 64-80.

**Template** (lines 64-80):
```python
_original_set = BurnerRedis.set


async def _coerced_set(self, name, value, ex=None, px=None, nx=False, xx=False):
    """SET with value coercion matching redis-py behavior."""
    return await _original_set(self, name, _coerce_value(value), ex=ex, px=px, nx=nx, xx=xx)


BurnerRedis.set = _coerced_set


async def _setex(self, name, time, value):
    """SETEX: Set key with expiration in seconds. Shorthand for SET with EX."""
    return await self.set(name, _coerce_value(value), ex=time)


BurnerRedis.setex = _setex
```

**Apply to (per D-19):**
- `lpush(name, *values)` — coerce each item in `*values` before calling the original pymethod
- `rpush(name, *values)` — same
- `lset(name, index, value)` — coerce `value`
- `linsert(name, where, refvalue, value)` — coerce `value` (refvalue is a lookup pivot, typically left uncoerced to match redis-py)
- `lmove(src, dst, src_end, dst_end)` — no coercion needed (no new value passed in; source element is already bytes)
- `rpoplpush(src, dst)` — same as LMOVE
- `blmove(src, dst, src_end, dst_end, timeout)` — same as LMOVE

**Note:** for variadic value args (`*values`), coercion is applied per-element:
```python
_original_lpush = BurnerRedis.lpush

async def _coerced_lpush(self, name, *values):
    coerced = [_coerce_value(v) for v in values]
    return await _original_lpush(self, name, *coerced)

BurnerRedis.lpush = _coerced_lpush
```

---

### `python/burner_redis/pipeline.py` — 16 new stub methods

**Analog for non-blocking variadic:** `sadd` stub at lines 91-93.

**Template** (lines 91-93):
```python
def sadd(self, name, *values):
    self._commands.append(("sadd", (name, *values), {}))
    return self
```

Use for `lpush`, `rpush`.

**Analog for count kwarg (LPOP/RPOP):** closest is the xtrim stub at line 147-149 (kwarg dict pattern):
```python
def xtrim(self, name, maxlen=None, minid=None, approximate=True):
    self._commands.append(("xtrim", (name,), {"maxlen": maxlen, "minid": minid, "approximate": approximate}))
    return self
```

**Analog for positional-only (LRANGE, LSET, LTRIM, LINSERT, LINDEX, LLEN, LREM, LMOVE, RPOPLPUSH):** `zrange` at lines 117-119:
```python
def zrange(self, name, start, end, withscores=False):
    self._commands.append(("zrange", (name, start, end), {"withscores": withscores}))
    return self
```

**Analog for blocking with timeout kwarg (BRPOP, BLPOP, BLMOVE):** the xread stub at lines 139-141 (uses `block=None` kwarg pattern):
```python
def xread(self, streams, count=None, block=None):
    self._commands.append(("xread", (streams,), {"count": count, "block": block}))
    return self
```

For BRPOP/BLPOP, keys are variadic + timeout is a keyword:
```python
def brpop(self, keys, timeout=0):
    # keys may be a list/tuple or variadic; normalize to tuple here
    if isinstance(keys, (list, tuple)):
        return self._commands.append(...)  # keys packed into args
    # See redis-py brpop signature for exact handling
```

Verify final signature against redis-py `core.py` — redis-py `brpop(keys, timeout=0)` takes keys as a list/tuple, timeout as float seconds.

**Place the 16 new stubs in a `# ---- List Commands ----` section** mirroring the existing grouping convention at lines 52 (`# ---- String Commands ----`), 71 (`# ---- Hash Commands ----`), 89 (`# ---- Set Commands ----`), etc.

---

### `tests/test_lists.py` (NEW)

**Analog:** `tests/test_streams.py` (full file, 1800+ lines — mirror the structure, not every test).

**File header convention** (test_streams.py lines 1-11):
```python
"""Tests for list commands: LPUSH, RPUSH, LPOP, RPOP, LRANGE, LLEN, LINDEX,
LINSERT, LREM, LSET, LTRIM, LMOVE, RPOPLPUSH, BRPOP, BLPOP, BLMOVE.

Covers requirements: LIST-01 through LIST-16.
"""
import asyncio
import time

import pytest
import redis.exceptions
from burner_redis import BurnerRedis
```

**`r` fixture** is already provided by `tests/conftest.py` (8 lines, creates a fresh `BurnerRedis()` per test). No change needed.

**Non-blocking test pattern analog:** `test_xadd_returns_id` at lines 16-25 — use for LPUSH/RPUSH return-value, LLEN, LRANGE etc.

**WRONGTYPE test pattern analog:** `test_xadd_wrongtype` at lines 74-78:
```python
async def test_xadd_wrongtype(r):
    """STRM-01: XADD on a string key raises WRONGTYPE."""
    await r.set("strkey", "value")
    with pytest.raises(Exception, match="WRONGTYPE"):
        await r.xadd("strkey", {"f": "v"})
```

Apply to every list command against a non-list key.

**Blocking-loop test patterns (most load-bearing):**

1. **Deadline-returns-None on timeout** — `test_xread_block_timeout_returns_empty` at lines 1088-1097:
```python
async def test_xread_block_timeout_returns_empty(r):
    last_id = await r.xadd("mystream", {"f": "v1"})
    start = time.monotonic()
    result = await r.xread({"mystream": last_id.decode()}, count=10, block=50)
    elapsed = time.monotonic() - start
    assert result is None
    assert elapsed >= 0.03
```

Apply to BRPOP/BLPOP with `timeout=0.05` expecting `None`.

2. **Wakeup-on-push from another task** — `test_xread_block_returns_new_entries` at lines 1067-1085:
```python
async def add_later():
    await asyncio.sleep(0.05)
    await r.xadd("mystream", {"f": "v2"})

task = asyncio.create_task(add_later())
result = await r.xread({"mystream": last_id.decode()}, count=10, block=2000)
```

Apply to BRPOP: schedule an `r.lpush("k", "v")` after 50ms while main task awaits BRPOP.

3. **Event-loop-cooperation test** — `test_xread_block_yields_to_event_loop` at lines 1112-1145 — **CRITICAL**. Guarantees the blocking future yields so other tasks run. Mirror exactly for BRPOP.

4. **`block=0` indefinite blocking** — `test_xread_block_zero_blocks_until_data` at lines 1148-1166 — wrap with `asyncio.wait_for(..., timeout=2.0)` to prevent the test from hanging if the `block=0` path is broken:
```python
result = await asyncio.wait_for(
    r.brpop(["k"], timeout=0),
    timeout=2.0,
)
```

5. **Multi-key left-to-right scan order** — `test_xread_block_multiple_streams` at lines 1169-1193:
```python
async def test_brpop_multi_key_scan_order(r):
    # Pre-populate both k2 and k4 with data
    await r.lpush("k2", "v2")
    await r.lpush("k4", "v4")
    # BRPOP should return from k2 (first non-empty, left-to-right)
    result = await r.brpop(["k1", "k2", "k3", "k4"], timeout=0.1)
    assert result == (b"k2", b"v2")
```

6. **Lua-to-BRPOP wake-up** — the class-of-bug that D-14's `had_list_mutation` flag prevents:
```python
async def test_brpop_wakes_on_lua_lpush(r):
    """Regression: BRPOP must wake when LPUSH is issued from inside a Lua script."""
    async def lua_push_later():
        await asyncio.sleep(0.05)
        await r.eval("redis.call('LPUSH', KEYS[1], 'v'); return 1", 1, "k")

    task = asyncio.create_task(lua_push_later())
    start = time.monotonic()
    result = await r.brpop(["k"], timeout=2.0)
    elapsed = time.monotonic() - start
    await task
    assert elapsed < 1.0
    assert result == (b"k", b"v")
```

7. **Pipeline mixing blocking + non-blocking** — new territory, no direct analog. Construct based on D-16:
```python
async def test_pipeline_with_blocking_commands(r):
    await r.lpush("k", "v")
    pipe = r.pipeline()
    pipe.set("x", "1")
    pipe.brpop(["k"], timeout=1.0)
    pipe.set("y", "2")
    results = await pipe.execute()
    assert results[0] is True
    assert results[1] == (b"k", b"v")
    assert results[2] is True
```

8. **Pipeline all-non-blocking (fast-path preservation)** — `test_xread_block_none_is_non_blocking` at lines 1100-1109 (measure elapsed < 50ms):
```python
async def test_list_pipeline_non_blocking_fast_path(r):
    pipe = r.pipeline()
    pipe.lpush("k", "a", "b", "c")
    pipe.llen("k")
    pipe.lrange("k", 0, -1)
    start = time.monotonic()
    results = await pipe.execute()
    elapsed = time.monotonic() - start
    assert elapsed < 0.05  # sync fast path preserved
```

---

### `.planning/REQUIREMENTS.md`

**Analog (Out of Scope table):** lines 120-131 of REQUIREMENTS.md.

**Current row to remove** (line 130):
```markdown
| Blocking list commands (BLPOP/BRPOP) | Prefect uses Streams, not blocking lists |
```

**Analog (requirements section):** Stream Commands block at lines 48-60 — add a parallel `### List Commands` section following the same `[ ] **LIST-NN**: User can ...` style (unchecked — this phase will tick them).

Add immediately after `### Stream Commands` (after line 60):
```markdown
### List Commands

- [ ] **LIST-01**: User can LPUSH one or more values onto the head of a list
- [ ] **LIST-02**: User can RPUSH one or more values onto the tail of a list
- [ ] **LIST-03**: User can LPOP with optional count (returns bytes, list of bytes, or None)
- [ ] **LIST-04**: User can RPOP with the same semantics as LPOP
- [ ] **LIST-05**: User can LRANGE with negative indices to slice a list
- [ ] **LIST-06**: User can LLEN to get the length of a list
- [ ] **LIST-07**: User can LINDEX to read an element at an index
- [ ] **LIST-08**: User can LINSERT BEFORE or AFTER a pivot
- [ ] **LIST-09**: User can LREM with positive, negative, or zero count
- [ ] **LIST-10**: User can LSET to replace an element at an index
- [ ] **LIST-11**: User can LTRIM to clamp a list to a range
- [ ] **LIST-12**: User can LMOVE between two lists atomically
- [ ] **LIST-13**: User can RPOPLPUSH (legacy alias for LMOVE RIGHT LEFT)
- [ ] **LIST-14**: User can BRPOP/BLPOP with float-seconds timeout, multi-key scan
- [ ] **LIST-15**: User can BLMOVE with timeout, atomic src/dst semantics
- [ ] **LIST-16**: All list commands work in pipelines; 13 non-blocking work in Lua
```

**Analog (Traceability table):** lines 137-189, which currently end at the Phase 13 entries. Append rows mapping LIST-01..LIST-16 to Phase 14 using the existing `| LIST-NN | Phase 14 | Complete |` row shape.

## Shared Patterns

### Passive Expiration at the top of every Store method

**Source:** lines 1225-1230 of `src/store.rs` (xadd), lines 700-705 (sadd), lines 754-759 (sismember).

**Apply to:** all new Store methods (`lpush`, `rpush`, `lpop`, `rpop`, `lrange`, `llen`, `lindex`, `linsert`, `lrem`, `lset`, `ltrim`, `lmove_atomic`, `rpoplpush_atomic`, `brpop_poll`, `blpop_poll`).

```rust
// Passive expiration
if let Some(entry) = data.get(&key) {
    if entry.is_expired() {
        data.remove(&key);
    }
}
```

### WRONGTYPE → ResponseError conversion

**Source:** `store_err_to_py` at `src/lib.rs:99-101`; `StoreError::WrongType` at `src/store.rs:190-191`; `make_response_error` at `src/lib.rs:82-96`.

**Apply to:** every `#[pymethods]` in `src/lib.rs` that calls a `Store` method which can return `StoreError::WrongType`. The existing `.map_err(store_err_to_py)?` idiom is sufficient.

```rust
let count = self.store.lpush(key, vals).map_err(store_err_to_py)?;
```

### `resolved(py, ...)` wrapping for sync pymethods

**Source:** `fn resolved` at `src/lib.rs:56-58`.

**Apply to:** all 13 non-blocking list `#[pymethods]`. Eliminates Tokio scheduling overhead — same mechanism as `sadd` / `hget` / `xlen`.

```rust
resolved(py, count.into_pyobject(py)?.into_any().unbind())
```

### `pyo3_async_runtimes::tokio::future_into_py` for blocking pymethods

**Source:** `fn xread` at `src/lib.rs:982-1038`.

**Apply to:** `brpop`, `blpop`, `blmove`. The store handle must be cloned before moving into the async block (line 979: `let store = self.store.clone();`).

### `extract_bytes` for key/value coercion at the PyO3 boundary

**Source:** `src/commands/strings.rs:8-19`.

**Apply to:** every `&Bound<'py, PyAny>` arg in new pymethods. Already imported in `src/lib.rs:15`.

### ValueData::List persistence

**Source:** `PersistableValueData` enum at `src/store.rs:2738-2745`, and the `from_store` / `into_runtime` match arms at lines 2799-2863 and 2899-2925.

**Apply to:** extend `PersistableValueData` with `List(Vec<Vec<u8>>)` (note: use `Vec<Vec<u8>>` not `VecDeque<Vec<u8>>` because `VecDeque` serde serializes as a sequence just like `Vec` but `Vec` is the established convention here per the Set arm at line 2742). Add a `from_store` arm (lines 2800-2808 style) and `into_runtime` arm (lines 2899-2912 style) to round-trip `ValueData::List(VecDeque<Bytes>)`.

## No Analog Found

| File | Role | Data Flow | Reason |
|------|------|-----------|--------|
| None | — | — | Every file has a close in-codebase analog. The BLMOVE cross-key-atomicity pattern is new (D-10 destination-write notify is a novel combination, but the building blocks are all from existing patterns) and is documented inline in the RESEARCH.md pseudo-code. |

## Metadata

**Analog search scope:**
- `src/` (all `.rs` files) — 4,244 + 2,923 + 1,873 + 326 lines across 4 primary files, plus 7 command helper files
- `python/burner_redis/` — 198 + 286 lines across `__init__.py` and `pipeline.py`
- `tests/` — 16 test files, primary reference `test_streams.py` (~1800 lines) and `test_pipeline.py`
- `.planning/` — REQUIREMENTS.md, ROADMAP.md, CONTEXT.md, RESEARCH.md

**Files scanned:** 24 primary files (read fully or via targeted Grep+Read).

**Pattern extraction date:** 2026-04-24.

**Key load-bearing patterns identified:**
1. **Notify-inside-write-lock at Store method boundary** (`store.rs:1262, 2402`) — must apply to LPUSH, RPUSH, LMOVE-dst, RPOPLPUSH-dst, BLMOVE-dst.
2. **Re-arm waiter idiom** in blocking loop (`lib.rs:1021-1023, 1324-1325`) — must apply to BRPOP/BLPOP/BLMOVE.
3. **`had_xadd` return-tuple flag** (`scripting.rs:274, 277`) — direct template for `had_list_mutation`.
4. **Sync fast path for all-non-blocking pipeline** (`lib.rs:2182-2197`) — must be preserved per quick task `260415-an2`.
5. **Monkey-patch coercion wrapper** (`__init__.py:64-80`) — direct template for LPUSH/RPUSH/LSET/LINSERT value coercion.
