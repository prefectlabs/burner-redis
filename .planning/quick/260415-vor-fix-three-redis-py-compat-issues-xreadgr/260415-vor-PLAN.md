---
quick_id: 260415-vor
type: execute
wave: 1
depends_on: []
files_modified:
  - tests/test_streams.py
  - tests/test_pubsub.py
  - src/store.rs
  - src/lib.rs
autonomous: true
requirements: []

must_haves:
  truths:
    - "xreadgroup(block=N) on an empty stream awaits without blocking the asyncio event loop; a concurrent xadd from another coroutine wakes it within tens of milliseconds"
    - "xpending_range against a missing stream key OR missing consumer group raises redis.exceptions.ResponseError whose message starts with 'NOGROUP' and contains the canonical 'No such key ... or consumer group ... in XPENDING' phrasing"
    - "xack, xclaim, xautoclaim, xreadgroup against a missing stream key or missing consumer group raise redis.exceptions.ResponseError with NOGROUP and the appropriate command name (XACK/XCLAIM/XAUTOCLAIM/XREADGROUP)"
    - "xgroup_create with mkstream=False against a missing key raises redis.exceptions.ResponseError starting with 'ERR' (matches Redis 'requires the key to exist' phrasing)"
    - "PubSub.get_message(timeout=0.1) inside a task can be cancelled via task.cancel() and the task completes within 2 seconds on Python 3.10, 3.11, 3.12, 3.13, and 3.14"
  artifacts:
    - path: "tests/test_streams.py"
      provides: "Failing-then-passing tests for Issue 1 (xreadgroup async wakeup) and Issue 2 (NOGROUP errors across stream-group commands)"
      contains: "test_xreadgroup_block_yields_to_event_loop, test_xpending_range_nogroup_raises_response_error, test_xack_nogroup_raises_response_error, test_xclaim_nogroup_raises_response_error, test_xautoclaim_nogroup_raises_response_error, test_xreadgroup_nogroup_raises_response_error, test_xgroup_create_missing_key_raises_response_error"
    - path: "tests/test_pubsub.py"
      provides: "Regression test for Issue 3 (PubSub.get_message cancellation)"
      contains: "test_pubsub_get_message_task_cancellation"
    - path: "src/store.rs"
      provides: "Updated StoreError::NoGroup variants whose Display format matches Redis canonical error text"
      contains: "in XPENDING"
    - path: "src/lib.rs"
      provides: "Per-command NOGROUP wrapping so command name appears in the error string for XPENDING, XPENDING_RANGE, XACK, XCLAIM, XAUTOCLAIM, XREADGROUP"
      contains: "in XPENDING"
  key_links:
    - from: "src/lib.rs xreadgroup blocking path"
      to: "store.stream_notify().notified()"
      via: "tokio::select! with future_into_py — must yield to asyncio loop while waiting"
      pattern: "stream_notify\\(\\).*notified"
    - from: "src/lib.rs xpending_range / xack / xclaim / xautoclaim handlers"
      to: "redis.exceptions.ResponseError"
      via: "store_err_to_py -> make_response_error after per-command NOGROUP message rewrite"
      pattern: "NOGROUP.*in (XPENDING|XACK|XCLAIM|XAUTOCLAIM|XREADGROUP)"
    - from: "python/burner_redis/pubsub.py get_message timeout branch"
      to: "asyncio.wait + manual cancel"
      via: "no asyncio.wait_for so external task.cancel() propagates on Python 3.10/3.11"
      pattern: "asyncio\\.wait\\("
---

<objective>
Close three redis-py compatibility gaps so downstream consumers (pydocket, Prefect) can drop the `is_memory` workaround branches and rely on standard redis-py error handling.

Purpose: BurnerRedis is meant to be a drop-in replacement for `redis.asyncio.Redis`. Today, three behaviors deviate enough to force consumers to special-case `memory://` URLs. Fixing these here removes the special-casing forever.

