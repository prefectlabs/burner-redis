# T01: Implement the three known compatibility fixes: XREADGROUP blocking support, XCLAIM command, and XTRIM approximate parameter.

**Slice:** S11 — **Milestone:** M001

## Description

Implement the three known compatibility fixes: XREADGROUP blocking support, XCLAIM command, and XTRIM approximate parameter.

Purpose: These are the concrete gaps identified by research that prevent pydocket's test suite from passing. The XREADGROUP blocking fix resolves the ~19% delayed task race (D-07). XCLAIM enables pydocket's lease renewal. XTRIM approximate prevents errors in docket.clear().

Output: All three fixes implemented across all four layers (Store, PyO3, Pipeline, Lua dispatch) with unit tests.

## Legacy Source

---
phase: 11-close-redis-py-compatibility-gaps-for-pydocket-integration
plan: 01
type: execute
wave: 1
depends_on: []
files_modified:
  - src/store.rs
  - src/lib.rs
  - src/scripting.rs
  - python/burner_redis/pipeline.py
  - tests/test_streams.py
autonomous: true
requirements: [D-03, D-06, D-07, D-08]

must_haves:
  truths:
    - "XREADGROUP with block parameter waits for new entries instead of returning empty immediately"
    - "XREADGROUP wakes up when XADD inserts data (both direct and via Lua scripts)"
    - "XREADGROUP blocking does NOT deadlock (lock released before waiting)"
    - "XCLAIM transfers PEL entries between consumers with correct idle time reset"
    - "XCLAIM returns claimed message data in redis-py format"
    - "XTRIM accepts approximate parameter without error"
  artifacts:
    - path: "src/store.rs"
      provides: "stream_notify field on Store, XCLAIM method, notification in XADD"
      contains: "stream_notify"
    - path: "src/lib.rs"
      provides: "Blocking XREADGROUP PyO3 binding, XCLAIM PyO3 binding, XTRIM approximate kwarg"
      contains: "stream_notify"
    - path: "src/scripting.rs"
      provides: "XCLAIM Lua dispatch entry, stream_xadd_occurred flag for Lua XADD notification"
      contains: "XCLAIM"
    - path: "python/burner_redis/pipeline.py"
      provides: "xclaim pipeline buffer method"
      contains: "def xclaim"
    - path: "tests/test_streams.py"
      provides: "XCLAIM unit tests, XREADGROUP blocking tests"
      contains: "test_xclaim"
  key_links:
    - from: "src/store.rs (xadd)"
      to: "src/store.rs (stream_notify)"
      via: "self.stream_notify.notify_waiters() after XADD insert"
      pattern: "stream_notify\\.notify_waiters"
    - from: "src/lib.rs (xreadgroup)"
      to: "src/store.rs (stream_notify)"
      via: "tokio::select! waiting on store.stream_notify.notified()"
      pattern: "stream_notified|stream_notify"
    - from: "src/scripting.rs (dispatch XADD)"
      to: "src/store.rs (eval/evalsha)"
      via: "Return signal that XADD occurred, caller fires notification"
      pattern: "xadd_occurred|stream_written"
---

<objective>
Implement the three known compatibility fixes: XREADGROUP blocking support, XCLAIM command, and XTRIM approximate parameter.

Purpose: These are the concrete gaps identified by research that prevent pydocket's test suite from passing. The XREADGROUP blocking fix resolves the ~19% delayed task race (D-07). XCLAIM enables pydocket's lease renewal. XTRIM approximate prevents errors in docket.clear().

Output: All three fixes implemented across all four layers (Store, PyO3, Pipeline, Lua dispatch) with unit tests.
</objective>

<execution_context>
@$HOME/.claude/get-shit-done/workflows/execute-plan.md
@$HOME/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@.planning/PROJECT.md
@.planning/ROADMAP.md
@.planning/STATE.md
@.planning/phases/11-close-redis-py-compatibility-gaps-for-pydocket-integration/11-CONTEXT.md
@.planning/phases/11-close-redis-py-compatibility-gaps-for-pydocket-integration/11-RESEARCH.md

<interfaces>
<!-- Key types and contracts the executor needs. Extracted from codebase. -->

