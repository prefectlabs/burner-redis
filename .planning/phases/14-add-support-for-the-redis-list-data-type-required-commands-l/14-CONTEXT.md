# Phase 14: List data type (LPUSH, BRPOP, BLPOP, and full list command set) - Context

**Gathered:** 2026-04-24
**Status:** Ready for planning

<domain>
## Phase Boundary

Add Redis list data type support to burner-redis with the full 16-command surface: **LPUSH, RPUSH, LPOP, RPOP, LRANGE, LLEN, LINDEX, LINSERT, LREM, LSET, LTRIM, LMOVE, RPOPLPUSH, BRPOP, BLPOP, BLMOVE**. Blocking commands (BRPOP, BLPOP, BLMOVE) integrate with the existing Tokio runtime using the `tokio::sync::Notify` pattern established in Phase 11 for XREAD/XREADGROUP blocking, and respect Python asyncio cancellation and timeout semantics. Storage is `VecDeque<Bytes>` behind the existing keyspace `parking_lot::RwLock`.

**Scope reversal flagged:** `.planning/REQUIREMENTS.md` currently lists "Blocking list commands (BLPOP/BRPOP)" under Out of Scope with rationale "Prefect uses Streams, not blocking lists". This phase consciously reverses that decision. REQUIREMENTS.md must be updated to (a) remove BLPOP/BRPOP from Out of Scope, and (b) add a `LIST-*` requirements section mapped to Phase 14. BRPOPLPUSH (blocking legacy variant) stays out of scope — not in the ROADMAP stretch list and superseded by BLMOVE in redis-py.

</domain>

<decisions>
## Implementation Decisions

### Command Surface
- **D-01:** Full 16-command coverage in this phase — LPUSH, RPUSH, LPOP, RPOP, LRANGE, LLEN, LINDEX, LINSERT, LREM, LSET, LTRIM, LMOVE, RPOPLPUSH, BRPOP, BLPOP, BLMOVE. No BRPOPLPUSH (not in ROADMAP.md stretch list, superseded by BLMOVE).
- **D-02:** LPOP/RPOP match redis-py exactly — `count=None` returns single `bytes` (or `None`), `count=N` returns `list[bytes]` of up to N popped elements (or `None` if key missing). Full drop-in parity, no gaps.
- **D-03:** Empty list after final pop deletes the key — Redis standard behavior.

### Storage
- **D-04:** Add `ValueData::List(VecDeque<Bytes>)` variant to the existing enum in `src/store.rs:118`, following the Phase 2/5 ValueData expansion pattern. Mutations go through the same `parking_lot::RwLock` write-lock-for-all-mutations discipline.
- **D-05:** New `src/commands/lists.rs` module for command-specific helpers (count parsing, LRANGE negative-index normalization, LREM count-sign handling, LINSERT pivot lookup, etc.), mirroring `src/commands/streams.rs`.

### Blocking Architecture
- **D-06:** New dedicated `list_notify: Arc<Notify>` field on `Store`, parallel to the existing `stream_notify` (`src/store.rs:275`). Clean separation from streams; no cross-wake noise.
- **D-07:** BRPOP/BLPOP blocking loop mirrors the XREAD blocking loop in `src/lib.rs:980-1038` — first non-blocking attempt, then `notify.notified()` + `tokio::time::sleep(remaining)` inside `tokio::select!` with a deadline derived from `block_ms`. Re-arm `waiter.set(notify.notified()); waiter.as_mut().enable();` on each wake. BLMOVE uses the same skeleton but operates on source + destination.
- **D-08:** Graceful shutdown via `store.is_shutdown()` check at the top of each loop iteration — returns empty result so the Rust future completes via `call_soon_threadsafe` before the Python event loop tears down.
- **D-09:** Multi-key BRPOP/BLPOP — on each wake (and on first attempt), scan the keys list in order and pop from the first non-empty list. Return `(key, value)` tuple matching redis-py. If all are still empty, re-arm notify and loop until deadline. This is the only behavior that passes redis-py/Redis spec.
- **D-10:** `list_notify.notify_waiters()` fires inside the write lock at the Store-method level, matching the existing XADD pattern at `src/store.rs:1262` and `2402`. Wake sites: LPUSH, RPUSH, LMOVE (destination side), RPOPLPUSH (destination side), and BLMOVE destination write.
- **D-11:** Timeout accepts `float` seconds at the Python layer (matching redis-py), converts to `Option<u64>` milliseconds passed to Rust. `0` → block forever (long-slice sleep, no deadline; loop re-arms on wake). Positive → deadline `tokio::time::Instant::now() + Duration::from_millis(block_ms)`. Exact mirror of XREAD `block=0` handling.