Output:
1. xreadgroup (and any sibling block-aware paths) cooperate with the asyncio event loop; a concurrent xadd wakes a blocked reader promptly.
2. Stream-group commands raise `redis.exceptions.ResponseError("NOGROUP ... in <CMD>")` consistently (no TypeError leakage); xgroup_create without mkstream raises ResponseError("ERR ...").
3. Regression test pins the existing PubSub.get_message cancellation fix so it cannot regress on Python 3.10/3.11.
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
@python/burner_redis/pubsub.py
@tests/conftest.py
@tests/test_streams.py
@tests/test_pubsub.py

<interfaces>
<!-- Key existing contracts the executor needs. Do NOT explore further; use these. -->

From src/store.rs (current):
```rust
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("WRONGTYPE Operation against a key holding the wrong kind of value")]
    WrongType,
    #[error("NOGROUP No such consumer group '{0}' for key name '{1}'")]
    NoGroup(String, String),  // (group, key)
    #[error("BUSYGROUP Consumer Group name already exists")]
    BusyGroup,
    #[error("ERR The XGROUP subcommand requires the key to exist")]
    KeyNotFound,
}

// Notification primitive already wired through xadd / xclaim / xautoclaim:
pub fn stream_notify(&self) -> Arc<Notify>;  // returns Tokio Notify
// Notify points: src/store.rs:1194 (xadd), :2269 (xclaim), :2297 (xautoclaim)
```

From src/lib.rs (current):
```rust
fn store_err_to_py(e: StoreError) -> PyErr {
    make_response_error(e.to_string())
}

fn make_response_error(msg: String) -> PyErr {
    // Returns redis.exceptions.ResponseError if redis is importable, else PyException.
    // ALREADY WORKS — issue is in callers that bypass it (extract_bytes errors, pre-store TypeError).
}

// Blocking xreadgroup path (lib.rs:1108-1151) already uses pyo3_async_runtimes::tokio::future_into_py
// and tokio::select! { _ = notify.notified() => ..., _ = tokio::time::sleep(remaining) => ... }
// THE BRIDGE EXISTS — verify the regression test triggers it from a real asyncio loop.
```

From python/burner_redis/pubsub.py (already fixed, needs regression test):
```python
async def get_message(self, ignore_subscribe_messages=False, timeout=0.0):
    # Uses asyncio.wait({get_task}, timeout=timeout) NOT asyncio.wait_for.
    # cpython#86296: wait_for swallows external task.cancel() on 3.10/3.11.
```

Existing test fixture (tests/conftest.py):
```python
@pytest.fixture
def r():
    return BurnerRedis()  # session-fresh, no shared state
```

Existing tests for context (DO NOT DUPLICATE — extend):
- tests/test_streams.py:740 test_xpending_range_nogroup_error — uses `match="NOGROUP"`, only verifies substring. New test must verify exception class is `redis.exceptions.ResponseError` AND message contains canonical phrasing.
- tests/test_streams.py:774 test_xreadgroup_block_returns_new_entries — proves wakeup works in a single coroutine. New test must specifically prove the event loop is not starved (run a counter coroutine concurrently and assert it advances during the block).
</interfaces>
</context>

<tasks>

