---
quick_id: 260416-cea
type: execute
wave: 1
depends_on: []
files_modified:
  - tests/test_streams.py
  - src/store.rs
  - src/lib.rs
autonomous: true
requirements: []

must_haves:
  truths:
    - "xread(block=N) on a stream with no new entries awaits up to N milliseconds while cooperating with the asyncio event loop; a concurrent xadd from another coroutine wakes the reader promptly (well under N)"
    - "xread(block=None) still returns immediately (non-blocking fast path preserved)"
    - "xread(block=0) blocks indefinitely until new data arrives, respecting task cancellation"
    - "xread accepts '$' (and b'$') as a stream ID meaning 'resolve to the stream's current last-generated-id at call time and read entries strictly after it'; nonexistent streams resolve '$' to (0,0)"
    - "xread '$' is evaluated ONCE at call time, not re-resolved on wakeup (so an xadd after the call started returns as NEW, not as already-seen)"
    - "xreadgroup continues to reject '$' (uses '>' instead); '$' support is xread-only"
    - "BurnerRedis.xinfo_stream(name) returns a dict matching redis-py keys (str keys, not bytes): 'length', 'radix-tree-keys', 'radix-tree-nodes', 'last-generated-id' (bytes), 'groups' (int), 'first-entry' (tuple | None), 'last-entry' (tuple | None)"
    - "xinfo_stream against a nonexistent or expired stream raises redis.exceptions.ResponseError whose message contains 'no such key'"
    - "xread blocking path and xreadgroup blocking path share the same notify/select plumbing (DRY) — a single xadd wakes waiters registered via either command"
  artifacts:
    - path: "tests/test_streams.py"
      provides: "RED-then-GREEN tests for all three gaps, added alongside the existing XREAD, XREADGROUP Blocking, and XINFO sections"
      contains: "test_xread_block_yields_to_event_loop, test_xread_block_timeout_returns_empty, test_xread_block_none_is_non_blocking, test_xread_dollar_id_returns_only_new_entries, test_xread_dollar_id_on_missing_stream, test_xreadgroup_dollar_id_still_rejected, test_xinfo_stream_basic, test_xinfo_stream_empty_stream, test_xinfo_stream_missing_key_raises, test_xinfo_stream_with_groups"
    - path: "src/lib.rs"
      provides: "xread blocking path (future_into_py + tokio::select! + stream_notify.notified), '$' resolution at xread call-time, new xinfo_stream pymethod on BurnerRedis that returns a dict with str keys"
      contains: "fn xinfo_stream"
    - path: "src/store.rs"
      provides: "Store::xinfo_stream(key) returning a structured snapshot (length, last_id, groups count, first/last entry); reuses existing Stream fields"
      contains: "pub fn xinfo_stream"
  key_links:
    - from: "src/lib.rs xread blocking path"
      to: "store.stream_notify().notified()"
      via: "future_into_py + tokio::select! — must mirror xreadgroup (lib.rs:1122-1179), with notify registered BEFORE first poll to close the notify race window"
      pattern: "stream_notify\\(\\).*notified"
    - from: "src/lib.rs xread '$' handling"
      to: "Store internal last_id per stream"
      via: "resolve '$' -> current stream.last_id at the top of xread BEFORE entering the blocking loop; pass the resolved StreamId to store.xread() thereafter"
      pattern: "\\$.*last_id|resolve_dollar"
    - from: "src/lib.rs xinfo_stream"
      to: "redis.exceptions.ResponseError('no such key')"
      via: "make_response_error when Store::xinfo_stream returns KeyNotFound / None-shaped result"
      pattern: "no such key"
---

<objective>
Close three redis-py stream compatibility gaps that force downstream consumers (pydocket, Prefect) to special-case BurnerRedis:

1. **xread(block=N) does not block.** It returns immediately even when new entries could arrive within the window. Real redis-py / Redis block for up to N ms.
2. **xread does not accept `$` as a stream ID.** Real Redis treats `$` as "only messages strictly after the current end of stream"; currently we raise `ValueError: Invalid stream ID: $`.
3. **xinfo_stream(name) is missing.** Downstream code calling `await client.xinfo_stream("s")` gets `AttributeError`.

Purpose: BurnerRedis is a drop-in replacement for `redis.asyncio.Redis`. These three missing behaviors are the only remaining stream-command blockers (xreadgroup blocking was fixed in 260415-vor; xinfo_groups str-keys was fixed earlier). After this change, no known stream-API gap remains between BurnerRedis and redis-py.

Output: Three independently-committable fixes (TDD: failing test commit + implementation commit per gap). The xread(block=N) fix MUST share the Notify plumbing already used by xreadgroup(block=N); extract to a helper if reuse is not already clean. xinfo_stream MUST return str-keyed dict matching the recently-established convention from xinfo_groups.
</objective>