### Lua Integration
- **D-12:** All **non-blocking** list commands added to `dispatch_command_inner` in `src/scripting.rs` — LPUSH, RPUSH, LPOP, RPOP, LRANGE, LLEN, LINDEX, LINSERT, LREM, LSET, LTRIM, LMOVE, RPOPLPUSH. Return-type conversions follow the existing RedisValue pattern.
- **D-13:** BRPOP / BLPOP / BLMOVE called from Lua return a `RedisValue::Error` matching real Redis: `"ERR This Redis command is not allowed from scripts: <cmd>"`. Scripts are atomic in Redis; blocking would deadlock. Preserves the compat contract for anyone porting Lua scripts from a real Redis deployment.
- **D-14:** Lua list mutations fire `list_notify.notify_waiters()` after script execution — follows Phase 11's XADD-from-Lua fix pattern. Extend `dispatch_command()`'s return tuple: add a `had_list_mutation` flag alongside the existing `had_xadd` flag. After Lua execution, if the flag is set, call `list_notify.notify_waiters()`. Prevents the class-of-bug where BRPOP consumers silently miss LPUSH emitted from `redis.call()` inside a script.

### Pipeline Integration
- **D-15:** BRPOP/BLPOP/BLMOVE inside a pipeline respect their per-command timeouts. Pipelines are sequential in-process loops; blocking one command simply delays subsequent commands. Matches redis-py semantics: redis-py pipelines are batched commands, not a true atomic unit against a server; blocking commands really do block.
- **D-16:** `execute_pipeline()` in `src/lib.rs:2182` detects blocking commands in the queue. **No blocking commands present →** keep the existing synchronous `dispatch_pipeline_command()` fast path (preserves the async-overhead elimination from quick task `260415-an2-eliminate-async-overhead-with-sync-fast-`). **Blocking commands present →** fall through to a per-command async loop that invokes the normal `BurnerRedis.brpop()` / `blpop()` / `blmove()` awaitables. Non-blocking pipelines keep their speed; blocking pipelines pay the async cost where it's unavoidable.
- **D-17:** Every new list command gets a pipeline stub method in `python/burner_redis/pipeline.py` buffering `(method_name, args, kwargs)`, following the established pattern.

### Python Surface & Compatibility
- **D-18:** All 16 command methods are async and match `redis.asyncio.Redis` signatures exactly (drop-in discipline from Phase 1). Method order: positional args then keyword args matching redis-py.
- **D-19:** Value coercion for pushed values (int, float, bool, memoryview, other → `str(value).encode()`) uses the existing `_coerce_value` helper from `python/burner_redis/__init__.py` (Phase 12). Applied to LPUSH, RPUSH, LSET, LINSERT, and the destination-write of LMOVE / RPOPLPUSH / BLMOVE.
- **D-20:** WRONGTYPE errors against non-list keys use the existing `StoreError::WrongType` → `ResponseError` conversion (`store_err_to_py` in `src/lib.rs`).
- **D-21:** REQUIREMENTS.md update is part of this phase's deliverable — remove "Blocking list commands (BLPOP/BRPOP)" from Out of Scope, add a `LIST-*` requirements section (LIST-01 through ~LIST-16), map to Phase 14 in the Traceability table.

### Testing
- **D-22:** New `tests/test_lists.py` for pytest integration coverage of all 16 commands, WRONGTYPE, LPOP count semantics, BRPOP/BLPOP blocking (asyncio cancellation, timeout=0 indefinite, timeout mid-block wake, multi-key order-scan, Lua-to-BRPOP wake-up path), LMOVE cross-key edge cases, and pipeline execution mixing blocking + non-blocking commands.