From src/store.rs:
```rust
pub struct Store {
    data: RwLock<HashMap<Bytes, ValueEntry>>,
    scripts: RwLock<HashMap<String, String>>,
    pub(crate) pubsub: RwLock<PubSubRegistry>,
    // NEW: stream_notify will be added here
}

pub struct ConsumerGroup {
    pub last_delivered_id: StreamId,
    pub consumers: HashMap<Bytes, Consumer>,
}

pub struct Consumer {
    pub pending: HashMap<StreamId, PendingEntry>,
}

pub struct PendingEntry {
    pub delivery_time: Instant,
    pub delivery_count: u64,
}

pub enum StoreError {
    WrongType,
    NoGroup(String, String),
    BusyGroup,
    KeyNotFound,
}
```

From src/scripting.rs:
```rust
fn dispatch_command(
    cmd: &str,
    args: &[Bytes],
    data: &mut HashMap<Bytes, ValueEntry>,
    pubsub_tx: Option<&broadcast::Sender<PubSubMessage>>,
) -> Result<RedisValue, String>

pub fn execute(
    script: &str,
    keys: Vec<Bytes>,
    args: Vec<Bytes>,
    data: &mut HashMap<Bytes, ValueEntry>,
    pubsub_tx: Option<&broadcast::Sender<PubSubMessage>>,
) -> Result<RedisValue, String>
```

From src/lib.rs:
```rust
pub struct BurnerRedis {
    store: Arc<Store>,
    persistence_path: Option<String>,
}
```
</interfaces>
</context>

<tasks>

<task type="auto">
  <name>Task 1: XREADGROUP blocking support with tokio::sync::Notify</name>
  <read_first>
    - src/store.rs (lines 227-240 for Store struct and new(), lines 1080-1128 for xadd, lines 1442-1570 for xreadgroup)
    - src/lib.rs (lines 1087-1174 for xreadgroup PyO3 binding)
    - src/scripting.rs (lines 110-120 for execute() signature, lines 258-263 for dispatch_command signature, lines 1143-1218 for XADD dispatch)
  </read_first>
  <files>src/store.rs, src/lib.rs, src/scripting.rs, tests/test_streams.py</files>
  <action>
**1. Add `tokio::sync::Notify` to Store (src/store.rs):**

Add `use tokio::sync::Notify;` to imports. Add field `stream_notify: Arc<Notify>` to `Store` struct (after the `pubsub` field). Initialize in `Store::new()` as `stream_notify: Arc::new(Notify::new())`. Add a public accessor method:

```rust
/// Get a reference to the stream notification handle for async waiting.
pub fn stream_notify(&self) -> Arc<Notify> {
    self.stream_notify.clone()
}
```

**2. Fire notification from Store::xadd (src/store.rs):**

In the `xadd` method, after `stream.last_id = new_id;` (line ~1123) and before `Ok(new_id)`, add:
```rust
// Wake any blocking XREADGROUP waiters
self.stream_notify.notify_waiters();
```

**3. Signal XADD from Lua dispatch (src/scripting.rs):**

The challenge: `dispatch_command` operates on raw `&mut HashMap` without access to Store's Notify. Use a return-value signal approach.

Change `dispatch_command` return type from `Result<RedisValue, String>` to `Result<(RedisValue, bool), String>` where the bool indicates "stream XADD occurred". Update ALL call sites in `dispatch_command` to return `(result, false)` for all commands EXCEPT XADD which returns `(result, true)`.

In `LuaEngine::execute`, where dispatch_command results are used (inside the redis.call/redis.pcall closures), capture the bool. After the Lua script completes (after `lua.scope`), return `(RedisValue, bool)` from execute as well.

Then in `Store::eval` and `Store::evalsha` (src/store.rs lines 1964-1996), after `LuaEngine::execute(...)` returns, check the bool flag. If true, call `self.stream_notify.notify_waiters()` AFTER releasing the data write lock. Pattern:
```rust
pub fn eval(&self, script: &str, keys: Vec<Bytes>, args: Vec<Bytes>) -> Result<RedisValue, String> {
    let sha1 = LuaEngine::sha1_hex(script);
    self.scripts.write().insert(sha1, script.to_string());
    let pubsub_tx = self.pubsub_sender();
    let (result, had_xadd) = {
        let mut data = self.data.write();
        LuaEngine::execute(script, keys, args, &mut *data, Some(&pubsub_tx))?
        // data write lock drops here
    };
    if had_xadd {
        self.stream_notify.notify_waiters();
    }
    Ok(result)
}
```
Apply the same pattern to `evalsha`.