<execution_context>
@$HOME/.claude/get-shit-done/workflows/execute-plan.md
@$HOME/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@.planning/STATE.md
@.planning/PROJECT.md
@./CLAUDE.md
@src/lib.rs
@src/store.rs
@src/commands/streams.rs
@python/burner_redis/__init__.py
@tests/conftest.py
@tests/test_streams.py
@.planning/quick/260415-vor-fix-three-redis-py-compat-issues-xreadgr/260415-vor-PLAN.md

<interfaces>
<!-- Key existing contracts. Use these directly — no further exploration needed. -->

From src/store.rs:
```rust
// Stream struct (src/store.rs:68-83):
pub struct Stream {
    pub entries: BTreeMap<StreamId, HashMap<Bytes, Bytes>>,
    pub last_id: StreamId,                        // source of truth for "$"
    pub groups: HashMap<Bytes, ConsumerGroup>,    // len() gives "groups" count
}

// StreamId alias (src/commands/streams.rs):
pub type StreamId = (u64, u64);

// Notification primitive (src/store.rs:239,248,253-255):
stream_notify: Arc<Notify>,
pub fn stream_notify(&self) -> Arc<Notify> { self.stream_notify.clone() }
// Fired by: xadd (store.rs:1200), xclaim (:2275), xautoclaim (:2303).

// Existing xread entrypoint (store.rs:1211-1254) — takes pre-parsed StreamIds:
pub fn xread(
    &self,
    keys: &[Bytes],
    ids: &[StreamId],
    count: Option<usize>,
) -> Result<Vec<(Bytes, Vec<(StreamId, HashMap<Bytes, Bytes>)>)>, StoreError>;

// Error variants (store.rs:178-193):
pub enum StoreError {
    WrongType,
    NoGroup(String, String),   // (group, key), "NOGROUP No such key '{1}' or consumer group '{0}'"
    BusyGroup,
    KeyNotFound,               // "ERR The XGROUP subcommand requires the key to exist"
}
// Note: no "NoSuchKey" variant with xinfo phrasing exists — handle the missing-stream
// message for xinfo_stream at the lib.rs layer (make_response_error("ERR no such key")).
```

From src/lib.rs:
```rust
// Existing xreadgroup blocking template (lib.rs:1122-1179) — mirror this EXACTLY for xread.
// Key points:
//   - Non-blocking path when block.is_none(): synchronous store.xread() call, return result.
//   - Blocking path: pyo3_async_runtimes::tokio::future_into_py(py, async move { ... }).
//   - Register Notify::notified() BEFORE first store poll so a notification fired between
//     the first read and the select! await is not lost. Pin via Box::pin. Call .enable().
//   - Loop with tokio::select! { _ = waiter.as_mut() => { re-arm waiter and re-poll }
//                                _ = tokio::time::sleep(remaining) => { return empty } }
//   - block=0 (infinite) maps to a very-long deadline OR a pure notify-without-sleep arm.
//     Simplest: block=0 -> Duration::MAX sentinel -> skip the sleep arm entirely. Matches redis-py.

// Error helper (lib.rs:81-95):
fn make_response_error(msg: String) -> PyErr;

// Existing xread signature (lib.rs:813-890):
#[pyo3(signature = (streams, count=None, block=None))]
fn xread<'py>(&self, py: Python<'py>,
              streams: &Bound<'py, PyDict>,
              count: Option<usize>,
              block: Option<u64>) -> PyResult<Bound<'py, PyAny>>;
// TODAY: `block` has `#[allow(unused_variables)]` and the comment "// Accepted for API
// compatibility, ignored (in-process DB)". Remove that attribute and implement the blocking path.

// Existing xinfo_groups (lib.rs:1376-1424) for reference — the str-keyed dict convention
// this plan's xinfo_stream must follow:
//   dict.set_item(PyString::new(py, "name"), PyBytes::new(py, ...))  // str keys!

// Format helper already exists (lib.rs:27ff or similar):
fn format_stream_id(id: StreamId) -> String;  // "ms-seq"
fn parse_stream_id(s: &str) -> Option<StreamId>;
```

From existing blocking xreadgroup tests (tests/test_streams.py:907-1053) — pattern to mirror for xread:
```python
# test_xreadgroup_block_yields_to_event_loop is the gold standard. Copy its shape for xread:
#   - Spawn a tick-counter coroutine proving the event loop keeps scheduling.
#   - Spawn an xadd-after-delay coroutine.
#   - Assert wall-time < 1s (proves wakeup works) AND tick counter >= 5 (proves no GIL starvation).
# This matters because a naive "block=N ignored" fix that uses tokio::time::sleep only
# (without stream_notify) would make the wall-time assertion pass with a hardcoded sleep.
# The event-loop + wakeup assertions together prove both correctness and cooperation.
```

Existing test fixture (tests/conftest.py):
```python
@pytest.fixture
def r():
    return BurnerRedis()  # session-fresh, no shared state