### Claude's Discretion
- Exact helper boundary between `src/store.rs` (store methods) and `src/commands/lists.rs` (parsing, index normalization, pattern helpers).
- LRANGE negative-index normalization logic (follow Redis documented behavior: negative indices offset from tail, out-of-range clamps to empty).
- LINSERT pivot-not-found return code (`-1` per Redis spec).
- LREM count-sign semantics (positive = head-to-tail, negative = tail-to-head, 0 = all occurrences).
- LPOP `count=0` precise return (empty list vs `None` — follow redis-py's actual behavior).
- Internal organization of the 16 new `#[pymethods]` in `src/lib.rs` (grouping/ordering).
- Whether to split the phase into 2 plans (engine + Python surface, echoing Phase 2/3/5) or 3 plans (engine / Python / Lua+pipeline). Planner will decide based on plan-length heuristics.

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Phase Scope & Requirements
- `.planning/ROADMAP.md` §Phase 14 — Authoritative command list and the stated storage/blocking constraints
- `.planning/REQUIREMENTS.md` §"Out of Scope" — Lists BLPOP/BRPOP as out of scope; this phase reverses that (plan must update this file)
- `.planning/PROJECT.md` — Drop-in redis.asyncio.Redis compatibility constraint

### Prior Phase Contexts (pattern origins)
- `.planning/phases/05-stream-commands-and-consumer-groups/05-CONTEXT.md` — Stream blocking model, ValueData enum expansion pattern
- `.planning/phases/06-lua-scripting/06-CONTEXT.md` — Lua dispatch architecture, `dispatch_command_inner` pattern, deadlock-prevention lock ordering
- `.planning/phases/07-pipeline-and-locking/07-CONTEXT.md` — Pipeline class pattern, async context manager
- `.planning/phases/11-close-redis-py-compatibility-gaps-for-pydocket-integration/11-CONTEXT.md` — XREADGROUP blocking fix, `tokio::sync::Notify` + `tokio::select!` deadline pattern, Lua-to-Store wake-up race fix (directly analogous to this phase's Lua-to-BRPOP wake)
- `.planning/phases/12-close-remaining-redis-py-compatibility-gaps-for-drop-in-repl/12-CONTEXT.md` — Value coercion at Python layer, exception hierarchy, pipeline stub pattern

### Codebase Integration Points
- `src/store.rs:118` — `ValueData` enum (add `List(VecDeque<Bytes>)` variant)
- `src/store.rs:275-302` — `stream_notify: Arc<Notify>` pattern (template for new `list_notify`)
- `src/store.rs:1262, 2402` — XADD `notify_waiters()` call sites (template for LPUSH/RPUSH/LMOVE notify)
- `src/lib.rs:915-1039` — XREAD blocking loop (direct template for BRPOP/BLPOP blocking)
- `src/lib.rs:1229-1337` — XREADGROUP blocking loop (template for BLMOVE)
- `src/lib.rs:2182-2197` — `execute_pipeline()` (needs blocking-aware branch)
- `src/lib.rs:2200+` — `dispatch_pipeline_command()` (extend with non-blocking list commands)
- `src/scripting.rs:268-279` — `dispatch_command()` with `had_xadd` flag (extend with `had_list_mutation`)
- `src/scripting.rs:282+` — `dispatch_command_inner()` (add 13 non-blocking list command cases + 3 blocking-reject cases)
- `src/commands/streams.rs` — Module structure template for new `src/commands/lists.rs`
- `python/burner_redis/__init__.py` — `_coerce_value` helper, `ResponseError` pattern, method monkey-patch pattern
- `python/burner_redis/pipeline.py` — Pipeline stub pattern (add 16 new list command stubs)

### Quick Task References (must not regress)
- `.planning/quick/260415-an2-eliminate-async-overhead-with-sync-fast-/` — Sync fast path for `execute_pipeline()`; keep intact when no blocking commands are in the queue

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `stream_notify: Arc<Notify>` at `src/store.rs:275` — direct template for `list_notify` (field, constructor line 285, accessor at 291, shutdown notify at 302)
- XREAD blocking loop at `src/lib.rs:980-1038` — direct template for BRPOP/BLPOP (first non-blocking poll, `tokio::select!` with notify + deadline sleep, `is_shutdown()` check, block=0 long-slice handling)
- `dispatch_command` / `dispatch_command_inner` in `src/scripting.rs` — extend with list commands; `had_xadd` → `had_list_mutation` flag extension is a direct pattern replay
- `_coerce_value` helper in `python/burner_redis/__init__.py` — apply unchanged to value-accepting list commands
- `StoreError::WrongType` + `store_err_to_py` conversion — used unchanged for non-list key type errors
- `is_shutdown()` check — used unchanged in every blocking loop
- `extract_bytes()` in `src/commands/strings.rs` — used unchanged for key/value extraction
- Pipeline stub pattern in `python/burner_redis/pipeline.py` — 16 new stubs follow the existing signature-buffering format

### Established Patterns
- One `Store` method per command, returning `Result<T, StoreError>`
- PyO3 `#[pymethods]` async binding via `pyo3_async_runtimes::tokio::future_into_py`
- `Python::try_attach` for GIL re-attach in async blocks (PyO3 0.28.3 convention)
- One pytest file per command group (`tests/test_lists.py`)
- Lua dispatch entry per non-blocking command; blocking commands return `RedisValue::Error` matching real Redis wording
- Pipeline stub per command, synchronous buffer-only
- Notify waiters fire inside the write lock at Store method level

### Integration Points
- `ValueData` enum needs `List(VecDeque<Bytes>)` variant (`src/store.rs:118`)
- `Store` gains `list_notify: Arc<Notify>` field next to `stream_notify` (`src/store.rs:275`)
- `Store::new()` initializes `list_notify` alongside `stream_notify` (~line 285)
- `Store::shutdown()` calls `list_notify.notify_waiters()` alongside `stream_notify` (~line 302)
- New `src/commands/lists.rs` module, registered in `src/commands/mod.rs`
- `src/lib.rs` gains ~16 new `#[pymethods]`
- `src/lib.rs::execute_pipeline()` gains a blocking-aware branch; `dispatch_pipeline_command()` gains 13 non-blocking list command cases
- `src/scripting.rs::dispatch_command_inner()` gains 13 non-blocking cases + 3 blocking-reject cases
- `src/scripting.rs::dispatch_command()` tuple grows: `(RedisValue, had_xadd, had_list_mutation)`
- `python/burner_redis/pipeline.py` gains 16 new stub methods
- `python/burner_redis/__init__.py` value-coercion monkey-patches list push commands if needed (or applies coercion at Rust boundary like set() does)
- `.planning/REQUIREMENTS.md` updated: BLPOP/BRPOP removed from Out of Scope; new `LIST-*` section; Traceability table mapping to Phase 14
- New `tests/test_lists.py`

</code_context>

<specifics>
## Specific Ideas

- Redis-py compatibility is the governing discipline (established Phase 12). Every behavioral decision above is shaped by matching real redis-py exactly so drop-in replacement holds.
- Phase 11's Lua-to-Store wake-up race is the specific bug we're preemptively preventing in the list subsystem via the `had_list_mutation` flag and notify-after-script pattern.
- Quick task `260415-an2` eliminated async overhead in pipelines via a sync fast path — that win must not regress. The blocking-aware branch in `execute_pipeline()` is specifically scoped so that non-blocking pipelines (the common case) stay on the fast path.
- BLMOVE is the only command that is both blocking AND multi-key-write; plan should treat it as its own concern (cross-key atomicity under the write lock, destination notify firing correctly, asyncio cancellation cleanup).

</specifics>

<deferred>
## Deferred Ideas

- **BRPOPLPUSH** (blocking legacy variant): not in ROADMAP.md stretch list, superseded by BLMOVE in redis-py. Add later only if a concrete caller needs it.
- **Per-key fine-grained notify** (`HashMap<Bytes, Arc<Notify>>`): over-engineered for single-process embedded use where waiter counts are expected to be small. Revisit only if wake-storm profiling shows the shared-notify model costs measurable CPU.
- **LPOS command**: not requested in this phase, not in ROADMAP.md. Future compatibility phase if pydocket or Prefect ever needs positional lookup.

</deferred>

---

*Phase: 14-add-support-for-the-redis-list-data-type-required-commands-l*
*Context gathered: 2026-04-24*