NOTE: The `LuaEngine::execute` function must propagate the `had_xadd` flag from any dispatch_command call during execution. Use a `Cell<bool>` or `RefCell<bool>` alongside the existing data RefCell pattern. Set it to true if any dispatch_command call returns `had_xadd=true`. Return it alongside the RedisValue from execute.

**4. Implement blocking XREADGROUP in PyO3 binding (src/lib.rs):**

Modify the `xreadgroup` method. Remove `#[allow(unused_variables)]` from the `block` parameter. In the async block inside `future_into_py`:

```rust
pyo3_async_runtimes::tokio::future_into_py(py, async move {
    // First non-blocking attempt
    let results = store
        .xreadgroup(&group, &consumer, &keys, &id_strs, count)
        .map_err(store_err_to_py)?;

    if !results.is_empty() || block.is_none() {
        // Return immediately if we have data or no blocking requested
        return format_xreadgroup_result(results);
    }

    // Blocking: wait for stream notification or timeout
    let block_ms = block.unwrap();
    let notify = store.stream_notify();
    let timeout_duration = std::time::Duration::from_millis(block_ms);

    tokio::select! {
        _ = notify.notified() => {
            // New data arrived, retry
            let results = store
                .xreadgroup(&group, &consumer, &keys, &id_strs, count)
                .map_err(store_err_to_py)?;
            format_xreadgroup_result(results)
        }
        _ = tokio::time::sleep(timeout_duration) => {
            // Timeout, return empty
            format_xreadgroup_result(Vec::new())
        }
    }
})
```

Extract the Python result formatting code (the `Python::try_attach(|py| ...)` blocks currently in xreadgroup) into a helper function `format_xreadgroup_result()` to avoid duplication. The helper takes `Vec<(Bytes, Vec<(StreamId, HashMap<Bytes, Bytes>)>)>` and returns `PyResult<Py<PyAny>>`.

CRITICAL: The Store's data write lock is NOT held during the `tokio::select!` wait. The `store.xreadgroup()` call acquires and releases the lock synchronously. The `notify.notified()` wait happens with NO lock held. This prevents deadlock (Pitfall 3 from research).

**5. Add XREADGROUP blocking tests (tests/test_streams.py):**

Add these test functions after the existing XREADGROUP tests:

```python
async def test_xreadgroup_block_returns_new_entries(r):
    """XREADGROUP with block waits for new entries added after the call."""
    await r.xadd("mystream", {"f": "v1"})
    await r.xgroup_create("mystream", "mygroup", id="0")
    # Read existing entry
    await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"})

    # Schedule an XADD after a short delay
    async def add_later():
        await asyncio.sleep(0.05)
        await r.xadd("mystream", {"f": "v2"})

    task = asyncio.create_task(add_later())
    # Block for up to 2000ms -- should return quickly after add_later fires
    result = await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"}, block=2000)
    await task
    assert len(result) > 0
    # Verify we got the new entry
    stream_name, entries = result[0]
    assert entries[0][1][b"f"] == b"v2"


async def test_xreadgroup_block_timeout_returns_empty(r):
    """XREADGROUP with block returns empty after timeout if no new data."""
    await r.xadd("mystream", {"f": "v1"})
    await r.xgroup_create("mystream", "mygroup", id="0")
    await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"})

    # Block for 50ms with no new data
    import time
    start = time.monotonic()
    result = await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"}, block=50)
    elapsed = time.monotonic() - start
    # Should return empty (either [] or None-ish)
    assert len(result) == 0
    # Should have waited approximately 50ms (at least 30ms to allow for timing variance)
    assert elapsed >= 0.03


async def test_xreadgroup_block_lua_xadd_wakes_reader(r):
    """XREADGROUP with block wakes up when XADD is done from a Lua script."""
    await r.xadd("mystream", {"f": "v1"})
    await r.xgroup_create("mystream", "mygroup", id="0")
    await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"})

    lua_script = r.register_script("""
    redis.call('XADD', KEYS[1], '*', 'f', ARGV[1])
    return 1
    """)

    async def lua_add_later():
        await asyncio.sleep(0.05)
        await lua_script(keys=["mystream"], args=["from_lua"])

    task = asyncio.create_task(lua_add_later())
    result = await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"}, block=2000)
    await task
    assert len(result) > 0
    stream_name, entries = result[0]
    assert entries[0][1][b"f"] == b"from_lua"
```