<task type="auto" tdd="true">
  <name>Task 1: Issue 2 — write failing tests for NOGROUP errors across stream-group commands, then make StoreError messages and per-command wrapping match redis-py canonical text</name>
  <files>tests/test_streams.py, src/store.rs, src/lib.rs</files>
  <behavior>
    Add these tests to tests/test_streams.py (in the existing XPENDING / consumer-group sections; place near related tests, not all at the bottom):

    - test_xpending_range_nogroup_raises_response_error:
        * Imports `redis.exceptions` at top of test if not already imported.
        * Calls `await r.xpending_range("nonexistent-stream", "nonexistent-group", "-", "+", 10)`.
        * Asserts `pytest.raises(redis.exceptions.ResponseError)` with `match=r"NOGROUP No such key 'nonexistent-stream' or consumer group 'nonexistent-group' in XPENDING"`.
        * Also asserts the second case: stream exists, group missing — same exception, message names the existing key.
        * Also asserts the consumer-filter variant: `await r.xpending_range("ns", "ng", "-", "+", 10, consumername="anyone")` against missing key/group raises the same NOGROUP ResponseError (NOT TypeError).

    - test_xpending_summary_nogroup_raises_response_error:
        * Same shape, calls `await r.xpending("nonexistent-stream", "nonexistent-group")`. Message must contain "in XPENDING".

    - test_xack_nogroup_raises_response_error:
        * `await r.xack("nostream", "nogroup", "0-0")` raises ResponseError with "NOGROUP" and "in XACK".
        * NOTE: Today xack currently returns 0 silently when group is missing (see store.rs:1642 — review and decide). If existing behavior is "return 0", document that real Redis returns 0 too for XACK on missing group/entries — and SKIP this assertion in the test (delete it) but leave a comment explaining why. Verify against `redis-py` behavior before asserting. Use `mlua-rs/mlua` is irrelevant; this is direct redis-py call shape.

    - test_xclaim_nogroup_raises_response_error:
        * `await r.xclaim("nostream", "nogroup", "consumer1", min_idle_time=0, message_ids=["0-0"])` raises ResponseError matching `r"NOGROUP No such key '.*' or consumer group '.*' in XCLAIM"`.

    - test_xautoclaim_nogroup_raises_response_error:
        * `await r.xautoclaim("nostream", "nogroup", "consumer1", min_idle_time=0)` raises ResponseError with "NOGROUP" and "in XAUTOCLAIM".

    - test_xreadgroup_nogroup_raises_response_error:
        * `await r.xreadgroup("nogroup", "c1", {"nostream": ">"})` raises ResponseError with "NOGROUP" and "in XREADGROUP".

    - test_xgroup_create_missing_key_raises_response_error:
        * `await r.xgroup_create("nostream", "g", id="0", mkstream=False)` raises ResponseError matching `r"ERR.*requires the key to exist"`.

    Run the test suite (`uv run pytest tests/test_streams.py -k "nogroup or missing_key" -x`) and CONFIRM these tests fail BEFORE implementation. Capture the actual failure modes (TypeError? wrong message? no exception?) to confirm what needs fixing.
  </behavior>
  <action>
    Implementation steps after the tests are red:

    1. **Update `StoreError::NoGroup` in src/store.rs (~line 178-187):**
       Change variant to carry an optional command name OR change the Display format to match Redis canonical text. Recommended approach (minimal blast radius — store layer stays command-agnostic):
       - Keep `StoreError::NoGroup(String, String)` with semantics `(group, key)`.
       - Change the `#[error(...)]` attribute to: `"NOGROUP No such key '{1}' or consumer group '{0}'"` (key first in canonical phrasing, group second; note arg order swap from current code so verify call sites still pass `(group, key)`).
       - The per-command suffix " in XPENDING" / " in XACK" etc. is added by lib.rs at the call site (see step 2). Do NOT bake the command name into the store layer.
       - Update Phase 5/store.rs tests that match on the old text (search `"NOGROUP No such consumer group"` and adjust — should be 1-3 places).

    2. **Add a per-command NOGROUP-rewrite helper in src/lib.rs (near `store_err_to_py`):**
       ```rust
       fn store_err_to_py_for_cmd(e: StoreError, cmd: &str) -> PyErr {
           let msg = match &e {
               StoreError::NoGroup(_, _) => format!("{} in {}", e, cmd),
               StoreError::KeyNotFound => format!("ERR The XGROUP subcommand requires the key to exist"),
               _ => e.to_string(),
           };
           make_response_error(msg)
       }
       ```
       (Keep the original `store_err_to_py` for non-stream-group call sites; non-NOGROUP errors fall through unchanged.)

    3. **Update each stream-group command in src/lib.rs to use the new helper with its command name:**
       - `xreadgroup` (sync + blocking paths + execute_pipeline branch ~line 2260-2276) — pass `"XREADGROUP"`.
       - `xack` (~line 1184 + pipeline ~line 2289) — pass `"XACK"`. (If xack-on-missing-group returns 0, no error is raised; the helper still applies for the actual NOGROUP case in the store, e.g. `xack` with `WrongType`.)
       - `xclaim` (~line 1305 + pipeline ~line 2345) — pass `"XCLAIM"`.
       - `xautoclaim` (~line 1219 + pipeline ~line 2305) — pass `"XAUTOCLAIM"`.
       - `xpending_range` (~line 1494 + pipeline ~line 2428) — pass `"XPENDING"` (real Redis uses XPENDING for both forms).
       - `xpending` summary (~line 1814 + pipeline ~line 2528) — pass `"XPENDING"`.
       - `xgroup_create` (~line 1045 + pipeline ~line 2248) — pass `"XGROUP CREATE"` (the helper passes through KeyNotFound's "ERR ..." message verbatim).
       - `xgroup_destroy` — XGROUP DESTROY does not raise NOGROUP in real Redis (returns 0 for missing group), so existing behavior stays.
       - `xinfo_consumers` / `xinfo_groups` (search lib.rs for `xinfo_`) — apply same wrapping for completeness; pass `"XINFO CONSUMERS"` / `"XINFO GROUPS"`.

    4. **Audit `xpending_range` Python signature path in lib.rs (~line 1442-1494) for TypeError sources:**
       - When `consumername` is None or a missing consumer name, current code returns empty list rather than NOGROUP. That's intentional for *existing* group + missing consumer (real Redis returns empty), but the bug is the *missing key/group* path. Verify the store reaches NoGroup before consumer filtering — it does (see store.rs:2098-2121), so wrapping at the lib.rs map_err is sufficient.
       - If the `consumername` param is something unusual (e.g., empty bytes), the `extract_bytes` call may currently raise TypeError. Add a defensive `.map_err(|e| make_response_error(format!("ERR invalid consumer name: {e}")))` only if a real test demonstrates the TypeError leak; otherwise leave alone.

    5. **Verify existing test `test_xpending_range_nogroup_error` (line 740) still passes** — it uses `match="NOGROUP"` substring, which the new canonical message still contains.
  </action>
  <verify>
    <automated>cd /Users/alexander/dev/prefectlabs/burner-redis &amp;&amp; uv run maturin develop --release 2>&amp;1 | tail -20 &amp;&amp; uv run pytest tests/test_streams.py -k "nogroup or missing_key or xgroup_create" -x -v</automated>
  </verify>
  <done>
    All new NOGROUP / missing-key tests in tests/test_streams.py pass. Existing test_xpending_range_nogroup_error still passes. cargo test in src/store.rs still passes (run `cargo test --lib`). No TypeError leaks from xpending_range, xpending, xack, xclaim, xautoclaim, xreadgroup against missing key/group — all surface as `redis.exceptions.ResponseError` whose message contains `NOGROUP No such key '...' or consumer group '...' in <CMD>`.
  </done>
</task>

<task type="auto" tdd="true">
  <name>Task 2: Issue 1 — write failing test proving xreadgroup(block=N) yields to the asyncio loop and is woken by a concurrent xadd; verify the existing Tokio-bridged path satisfies it (and harden if not)</name>
  <files>tests/test_streams.py, src/lib.rs</files>
  <behavior>
    Add these tests to tests/test_streams.py in the "XREADGROUP Blocking" section (after line 833):

    - test_xreadgroup_block_yields_to_event_loop:
        * Setup: xadd one entry, xgroup_create, xreadgroup once to drain.
        * Spawn a "tick counter" coroutine that loops `await asyncio.sleep(0.005); counter += 1` for ~200ms then exits.
        * Spawn an "xadd-after-50ms" coroutine: `await asyncio.sleep(0.05); await r.xadd("mystream", {"f": "v2"})`.
        * Call `await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"}, block=5000)` — it MUST return BEFORE the 5000ms timeout (assert wall-time < 1.0s with generous slack).
        * Await both helper tasks.
        * Assert the tick counter advanced at least 5 times during the block (proves the event loop kept scheduling other coroutines while xreadgroup was waiting). Without proper async yielding, counter would be 0 or 1.
        * Assert xreadgroup returned the v2 entry.

    - test_xreadgroup_block_concurrent_consumers_event_loop_progress:
        * Two consumers in the same group both call `xreadgroup(..., block=2000)` concurrently via `asyncio.gather`.
        * A third coroutine does `asyncio.sleep(0.05); xadd; asyncio.sleep(0.05); xadd`.
        * Both consumers should return promptly (each gets one of the two messages OR one gets both — depends on group semantics; just assert wall-time < 1.0s and at least one consumer received an entry).
        * This pins behavior under contention and proves notify_waiters wakes all waiters.

    Run `uv run pytest tests/test_streams.py -k "yields_to_event_loop or concurrent_consumers" -x -v` and CONFIRM the tests show the actual current behavior. There are two possible outcomes:
      (a) Tests pass — the existing pyo3_async_runtimes bridge already yields correctly. In that case, the downstream workaround was defensive against an old bug; document this in the SUMMARY and skip the implementation step.
      (b) Tests fail — the bridge is not yielding (block becomes synchronous or notify_waiters fires too late). Proceed to action step.
  </behavior>
  <action>
    If the tests in <behavior> already pass with the existing implementation (most likely outcome — `pyo3_async_runtimes::tokio::future_into_py` + `tokio::select!` is the correct pattern and `stream_notify` is wired to xadd at store.rs:1194), commit just the new tests as regression coverage and STOP. Document in the commit message that the tests pass against the existing implementation and the downstream workaround can be removed.

    If they fail, the failure mode dictates the fix. Most likely culprits in priority order:

    1. **GIL-held loop blocking the asyncio thread:** The xreadgroup blocking path (lib.rs:1108-1151) must NOT call `Python::try_attach` inside the `tokio::select!` arms before returning. Verify `format_xreadgroup_result` (lib.rs:148-169) only acquires the GIL once at completion. If a `Python::try_attach` is held during the wait, switch to acquiring the GIL only in the final `format_xreadgroup_result` call (already the case — verify).

    2. **Multi-thread vs current-thread runtime mismatch:** PROJECT decision (Phase 01) switched to multi-thread runtime for `future_into_py` compatibility. Confirm `Cargo.toml` and runtime init in lib.rs still use multi-thread. If this regressed, restore.

    3. **stream_notify scope:** Verify `notify.notified()` is created BEFORE the second `xreadgroup` poll, so a notification that fires between the first poll and `.notified()` await isn't lost. Current code creates `notify` after the first poll (lib.rs:1125) — change to create the notify BEFORE the first poll to close the race window:
       ```rust
       let notify = store.stream_notify();
       let waiter = notify.notified();  // register interest BEFORE first read
       tokio::pin!(waiter);
       let results = store.xreadgroup(...).map_err(store_err_to_py)?;
       if !results.is_empty() { return format_xreadgroup_result(results); }
       // now waiter is already armed
       ```
       This is a real correctness improvement regardless of the test outcome — apply it even if tests pass.

    4. **Other `block=N` siblings:** `xread(..., block=N)`, `blpop`, `brpop`, `bzpopmin`, `bzpopmax`. Search lib.rs for `fn xread` / `blpop` / `brpop` / `bzpopmin` / `bzpopmax`. If any exist with a `block` parameter and synchronous polling, refactor to the same `future_into_py` + `select!` + `stream_notify` (or a dedicated `list_notify` / `zset_notify` Notify) pattern. If they don't exist yet, document in the SUMMARY that only xreadgroup needed fixing (which matches the user's task — "xread is not used in docket workaround"). Do NOT add new commands as part of this quick task; just confirm coverage.

    Keep changes minimal and surgical. The goal is "tests pass" + "no race window in notify registration".
  </action>
  <verify>
    <automated>cd /Users/alexander/dev/prefectlabs/burner-redis &amp;&amp; uv run maturin develop --release 2>&amp;1 | tail -10 &amp;&amp; uv run pytest tests/test_streams.py -k "yields_to_event_loop or concurrent_consumers or block" -x -v</automated>
  </verify>
  <done>
    Both new tests pass. Existing block tests (test_xreadgroup_block_returns_new_entries, test_xreadgroup_block_timeout_returns_empty, test_xreadgroup_block_lua_xadd_wakes_reader) still pass. Wall-clock duration of the new yield-test is &lt; 1.0s (proves no 5000ms synchronous block). Tick counter ≥ 5 (proves event loop ran other coroutines during the block). If implementation changes were made, they are limited to lib.rs xreadgroup blocking path and any new helper for notify registration; no public API changes.
  </done>