```
</interfaces>
</context>

<tasks>

<task type="auto" tdd="true">
  <name>Task 1: Gap 1 — xread(block=N) blocks and yields to the asyncio event loop (share plumbing with xreadgroup)</name>
  <files>tests/test_streams.py, src/lib.rs</files>
  <behavior>
    Add these tests to tests/test_streams.py under a new "# --- XREAD Blocking ---" section (place it right before the existing "# --- XREADGROUP Blocking ---" section for symmetry, around line 905):

    - test_xread_block_returns_new_entries:
        * XADD one seed entry, capture returned id as `last_id`.
        * Spawn an xadd-after-50ms coroutine adding {"f": "v2"}.
        * `result = await r.xread({"mystream": last_id}, count=10, block=2000)` — wall-time should be < 1s.
        * Assert result is non-None and contains the v2 entry.

    - test_xread_block_timeout_returns_empty:
        * XADD one seed entry, capture `last_id`.
        * `start = time.monotonic(); result = await r.xread({"mystream": last_id}, count=10, block=50); elapsed = time.monotonic() - start`
        * Assert result is None (xread returns None when empty — see lib.rs:853-855) and `elapsed >= 0.03`.

    - test_xread_block_none_is_non_blocking:
        * XADD one seed, capture `last_id`.
        * `start = time.monotonic(); result = await r.xread({"mystream": last_id}, count=10); elapsed = time.monotonic() - start` (block defaults to None).
        * Assert `elapsed < 0.05` (fast path unchanged) and result is None.

    - test_xread_block_yields_to_event_loop:
        * Mirror test_xreadgroup_block_yields_to_event_loop (tests/test_streams.py:968-1022) exactly, but call `r.xread({"mystream": last_id}, count=10, block=5000)`.
        * The tick-counter coroutine must advance >= 5 times during the block; wall-time < 1s after a 50ms-delayed xadd.

    - test_xread_block_zero_blocks_until_data:
        * XADD seed, capture `last_id`.
        * Spawn an xadd-after-100ms coroutine.
        * `result = await asyncio.wait_for(r.xread({"mystream": last_id}, count=10, block=0), timeout=2.0)` — MUST NOT raise asyncio.TimeoutError.
        * Assert the new entry is returned.

    - test_xread_block_multiple_streams:
        * XADD seed into streams "s1" and "s2", capture both ids.
        * Spawn "xadd to s2 after 50ms".
        * `result = await r.xread({"s1": id_s1, "s2": id_s2}, count=10, block=2000)` — must return with the s2 entry.

    Run `uv run pytest tests/test_streams.py -k "xread_block" -x -v` and CONFIRM all five tests fail with the current implementation (block param is ignored today — returns immediately with None, so elapsed ≈ 0 and blocking/wakeup assertions all fail). Commit these tests FIRST as a RED commit:
        `test(quick-260416-cea): add failing tests for xread(block=N) blocking`
  </behavior>
  <action>
    Implementation (GREEN commit after RED).

    1. **Refactor xread in src/lib.rs (lines 813-890) to mirror xreadgroup's dual-path structure:**

       Non-blocking path (block is None) — keep the existing synchronous code exactly as-is for the fast path.

       Blocking path (block is Some):
       ```rust
       // After extracting keys/ids from the dict (same as today):
       let store = self.store.clone();
       let count_opt = count;
       let block_ms = block.unwrap();  // u64

       pyo3_async_runtimes::tokio::future_into_py(py, async move {
           let notify = store.stream_notify();
           let mut waiter = Box::pin(notify.notified());
           waiter.as_mut().enable();

           // First non-blocking attempt
           let results = store.xread(&keys, &ids, count_opt).map_err(store_err_to_py)?;
           if !results.is_empty() {
               return format_xread_result(results);   // new helper, see step 3
           }

           // Blocking wait
           let deadline_opt = if block_ms == 0 {
               None   // block=0 means block forever
           } else {
               Some(tokio::time::Instant::now() + Duration::from_millis(block_ms))
           };

           loop {
               let remaining = match deadline_opt {
                   Some(d) => {
                       let r = d.saturating_duration_since(tokio::time::Instant::now());
                       if r.is_zero() { break format_xread_result(Vec::new()); }
                       r
                   }
                   None => Duration::from_secs(3600),  // long per-iteration slice; re-armed on wakeup. See note below.
               };

               tokio::select! {
                   _ = waiter.as_mut() => {
                       waiter.set(notify.notified());
                       waiter.as_mut().enable();
                       let results = store.xread(&keys, &ids, count_opt).map_err(store_err_to_py)?;
                       if !results.is_empty() { break format_xread_result(results); }
                       // Otherwise: notification was for a different stream; re-arm + continue.
                   }
                   _ = tokio::time::sleep(remaining) => {
                       if deadline_opt.is_some() {
                           break format_xread_result(Vec::new());
                       }
                       // deadline_opt is None (block=0): sleep completed without wakeup; keep looping.
                   }
               }
           }
       })
       ```

       Notes:
         - For `block_ms == 0` (infinite), the `deadline_opt = None` arm never times out — the sleep branch just loops. An alternative is `tokio::select!` without a sleep arm at all when deadline is None; that is slightly cleaner. Pick whichever is clearer — both are correct.
         - Compare the structure to xreadgroup (lib.rs:1122-1179) side by side to make sure timing semantics match.

    2. **DRY sharing with xreadgroup** — REVIEW after writing the xread blocking loop:
        - If the xread blocking loop is functionally identical to xreadgroup's apart from the store call being `store.xread(...)` vs `store.xreadgroup(...)`, extract a private helper like:
          ```rust
          async fn block_and_poll<F, R>(
              notify: Arc<Notify>,
              block_ms: u64,
              mut poll: F,
          ) -> PyResult<R>
          where F: FnMut() -> PyResult<Option<R>> + Send,
                R: Send,
          ```
          and have both xread and xreadgroup call into it. The Store call sites would each pass a closure that calls their respective `store.xread(...)` / `store.xreadgroup(...)` and returns `Ok(Some(formatted))` on non-empty or `Ok(None)` on empty.
        - If extracting the helper would require `Box<dyn Future>` trait gymnastics that obscure more than they share, leave both blocking loops as duplicated-but-obvious code and add a comment in both pointing to the other: `// NOTE: Keep in sync with xreadgroup blocking loop (lib.rs:NNNN) — DRY bailed; duplication is intentional`. Err toward keeping them duplicated-and-obvious over a shared helper that fights Rust's type system. The user's "DRY" guidance is a preference, not a blocker.

    3. **Add `format_xread_result(results) -> PyResult<Py<PyAny>>`** — analogous to `format_xreadgroup_result` (lib.rs:162-180). Reuses `Python::try_attach` to acquire the GIL once at completion. Key difference: xread's empty result returns `None` (not empty list), matching the existing sync xread (lib.rs:853-855).

    4. **Remove `#[allow(unused_variables)]` on the `block` parameter** (lib.rs:822-823) and delete the "// Accepted for API compatibility, ignored" comment — the parameter is now meaningful.

    5. **Rebuild and run tests:** `uv run maturin develop --release && uv run pytest tests/test_streams.py -k "xread_block" -x -v`. All five new tests must pass. Existing xreadgroup block tests must still pass (they shouldn't be touched, but if the DRY helper was extracted, re-run `-k "block"` to cover both).

    Commit message for the impl:
        `fix(quick-260416-cea): implement xread(block=N) with stream_notify wakeup`
  </action>
  <verify>
    <automated>cd /Users/alexander/dev/prefectlabs/burner-redis && uv run maturin develop --release 2>&1 | tail -10 && uv run pytest tests/test_streams.py -k "xread_block or xreadgroup_block" -x -v</automated>
  </verify>
  <done>
    All five new xread-block tests pass. Wall-clock elapsed for the yields-to-event-loop test < 1.0s, tick counter >= 5. block=None path unchanged (< 50ms). block=0 returns the new entry without TimeoutError. All existing xreadgroup_block_* tests still pass (no regression from any shared-helper refactor). RED commit (tests only) precedes GREEN commit (impl) in the git log.
  </done>
</task>

<task type="auto" tdd="true">
  <name>Task 2: Gap 2 — xread accepts '$' meaning 'strictly after current last-generated-id at call time'</name>
  <files>tests/test_streams.py, src/lib.rs</files>
  <behavior>
    Add tests to tests/test_streams.py under a new "# --- XREAD '$' ID ---" subsection (place near the other xread tests, before XREAD Blocking):

    - test_xread_dollar_id_returns_only_new_entries:
        * XADD initial entry: `initial_id = await r.xadd("s", {"initial": "1"})`.
        * Spawn adder coroutine: `await asyncio.sleep(0.05); await r.xadd("s", {"after-dollar": "1"})`.
        * `result = await r.xread({"s": "$"}, count=10, block=1000)`.
        * Assert result is not None; assert only the "after-dollar" entry is returned — NOT the "initial" entry.

    - test_xread_dollar_id_as_bytes:
        * Same as above but pass `{b"s": b"$"}` — must work identically. '$' accepted as both str and bytes.

    - test_xread_dollar_id_non_blocking_returns_none:
        * XADD initial entry. `result = await r.xread({"s": "$"})` (no block).
        * Assert result is None — `$` means "after current end", and there is no new data, so non-blocking returns None immediately.

    - test_xread_dollar_id_on_missing_stream:
        * Do NOT xadd anything. `result = await r.xread({"nostream": "$"})`.
        * Assert result is None (non-existent stream resolves `$` to (0,0), store skips non-existent streams, caller gets None). Must NOT raise.

    - test_xread_dollar_id_resolved_at_call_time:
        * XADD entry1, entry2, entry3 sequentially (ids captured).
        * Spawn an adder that sleeps 100ms then xadds entry4.
        * `result = await r.xread({"s": "$"}, count=10, block=1000)` — result should contain ONLY entry4, proving `$` was resolved at call time (not re-resolved on wakeup — if re-resolved after notify, the result would be empty because the new last_id would equal entry4's id).

    - test_xreadgroup_dollar_id_still_rejected:
        * `await r.xgroup_create("s", "g", id="0", mkstream=True)`.
        * Assert `pytest.raises((redis.exceptions.ResponseError, ValueError))` when calling `await r.xreadgroup("g", "c", {"s": "$"})` — xreadgroup uses `>`, not `$`. The specific exception type depends on where the rejection happens; ValueError from parse_stream_id or ResponseError from a deliberate check both acceptable. Use `pytest.raises(Exception)` with `match=r"\$|Invalid stream ID"` if the class is uncertain.

    Run `uv run pytest tests/test_streams.py -k "dollar" -x -v` and confirm all fail with the current code (ValueError: Invalid stream ID: $). Commit as RED:
        `test(quick-260416-cea): add failing tests for xread '$' stream ID`
  </behavior>
  <action>
    Implementation (GREEN commit after RED).

    1. **In src/lib.rs xread (lines 826-849), change the id-parsing loop** so `$` is recognized and resolved to the stream's current last_id at call time:

       Current code:
       ```rust
       let stream_id = if id_str == "0" || id_str == "0-0" {
           (0u64, 0u64)
       } else {
           parse_stream_id(&id_str).ok_or_else(|| ...)?
       };
       ```

       New code:
       ```rust
       let stream_id = if id_str == "0" || id_str == "0-0" {
           (0u64, 0u64)
       } else if id_str == "$" {
           // Resolve to the stream's current last_id. Missing stream -> (0,0).
           self.store.stream_last_id(&key).unwrap_or((0, 0))
       } else {
           parse_stream_id(&id_str).ok_or_else(|| ... )?
       };
       ```

       This resolution happens BEFORE the blocking path is entered, so `$` is fixed at call time (satisfies test_xread_dollar_id_resolved_at_call_time).

    2. **Add `pub fn stream_last_id(&self, key: &Bytes) -> Option<StreamId>` to src/store.rs** (place near other stream helpers, e.g. after xlen around line 1277):
       ```rust
       /// Returns the current last-generated-id of a stream, or None if the key
       /// doesn't exist or holds a non-stream value. Used by lib.rs to resolve
       /// "$" at xread call time.
       pub fn stream_last_id(&self, key: &Bytes) -> Option<StreamId> {
           let mut data = self.data.write();
           if let Some(entry) = data.get(key) {
               if entry.is_expired() {
                   data.remove(key);
                   return None;
               }
           }
           match data.get(key)? {
               entry => match &entry.data {
                   ValueData::Stream(s) => Some(s.last_id),
                   _ => None,   // WrongType silently becomes None; xread's own type-check will raise if needed
               },
           }
       }
       ```
       Justification for Option-over-Result: we want to treat missing-stream and wrong-type identically at the caller ("resolve to 0-0"). If the key is wrong-type, the subsequent `store.xread()` call will raise WrongType anyway — preserving that error path.

    3. **Apply the same `$` resolution to the pipeline xread branch** (lib.rs:2176-2210). Add the `id_str == "$"` branch with `self.store.stream_last_id(&key).unwrap_or((0,0))` at line ~2184 where the current `"0"/"0-0"` branch lives. Keep the pipeline xread non-blocking (pipelines are inherently synchronous; `$` just needs to resolve at pipeline-execution time).

    4. **Verify xreadgroup still rejects '$'** — search lib.rs xreadgroup path (line 1080+) and the store.rs xreadgroup (line 1519+). The store currently passes id_str straight through to `parse_stream_id(id_str).unwrap_or((0, 0))` (store.rs:1605). Add an explicit rejection near the top of xreadgroup's id handling in store.rs, OR more simply: in lib.rs xreadgroup (around lines 1103-1111), add a check `if id_str == "$" { return Err(make_response_error("ERR the $ ID meaning is only valid within XREAD")); }` before pushing into id_strs. This makes the behavior explicit and the error message matches Redis.

    5. **Rebuild and run tests:** `uv run maturin develop --release && uv run pytest tests/test_streams.py -k "dollar" -x -v`. All six new tests pass. Existing xread / xreadgroup tests still pass.

    Commit message:
        `fix(quick-260416-cea): accept '$' as stream ID in xread (resolved at call time)`
  </action>
  <verify>
    <automated>cd /Users/alexander/dev/prefectlabs/burner-redis && uv run maturin develop --release 2>&1 | tail -10 && uv run pytest tests/test_streams.py -k "dollar or xread" -x -v</automated>
  </verify>
  <done>
    All six dollar-ID tests pass. `$` resolves to stream.last_id at call time (not wakeup time). `$` on missing stream resolves to (0,0) without raising. xreadgroup still rejects `$`. pipeline xread also accepts `$`. No regression in existing tests.
  </done>
</task>

<task type="auto" tdd="true">
  <name>Task 3: Gap 3 — add BurnerRedis.xinfo_stream(name) returning str-keyed dict matching redis-py</name>
  <files>tests/test_streams.py, src/store.rs, src/lib.rs</files>
  <behavior>
    Add tests to tests/test_streams.py in a new "# --- XINFO STREAM ---" section (place it directly after the existing "# --- STRM-11: XINFO CONSUMERS ---" section, around line 720):

    - test_xinfo_stream_basic:
        * `last_id = await r.xadd("s", {"f": "v1"})`.
        * `info = await r.xinfo_stream("s")`.
        * Assert keys are str (not bytes): `assert all(isinstance(k, str) for k in info.keys())`.
        * Assert `info["length"] == 1`.
        * Assert `info["last-generated-id"] == last_id` (bytes).
        * Assert `info["groups"] == 0` (no groups yet).
        * Assert `info["first-entry"][0] == last_id` (bytes) and `info["first-entry"][1] == {b"f": b"v1"}`.
        * Assert `info["last-entry"]` equals `info["first-entry"]` when there is only one entry.
        * Assert `"radix-tree-keys" in info` and `"radix-tree-nodes" in info` (values are ints >= 0 — we can stub them as 0 or len(entries); test just checks presence + type).

    - test_xinfo_stream_multiple_entries:
        * Three xadds. Assert info["length"] == 3, first-entry != last-entry, last-generated-id == third id.

    - test_xinfo_stream_with_groups:
        * `await r.xadd("s", {"f": "v1"})`; create two groups.
        * `info = await r.xinfo_stream("s")`.
        * Assert `info["groups"] == 2`.

    - test_xinfo_stream_empty_stream:
        * `await r.xgroup_create("s", "g", id="0", mkstream=True)` — creates empty stream via mkstream.
        * `info = await r.xinfo_stream("s")`.
        * Assert `info["length"] == 0`, `info["first-entry"] is None`, `info["last-entry"] is None`, `info["groups"] == 1`.

    - test_xinfo_stream_missing_key_raises:
        * `with pytest.raises(redis.exceptions.ResponseError, match="no such key"): await r.xinfo_stream("nonexistent")`
        * Matches real Redis: `ERR no such key` for XINFO STREAM on missing key.

    - test_xinfo_stream_wrong_type_raises:
        * `await r.set("s", "value")`.
        * `with pytest.raises(redis.exceptions.ResponseError, match="WRONGTYPE"): await r.xinfo_stream("s")`.

    Run `uv run pytest tests/test_streams.py -k "xinfo_stream" -x -v` — expect all six to fail with AttributeError: 'BurnerRedis' object has no attribute 'xinfo_stream'. Commit as RED:
        `test(quick-260416-cea): add failing tests for xinfo_stream`
  </behavior>
  <action>
    Implementation (GREEN commit after RED).

    1. **Add `pub fn xinfo_stream(&self, key: &Bytes) -> Result<XInfoStreamSnapshot, StoreError>` to src/store.rs** (place near xinfo_groups around line 1965). Introduce a small struct for the snapshot so the Rust-to-Python conversion stays clean:

       ```rust
       // Near other stream structs (top of file or next to xinfo_groups):
       pub struct XInfoStreamSnapshot {
           pub length: usize,
           pub last_id: StreamId,
           pub groups_count: usize,
           pub first_entry: Option<(StreamId, HashMap<Bytes, Bytes>)>,
           pub last_entry: Option<(StreamId, HashMap<Bytes, Bytes>)>,
       }

       // New method:
       pub fn xinfo_stream(&self, key: &Bytes) -> Result<Option<XInfoStreamSnapshot>, StoreError> {
           let mut data = self.data.write();
           if let Some(entry) = data.get(key) {
               if entry.is_expired() {
                   data.remove(key);
                   return Ok(None);   // treat expired as missing
               }
           }
           let entry = match data.get(key) {
               None => return Ok(None),
               Some(e) => e,
           };
           let stream = match &entry.data {
               ValueData::Stream(s) => s,
               _ => return Err(StoreError::WrongType),
           };
           let first = stream.entries.iter().next()
               .map(|(id, fields)| (*id, fields.clone()));
           let last = stream.entries.iter().next_back()
               .map(|(id, fields)| (*id, fields.clone()));
           Ok(Some(XInfoStreamSnapshot {
               length: stream.entries.len(),
               last_id: stream.last_id,
               groups_count: stream.groups.len(),
               first_entry: first,
               last_entry: last,
           }))
       }
       ```
       Return shape `Result<Option<...>, StoreError>` distinguishes three cases: missing (Ok(None) -> ResponseError "no such key"), wrong type (Err(WrongType) -> existing WRONGTYPE response), present (Ok(Some(...)) -> populate dict).

    2. **Add `fn xinfo_stream` pymethod on BurnerRedis in src/lib.rs** (place near xinfo_groups around line 1376):

       ```rust
       /// XINFO STREAM command matching redis.asyncio.Redis.xinfo_stream() signature.
       /// Returns a dict with stream metadata using str keys (matches xinfo_groups convention).
       fn xinfo_stream<'py>(
           &self,
           py: Python<'py>,
           name: &Bound<'py, PyAny>,
       ) -> PyResult<Bound<'py, PyAny>> {
           let key = extract_bytes(name)?;
           let snapshot_opt = self.store.xinfo_stream(&key).map_err(store_err_to_py)?;
           let snapshot = match snapshot_opt {
               Some(s) => s,
               None => return Err(make_response_error(
                   format!("ERR no such key '{}'", String::from_utf8_lossy(&key))
               )),
           };

           let dict = PyDict::new(py);
           dict.set_item("length", snapshot.length as i64)?;
           dict.set_item("radix-tree-keys", snapshot.length as i64)?;    // stubbed: we don't use rax; expose length
           dict.set_item("radix-tree-nodes", (snapshot.length as i64) + 1)?;  // stubbed plausible
           dict.set_item(
               "last-generated-id",
               PyBytes::new(py, format_stream_id(snapshot.last_id).as_bytes()),
           )?;
           dict.set_item("groups", snapshot.groups_count as i64)?;

           // first-entry / last-entry as (id_bytes, {field_bytes: value_bytes}) or None
           let fmt_entry = |entry: &(StreamId, HashMap<Bytes, Bytes>)| -> PyResult<Py<PyAny>> {
               let (id, fields) = entry;
               let id_bytes = PyBytes::new(py, format_stream_id(*id).as_bytes());
               let field_dict = PyDict::new(py);
               for (fk, fv) in fields {
                   field_dict.set_item(
                       PyBytes::new(py, fk.as_ref()),
                       PyBytes::new(py, fv.as_ref()),
                   )?;
               }
               let tuple = PyTuple::new(py, &[id_bytes.into_any(), field_dict.into_any()])?;
               Ok(tuple.into_any().unbind())
           };
           match &snapshot.first_entry {
               Some(e) => dict.set_item("first-entry", fmt_entry(e)?)?,
               None => dict.set_item("first-entry", py.None())?,
           }
           match &snapshot.last_entry {
               Some(e) => dict.set_item("last-entry", fmt_entry(e)?)?,
               None => dict.set_item("last-entry", py.None())?,
           }

           resolved(py, dict.into_any().unbind())
       }
       ```

       Key decisions (with references):
         - **str keys, not bytes** — matches xinfo_groups convention (lib.rs:1388-1422). This is the "recently fixed from bytes to str keys" pattern called out in the task description.
         - **"radix-tree-keys" / "radix-tree-nodes" stubbed** — we don't use a radix tree; expose plausible integers so downstream code that reads these keys doesn't KeyError. `length` and `length+1` are fine placeholders.
         - **"ERR no such key"** phrasing matches real Redis; redis-py's exception `match="no such key"` will hit this. Do NOT add a new StoreError variant — keep the store layer generic and build the error message at the binding layer.

    3. **Expose xinfo_stream via the pipeline execute_pipeline dispatch** (lib.rs around line 2400 where xinfo_groups/xinfo_consumers branches live). Add an `"xinfo_stream"` arm mirroring the xinfo_groups pattern. This keeps `pipe.xinfo_stream(...).execute()` working if any downstream consumer pipelines XINFO STREAM. (If this feels speculative, skip it — redis-py's Pipeline rarely uses XINFO. Prefer to add it for API surface parity.)

    4. **No Python-side shim needed** — unlike `set`, `setex`, `pipeline`, `lock`, `pubsub`, `register_script` (python/burner_redis/__init__.py) which wrap Rust methods, `xinfo_stream` is a direct pymethod and should appear automatically on the BurnerRedis class. Verify with a REPL check in the verify step.

    5. **Rebuild and run tests:** `uv run maturin develop --release && uv run pytest tests/test_streams.py -k "xinfo_stream" -x -v`. All six new tests pass.

    Commit message:
        `fix(quick-260416-cea): add xinfo_stream method with str-keyed dict`
  </action>
  <verify>
    <automated>cd /Users/alexander/dev/prefectlabs/burner-redis && uv run maturin develop --release 2>&1 | tail -10 && uv run pytest tests/test_streams.py -k "xinfo_stream" -x -v</automated>
  </verify>
  <done>
    All six xinfo_stream tests pass. Returned dict uses str keys (not bytes), matches redis-py shape. Missing key raises redis.exceptions.ResponseError with "no such key". Wrong type raises WRONGTYPE. first-entry/last-entry are None on empty streams. No regression in existing xinfo_groups / xinfo_consumers tests.
  </done>
</task>

</tasks>

<verification>
After all three tasks:

1. **Full Python test suite:** `uv run pytest tests/ -x` — all tests pass.
2. **Stream-focused suite:** `uv run pytest tests/test_streams.py -x -v` — all existing + new tests pass (block, dollar, xinfo_stream).
3. **Rust unit tests:** `cargo test --lib` — all pass (verifies no regression in store.rs xinfo / stream methods).
4. **Build:** `uv run maturin develop --release` with no new warnings about unused params (the `#[allow(unused_variables)]` on `block` should be gone).
5. **Manual smoke test (reproduces all three original bug repros):**
   ```python
   import asyncio, time
   from burner_redis import BurnerRedis

   async def main():
       r = BurnerRedis()
       # Gap 1: xread(block=N) blocks and wakes up
       last = await r.xadd("s", {"init": "1"})
       async def reader():
           s = time.time()
           res = await r.xread({"s": last}, count=10, block=500)
           print(f"gap1 elapsed={time.time()-s:.3f}s got={bool(res)}")
       async def adder():
           await asyncio.sleep(0.1)
           await r.xadd("s", {"new": "1"})
       await asyncio.gather(reader(), adder())

       # Gap 2: $ as stream ID
       await r.xadd("t", {"initial": "1"})
       async def r2():
           return await r.xread({"t": "$"}, count=10, block=1000)
       async def a2():
           await asyncio.sleep(0.05)
           await r.xadd("t", {"after-dollar": "1"})
       results = await asyncio.gather(r2(), a2())
       print(f"gap2 result={results[0]}")  # should contain only after-dollar

       # Gap 3: xinfo_stream
       info = await r.xinfo_stream("t")
       print(f"gap3 info={info}")

   asyncio.run(main())
   ```

6. **Git history:** Should show 6 commits (RED + GREEN per gap) following the recent convention:
   ```
   fix(quick-260416-cea): add xinfo_stream method with str-keyed dict
   test(quick-260416-cea): add failing tests for xinfo_stream
   fix(quick-260416-cea): accept '$' as stream ID in xread (resolved at call time)
   test(quick-260416-cea): add failing tests for xread '$' stream ID
   fix(quick-260416-cea): implement xread(block=N) with stream_notify wakeup
   test(quick-260416-cea): add failing tests for xread(block=N) blocking
   ```
   Followed optionally by a docs commit.
</verification>

<success_criteria>
- All three gaps closed with failing-test-first commits (test + fix per gap).
- xread(block=N) yields to asyncio loop; concurrent xadd wakes it in < 1s; block=0 blocks until data; block=None fast path unchanged.
- xread(block=N) shares stream_notify plumbing with xreadgroup(block=N) — either via extracted helper or via obvious parallel-and-commented code with a cross-reference.
- xread accepts '$' and b'$'; resolved to current stream.last_id at call time; missing stream resolves to (0,0); xreadgroup still rejects '$'.
- xinfo_stream(name) returns str-keyed dict with keys matching redis-py: length, radix-tree-keys, radix-tree-nodes, last-generated-id, groups, first-entry, last-entry.
- Missing key raises redis.exceptions.ResponseError containing "no such key"; wrong type raises WRONGTYPE.
- No public API regressions (all existing stream tests still pass).
- No new compiler warnings.
</success_criteria>

<output>
After completion, create `.planning/quick/260416-cea-fix-three-redis-py-stream-compat-gaps-xr/260416-cea-SUMMARY.md` documenting:

1. **Files changed and why** (src/store.rs: added stream_last_id and xinfo_stream; src/lib.rs: implemented xread blocking path, added '$' resolution, added xinfo_stream pymethod; tests/test_streams.py: added ~17 new tests across three sections).
2. **DRY decision for xread/xreadgroup blocking** — did the executor extract a shared helper, or keep them as parallel-and-commented? Brief rationale either way.
3. **List of new tests** (one line each) and what each pins.
4. **Commit chain** (6 commits expected: test/fix pairs per gap).
5. **Downstream impact** — one-line confirmation that pydocket / Prefect consumers can now drop any stream-API workarounds for these three behaviors.
</output>