Add `import asyncio` at the top of the test file if not already present.
  </action>
  <verify>
    <automated>.venv/bin/python -m pytest tests/test_streams.py -k "test_xreadgroup_block" -x -q --tb=short</automated>
  </verify>
  <acceptance_criteria>
    - src/store.rs contains `stream_notify: Arc<Notify>` field in Store struct
    - src/store.rs contains `self.stream_notify.notify_waiters()` in xadd method
    - src/store.rs eval method contains `if had_xadd` check after LuaEngine::execute
    - src/lib.rs xreadgroup method contains `tokio::select!` with `notify.notified()`
    - src/lib.rs does NOT contain `#[allow(unused_variables)]` on the block parameter of xreadgroup
    - src/scripting.rs dispatch_command returns a tuple with bool flag for XADD occurrence
    - tests/test_streams.py contains `test_xreadgroup_block_returns_new_entries`
    - tests/test_streams.py contains `test_xreadgroup_block_timeout_returns_empty`
    - tests/test_streams.py contains `test_xreadgroup_block_lua_xadd_wakes_reader`
    - `.venv/bin/python -m pytest tests/test_streams.py -k "test_xreadgroup_block" -x` exits 0
    - `.venv/bin/python -m pytest tests/ -q -m "not integration" -x` exits 0 (no regressions)
  </acceptance_criteria>
  <done>
    XREADGROUP with block parameter waits for new entries (from both direct XADD and Lua XADD) and returns them. Timeout returns empty. No deadlock. All existing tests still pass.
  </done>
</task>

<task type="auto">
  <name>Task 2: XCLAIM command implementation + XTRIM approximate parameter</name>
  <read_first>
    - src/store.rs (lines 1620-1730 for xautoclaim pattern to follow for xclaim)
    - src/lib.rs (lines 1176-1230 for xack PyO3 binding pattern)
    - src/scripting.rs (lines 1383-1430 for XACK Lua dispatch pattern)
    - python/burner_redis/pipeline.py (lines 154-160 for xautoclaim pipeline pattern)
    - tests/test_streams.py (lines 451-567 for xautoclaim test patterns)
    - src/lib.rs (lines 890-910 for xtrim PyO3 binding)
    - python/burner_redis/pipeline.py (lines 132-136 for xtrim pipeline)
  </read_first>
  <files>src/store.rs, src/lib.rs, src/scripting.rs, python/burner_redis/pipeline.py, tests/test_streams.py</files>
  <action>
**1. Implement XCLAIM in Store (src/store.rs):**

Add the xclaim method near the xautoclaim method (after line ~1730). Follow the same lock-acquire, get-stream, get-consumer-group pattern:

```rust
/// XCLAIM: Transfer ownership of pending stream entries to a different consumer.
/// Moves PEL entries from their current consumer to the target consumer.
/// Only claims entries that have been idle for at least min_idle_time_ms.
/// If `idle` is Some, resets the entry's idle time to the specified ms value.
/// If `force` is true, creates the PEL entry even if it doesn't exist in any consumer's PEL.
/// Returns the claimed entries with their field data (or just IDs if justid is true).
pub fn xclaim(
    &self,
    key: &Bytes,
    group: &Bytes,
    consumer: Bytes,
    min_idle_time_ms: u64,
    ids: &[StreamId],
    idle: Option<u64>,
    time: Option<u64>,
    retrycount: Option<u64>,
    force: bool,
    justid: bool,
) -> Result<Vec<(StreamId, Option<HashMap<Bytes, Bytes>>)>, StoreError> {
    let mut data = self.data.write();

    // Passive expiration
    if let Some(entry) = data.get(key) {
        if entry.is_expired() {
            data.remove(key);
            return Err(StoreError::NoGroup(
                String::from_utf8_lossy(group.as_ref()).into_owned(),
                String::from_utf8_lossy(key.as_ref()).into_owned(),
            ));
        }
    }

    let entry = match data.get_mut(key) {
        None => {
            return Err(StoreError::NoGroup(
                String::from_utf8_lossy(group.as_ref()).into_owned(),
                String::from_utf8_lossy(key.as_ref()).into_owned(),
            ));
        }
        Some(e) => e,
    };

    let stream = match entry.data {
        ValueData::Stream(ref mut s) => s,
        _ => return Err(StoreError::WrongType),
    };

    let cg = match stream.groups.get_mut(group) {
        None => {
            return Err(StoreError::NoGroup(
                String::from_utf8_lossy(group.as_ref()).into_owned(),
                String::from_utf8_lossy(key.as_ref()).into_owned(),
            ));
        }
        Some(g) => g,
    };

    let now = Instant::now();
    let min_idle = Duration::from_millis(min_idle_time_ms);
    let mut claimed = Vec::new();

    for &id in ids {
        // Find the entry in any consumer's PEL
        let mut found_consumer: Option<Bytes> = None;
        let mut found_entry: Option<PendingEntry> = None;
        for (cname, c) in cg.consumers.iter() {
            if let Some(pe) = c.pending.get(&id) {
                let idle_ms = now.duration_since(pe.delivery_time);
                if idle_ms >= min_idle || force {
                    found_consumer = Some(cname.clone());
                    found_entry = Some(pe.clone());
                }
                break;
            }
        }

        // If force is set and entry not found in any PEL but exists in stream, create it
        if found_consumer.is_none() && force {
            if stream.entries.contains_key(&id) {
                // Create a synthetic PEL entry for the target consumer
                let new_delivery_time = match idle {
                    Some(idle_ms) => now - Duration::from_millis(idle_ms),
                    None => now,
                };
                let target = cg.consumers
                    .entry(consumer.clone())
                    .or_insert_with(|| Consumer { pending: HashMap::new() });
                target.pending.insert(id, PendingEntry {
                    delivery_time: new_delivery_time,
                    delivery_count: retrycount.unwrap_or(1),
                });
                if justid {
                    claimed.push((id, None));
                } else if let Some(fields) = stream.entries.get(&id) {
                    claimed.push((id, Some(fields.clone())));
                }
                continue;
            }
            continue;
        }

        if let (Some(from_consumer), Some(pe)) = (found_consumer, found_entry) {
            // Remove from source consumer's PEL
            if let Some(orig) = cg.consumers.get_mut(&from_consumer) {
                orig.pending.remove(&id);
            }

            // Determine new delivery time based on idle parameter
            let new_delivery_time = match idle {
                Some(idle_ms) => now - Duration::from_millis(idle_ms),
                None => pe.delivery_time, // Keep original
            };
            let new_delivery_count = retrycount.unwrap_or(pe.delivery_count + 1);

            // Add to target consumer's PEL
            let target = cg.consumers
                .entry(consumer.clone())
                .or_insert_with(|| Consumer { pending: HashMap::new() });
            target.pending.insert(id, PendingEntry {
                delivery_time: new_delivery_time,
                delivery_count: new_delivery_count,
            });

            // Return the entry data
            if justid {
                claimed.push((id, None));
            } else if let Some(fields) = stream.entries.get(&id) {
                claimed.push((id, Some(fields.clone())));
            }
        }
    }

    Ok(claimed)
}
```

**2. Add XCLAIM PyO3 binding (src/lib.rs):**

Add the binding near the xautoclaim binding. Follow the redis-py signature:

```rust
/// XCLAIM command matching redis.asyncio.Redis.xclaim() signature.
#[pyo3(signature = (name, groupname, consumername, min_idle_time, message_ids, idle=None, time=None, retrycount=None, force=false, justid=false))]
fn xclaim<'py>(
    &self,
    py: Python<'py>,
    name: &Bound<'py, PyAny>,
    groupname: &Bound<'py, PyAny>,
    consumername: &Bound<'py, PyAny>,
    min_idle_time: u64,
    message_ids: &Bound<'py, PyAny>,
    idle: Option<u64>,
    time: Option<u64>,
    retrycount: Option<u64>,
    force: bool,
    justid: bool,
) -> PyResult<Bound<'py, PyAny>> {
    let store = self.store.clone();
    let key = extract_bytes(name)?;
    let group = extract_bytes(groupname)?;
    let consumer = extract_bytes(consumername)?;

    // Parse message_ids from Python list/tuple of bytes/str
    let ids_list: Vec<Py<PyAny>> = message_ids.extract()?;
    let mut ids: Vec<StreamId> = Vec::new();
    for id_obj in &ids_list {
        Python::with_gil(|py| {
            let id_str: String = id_obj.bind(py).extract::<String>().or_else(|_| {
                id_obj.bind(py).extract::<Vec<u8>>()
                    .map(|b| String::from_utf8_lossy(&b).into_owned())
            })?;
            ids.push(parse_stream_id(&id_str).ok_or_else(|| {
                PyErr::new::<pyo3::exceptions::PyValueError, _>(
                    format!("Invalid stream ID: {}", id_str),
                )
            })?);
            Ok::<(), PyErr>(())
        })?;
    }

    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let results = store
            .xclaim(&key, &group, consumer, min_idle_time, &ids, idle, time, retrycount, force, justid)
            .map_err(store_err_to_py)?;

        Python::try_attach(|py| -> PyResult<Py<PyAny>> {
            let outer = pyo3::types::PyList::empty(py);
            for (id, fields_opt) in &results {
                if justid {
                    let id_bytes = format_stream_id(*id).into_bytes();
                    outer.append(PyBytes::new(py, &id_bytes))?;
                } else if let Some(fields) = fields_opt {
                    let id_bytes = format_stream_id(*id).into_bytes();
                    let field_dict = PyDict::new(py);
                    for (fk, fv) in fields {
                        field_dict.set_item(
                            PyBytes::new(py, fk.as_ref()),
                            PyBytes::new(py, fv.as_ref()),
                        )?;
                    }
                    let tuple = PyTuple::new(py, &[
                        PyBytes::new(py, &id_bytes).into_any(),
                        field_dict.into_any(),
                    ])?;
                    outer.append(tuple)?;
                }
            }
            Ok(outer.into_any().unbind())
        })
        .ok_or_else(|| {
            PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                "failed to attach to Python interpreter",
            )
        })?
    })
}
```

**3. Add XCLAIM Lua dispatch (src/scripting.rs):**

Add XCLAIM handling in dispatch_command near the XACK handler. Minimal Lua dispatch for pydocket use cases:

```rust
"XCLAIM" => {
    // XCLAIM key group consumer min-idle-time id [id ...] [IDLE ms] [FORCE] [JUSTID]
    if args.len() < 5 {
        return Ok((RedisValue::Error(
            "ERR wrong number of arguments for 'xclaim' command".to_string(),
        ), false));
    }
    let key = &args[0];
    let group = &args[1];
    let consumer = args[2].clone();
    let min_idle_str = String::from_utf8_lossy(&args[3]);
    let min_idle_time: u64 = min_idle_str.parse().map_err(|_| {
        "ERR Invalid min-idle-time argument for XCLAIM".to_string()
    })?;

    // Parse remaining args: IDs and optional flags
    let mut ids = Vec::new();
    let mut idle: Option<u64> = None;
    let mut force = false;
    let mut justid = false;
    let mut retrycount: Option<u64> = None;
    let mut i = 4;
    while i < args.len() {
        let arg_upper = String::from_utf8_lossy(&args[i]).to_uppercase();
        match arg_upper.as_str() {
            "IDLE" => {
                if i + 1 < args.len() {
                    idle = Some(String::from_utf8_lossy(&args[i+1]).parse().unwrap_or(0));
                    i += 2;
                    continue;
                }
                i += 1;
                continue;
            }
            "RETRYCOUNT" => {
                if i + 1 < args.len() {
                    retrycount = Some(String::from_utf8_lossy(&args[i+1]).parse().unwrap_or(0));
                    i += 2;
                    continue;
                }
                i += 1;
                continue;
            }
            "FORCE" => {
                force = true;
                i += 1;
                continue;
            }
            "JUSTID" => {
                justid = true;
                i += 1;
                continue;
            }
            _ => {
                // Must be a stream ID
                let id_str = String::from_utf8_lossy(&args[i]);
                if let Some(parsed) = crate::commands::streams::parse_stream_id(&id_str) {
                    ids.push(parsed);
                }
                i += 1;
            }
        }
    }

    // Execute XCLAIM against the data
    // (Follows same inline pattern as other stream commands in dispatch)
    // ... [inline implementation similar to Store::xclaim but operating on raw data]
}
```

For the dispatch implementation, follow the same inline pattern as XADD in Lua dispatch: directly access `data.get_mut(key)`, extract stream, get consumer group, iterate consumers' PELs, transfer entries. Return `(RedisValue::Array(...), false)` (XCLAIM doesn't trigger stream_notify since it doesn't add entries).

**4. Add XCLAIM Pipeline method (python/burner_redis/pipeline.py):**

Add after the xautoclaim method (around line 158):