</task>

<task type="auto" tdd="true">
  <name>Task 3: Issue 3 — add Python-only regression test pinning the PubSub.get_message cancellation fix across Python 3.10–3.14</name>
  <files>tests/test_pubsub.py</files>
  <behavior>
    Add `test_pubsub_get_message_task_cancellation` to tests/test_pubsub.py (place near other get_message tests; if none exist, add at end of file before any helper class definitions).

    Test shape:
    ```python
    async def test_pubsub_get_message_task_cancellation(r):
        """Regression: external task.cancel() must propagate through PubSub.get_message
        on Python 3.10/3.11 (cpython#86296). Pinned by the asyncio.wait-based
        implementation in pubsub.py.
        """
        ps = r.pubsub(ignore_subscribe_messages=True)
        await ps.subscribe("regression-channel")

        async def poll_forever():
            while True:
                msg = await ps.get_message(timeout=0.1)
                if msg is not None:
                    return msg

        task = asyncio.create_task(poll_forever())

        # Let the poller spin a few iterations to be mid-await.
        await asyncio.sleep(0.5)

        task.cancel()

        # Must finish within 2.0s. Without the fix, this hangs forever on 3.10/3.11.
        with pytest.raises(asyncio.CancelledError):
            await asyncio.wait_for(task, timeout=2.0)

        await ps.aclose()
    ```

    Variant — also test the pattern-subscribe path:
    ```python
    async def test_pubsub_get_message_task_cancellation_pattern(r):
        ps = r.pubsub(ignore_subscribe_messages=True)
        await ps.psubscribe("regression-*")
        # ... same shape with psubscribe
    ```

    Run `uv run pytest tests/test_pubsub.py -k "task_cancellation" -x -v` and confirm the tests pass against the existing (already-fixed) pubsub.py implementation. The tests should NOT require any change to pubsub.py — they exist solely to prevent regression.

    Also verify on at least one alternative Python version locally if available (e.g., `uv run --python 3.10 pytest tests/test_pubsub.py -k "task_cancellation" -x` and `uv run --python 3.11 pytest ...`). If the project's CI matrix already covers 3.10–3.14 (it does — see commit 21ab7f5 "add Python version matrix"), the CI run is sufficient verification across versions.
  </behavior>
  <action>
    Pure test addition. No source changes. Do NOT modify pubsub.py — the fix already exists (asyncio.wait, lib.rs:N/A, see python/burner_redis/pubsub.py:171-208).

    Implementation notes for the test author:
    - Use the existing `r` fixture from tests/conftest.py.
    - Import `asyncio` and `pytest` at top of file (already imported).
    - The test must use `pytest.raises(asyncio.CancelledError)` AROUND the `await asyncio.wait_for(task, timeout=2.0)` — when the task is cancelled, awaiting it re-raises CancelledError; that is the success signal.
    - Do NOT use `asyncio.wait_for(task, timeout=2.0)` to suppress the CancelledError — the assertion that the wait_for itself does NOT raise TimeoutError is the regression check. Phrase it carefully:
      ```python
      try:
          await asyncio.wait_for(task, timeout=2.0)
      except asyncio.CancelledError:
          pass  # expected
      except asyncio.TimeoutError:
          pytest.fail("PubSub.get_message swallowed task.cancel() — regression of cpython#86296 fix")
      ```
      This phrasing is more diagnostic than `pytest.raises`. Use whichever is clearer; both are acceptable.
    - The `await ps.aclose()` at the end is best-effort cleanup; wrap in try/except if the cancelled task left state in a weird spot.
  </action>
  <verify>
    <automated>cd /Users/alexander/dev/prefectlabs/burner-redis &amp;&amp; uv run pytest tests/test_pubsub.py -k "task_cancellation" -x -v</automated>
  </verify>
  <done>
    Both new pubsub regression tests pass against the current Python interpreter. No changes to python/burner_redis/pubsub.py. The tests will run in CI across Python 3.10, 3.11, 3.12, 3.13, 3.14 via the existing matrix and provide regression coverage if anyone reverts the asyncio.wait change.
  </done>