```python
def xclaim(self, name, groupname, consumername, min_idle_time, message_ids,
           idle=None, time=None, retrycount=None, force=False, justid=False):
    self._commands.append(("xclaim", (name, groupname, consumername, min_idle_time, message_ids),
                          {"idle": idle, "time": time, "retrycount": retrycount,
                           "force": force, "justid": justid}))
    return self
```

**5. Add `approximate` parameter to XTRIM (src/lib.rs and pipeline.py):**

In src/lib.rs, modify the xtrim signature to accept `approximate`:
```rust
#[pyo3(signature = (name, maxlen=None, minid=None, approximate=true))]
fn xtrim<'py>(
    &self,
    py: Python<'py>,
    name: &Bound<'py, PyAny>,
    maxlen: Option<usize>,
    minid: Option<&str>,
    #[allow(unused_variables)]
    approximate: bool,
) -> PyResult<Bound<'py, PyAny>> {
```

The `approximate` parameter is accepted but ignored -- our embedded DB always trims exactly. The default is `true` matching redis-py's default.

In pipeline.py, modify xtrim to accept approximate:
```python
def xtrim(self, name, maxlen=None, minid=None, approximate=True):
    self._commands.append(("xtrim", (name,), {"maxlen": maxlen, "minid": minid, "approximate": approximate}))
    return self
```

**6. Add XCLAIM tests (tests/test_streams.py):**

Add test functions following the xautoclaim test pattern:

```python
# --- XCLAIM ---

async def test_xclaim_transfers_ownership(r):
    """XCLAIM transfers pending entries from one consumer to another."""
    id1 = await r.xadd("mystream", {"f": "v1"})
    id2 = await r.xadd("mystream", {"f": "v2"})
    await r.xgroup_create("mystream", "mygroup", id="0")
    await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"})

    # consumer2 claims both entries
    result = await r.xclaim("mystream", "mygroup", "consumer2", 0, [id1, id2])
    assert len(result) == 2
    assert result[0][1][b"f"] == b"v1"
    assert result[1][1][b"f"] == b"v2"


async def test_xclaim_resets_idle_time(r):
    """XCLAIM with idle=0 resets the entry's idle time."""
    id1 = await r.xadd("mystream", {"f": "v1"})
    await r.xgroup_create("mystream", "mygroup", id="0")
    await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"})

    # Wait a tiny bit so entry has some idle time
    import asyncio
    await asyncio.sleep(0.01)

    # Claim with idle=0 (reset idle time) -- same consumer (lease renewal pattern)
    result = await r.xclaim("mystream", "mygroup", "consumer1", 0, [id1], idle=0)
    assert len(result) == 1


async def test_xclaim_respects_min_idle_time(r):
    """XCLAIM skips entries not idle long enough."""
    id1 = await r.xadd("mystream", {"f": "v1"})
    await r.xgroup_create("mystream", "mygroup", id="0")
    await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"})

    # Claim with huge min_idle_time -- nothing qualifies
    result = await r.xclaim("mystream", "mygroup", "consumer2", 999999, [id1])
    assert len(result) == 0


async def test_xclaim_justid_returns_ids_only(r):
    """XCLAIM with justid=True returns only IDs, not field data."""
    id1 = await r.xadd("mystream", {"f": "v1"})
    await r.xgroup_create("mystream", "mygroup", id="0")
    await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"})

    result = await r.xclaim("mystream", "mygroup", "consumer2", 0, [id1], justid=True)
    assert len(result) == 1
    # Should be just the ID bytes, not a tuple
    assert isinstance(result[0], bytes)


async def test_xclaim_nonexistent_id_is_skipped(r):
    """XCLAIM silently skips IDs not in any consumer's PEL."""
    id1 = await r.xadd("mystream", {"f": "v1"})
    await r.xgroup_create("mystream", "mygroup", id="0")
    await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"})

    result = await r.xclaim("mystream", "mygroup", "consumer2", 0, ["99999-0"])
    assert len(result) == 0
```

Add a test for XTRIM approximate:

```python
async def test_xtrim_accepts_approximate_parameter(r):
    """XTRIM accepts approximate parameter without error."""
    await r.xadd("mystream", {"f": "v1"})
    await r.xadd("mystream", {"f": "v2"})
    await r.xadd("mystream", {"f": "v3"})

    # Should work with approximate=False (pydocket's docket.clear() pattern)
    trimmed = await r.xtrim("mystream", maxlen=0, approximate=False)
    assert trimmed == 3
```
  </action>
  <verify>
    <automated>.venv/bin/python -m pytest tests/test_streams.py -k "test_xclaim or test_xtrim_accepts_approximate" -x -q --tb=short</automated>
  </verify>
  <acceptance_criteria>
    - src/store.rs contains `pub fn xclaim(` method
    - src/store.rs xclaim method has parameters: key, group, consumer, min_idle_time_ms, ids, idle, time, retrycount, force, justid
    - src/lib.rs contains `fn xclaim` PyO3 method with `#[pyo3(signature = (name, groupname, consumername, min_idle_time, message_ids, idle=None, time=None, retrycount=None, force=false, justid=false))]`
    - src/lib.rs xtrim signature contains `approximate`
    - src/scripting.rs dispatch_command contains `"XCLAIM"` match arm
    - python/burner_redis/pipeline.py contains `def xclaim(self,`
    - python/burner_redis/pipeline.py xtrim method contains `approximate`
    - tests/test_streams.py contains `test_xclaim_transfers_ownership`
    - tests/test_streams.py contains `test_xclaim_resets_idle_time`
    - tests/test_streams.py contains `test_xclaim_respects_min_idle_time`
    - tests/test_streams.py contains `test_xclaim_justid_returns_ids_only`
    - tests/test_streams.py contains `test_xtrim_accepts_approximate`
    - `.venv/bin/python -m pytest tests/test_streams.py -k "test_xclaim or test_xtrim_accepts_approximate" -x` exits 0
    - `.venv/bin/python -m pytest tests/ -q -m "not integration" -x` exits 0 (no regressions)
  </acceptance_criteria>
  <done>
    XCLAIM is fully implemented across Store, PyO3, Pipeline, and Lua dispatch layers. XTRIM accepts the approximate parameter. All XCLAIM tests pass. All existing tests pass.
  </done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|----------|-------------|
| Python -> Rust | User-provided arguments (stream IDs, consumer names) cross into Rust via PyO3 |
| Lua -> Store | Lua scripts dispatch commands with user-controlled arguments |

## STRIDE Threat Register

| Threat ID | Category | Component | Disposition | Mitigation Plan |
|-----------|----------|-----------|-------------|-----------------|
| T-11-01 | D (Denial of Service) | XREADGROUP block | mitigate | Block duration capped by user-provided `block` parameter in milliseconds. Tokio select! ensures the future completes within timeout. No unbounded waits. |
| T-11-02 | D (Denial of Service) | Store::stream_notify | accept | Global Notify means unrelated XADD wakes all blocked readers. Spurious wakeups cause one extra read attempt (O(1) overhead). Acceptable for embedded single-process use. |
| T-11-03 | T (Tampering) | XCLAIM | accept | XCLAIM transfers PEL ownership without authentication. Acceptable because this is an embedded in-process database with no auth boundary (documented in REQUIREMENTS.md Out of Scope). |
| T-11-04 | I (Information Disclosure) | XCLAIM | accept | XCLAIM reveals message content to the claiming consumer. Same trust boundary as all other read operations -- acceptable for in-process use. |
| T-11-05 | D (Denial of Service) | Lua XCLAIM dispatch | mitigate | Validate argument count and format before processing. Return ERR for malformed arguments rather than panicking. |
</threat_model>

<verification>
1. `maturin develop` compiles without errors
2. `.venv/bin/python -m pytest tests/test_streams.py -x -q` -- all stream tests pass including new XCLAIM and blocking tests
3. `.venv/bin/python -m pytest tests/ -q -m "not integration" -x` -- full unit test suite passes (no regressions)
4. No deadlock: blocking XREADGROUP + concurrent XADD completes within test timeout
</verification>

<success_criteria>
- XREADGROUP with block parameter waits for new data and returns it
- XREADGROUP wakes up for both direct XADD and Lua-script XADD
- XREADGROUP blocking does not deadlock
- XCLAIM transfers PEL entries between consumers
- XCLAIM supports idle, force, justid, retrycount, min_idle_time parameters
- XTRIM accepts approximate parameter without error
- All 282+ existing tests continue to pass
- New tests verify each behavior
</success_criteria>

<output>
After completion, create `.planning/phases/11-close-redis-py-compatibility-gaps-for-pydocket-integration/11-01-SUMMARY.md`
</output>