</task>

</tasks>

<verification>
After all three tasks:

1. Full Python test suite: `uv run pytest tests/ -x` — all tests pass.
2. Rust unit tests: `cargo test --lib` — all tests pass (verifies StoreError message change didn't break store-layer assertions).
3. Targeted compat verification: `uv run pytest tests/test_streams.py tests/test_pubsub.py -v` — all new tests visible and green.
4. Build: `uv run maturin develop --release` succeeds with no warnings about unused imports/variables introduced by the changes.
5. Sanity check downstream pattern (manual, optional): in a Python REPL, run:
   ```python
   import asyncio, redis.exceptions
   from burner_redis import BurnerRedis
   r = BurnerRedis()
   async def main():
       try:
           await r.xpending_range("nope", "nope", "-", "+", 10)
       except redis.exceptions.ResponseError as e:
           assert "NOGROUP" in str(e) and "in XPENDING" in str(e), str(e)
       print("OK")
   asyncio.run(main())
   ```
</verification>

<success_criteria>
- Three new test groups exist in tests/test_streams.py and tests/test_pubsub.py and all pass.
- All NOGROUP-bearing stream-group commands raise `redis.exceptions.ResponseError` whose message matches Redis canonical phrasing including the per-command suffix (e.g. "in XPENDING").
- xreadgroup(block=N) cooperates with the asyncio event loop — a tick-counter coroutine measurably advances during the block, and a concurrent xadd wakes the reader in well under 1 second.
- PubSub.get_message cancellation regression test pins the existing fix for cpython#86296 across the supported Python matrix.
- Downstream code can drop the `is_memory = url.startswith("memory://")` workaround branches with no behavior change.
- No public API changes (signature-compatible with redis.asyncio.Redis).
- No regressions: existing test_xpending_range_nogroup_error, test_xreadgroup_block_*, and all other stream/pubsub tests still pass.
</success_criteria>

<output>
After completion, create `.planning/quick/260415-vor-fix-three-redis-py-compat-issues-xreadgr/260415-vor-SUMMARY.md` documenting:

1. What was changed in src/store.rs (StoreError::NoGroup message format) and src/lib.rs (per-command NOGROUP wrapping helper, list of touched call sites).
2. Whether Issue 1 required actual code changes or whether the new test confirmed the existing bridge already yields correctly. If code changed, what was the race and how was it closed.
3. The full list of new tests added and a one-line statement of what each one pins.
4. Confirmation that downstream pydocket can now remove the `is_memory` workaround — and a one-line snippet showing the simplified call shape.
</output>
