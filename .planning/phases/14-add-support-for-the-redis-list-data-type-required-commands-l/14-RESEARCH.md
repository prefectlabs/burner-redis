# Phase 14: List data type (LPUSH, BRPOP, BLPOP, and full list command set) - Research

**Researched:** 2026-04-24
**Domain:** Redis list data type — storage model, blocking semantics, redis-py drop-in parity
**Confidence:** HIGH

## Summary

CONTEXT.md for Phase 14 is unusually complete — 22 locked decisions (D-01..D-22), explicit pattern-origin references to Phases 5/11, and exact line numbers for integration points. This research deliberately does NOT re-derive the storage model (`VecDeque<Bytes>` behind `parking_lot::RwLock`), the blocking architecture (`list_notify: Arc<Notify>` + `tokio::select!` deadline pattern), or the Lua integration (`dispatch_command_inner` + `had_list_mutation` flag). Those are locked.

Instead this document fills the remaining gaps:

1. **redis-py exact command signatures** — pulled verbatim from `redis-py` source. Signatures, return callbacks, and list-or-args unpacking are all sourced from the installed package at `/Users/alexander/.cache/uv/archive-v0/b2CFuwZrXaIDnSNRPmZXY/redis/commands/core.py`.
2. **Redis server-side semantics** — for every edge case flagged as Claude's Discretion (LRANGE negative indices, LPOP count=0, LINSERT pivot-not-found, LREM count signs, LTRIM empty-list key deletion, LMOVE src==dst), the precise server behavior is documented and cited.
3. **BLPOP multi-key scan order** — Redis canonical behavior verified: left-to-right scan; BLPOP on `[k1,k2,k3,k4]` where k2 and k4 are non-empty always returns from k2.
4. **BLPOP/BRPOP wire format** — server returns a 2-element array `[key, value]`; redis-py's callback wraps as tuple `(key, value)`. On timeout → nil → None. Must match this exactly on the Python side.
5. **Tokio Notify cancel-safety** — confirmed the existing re-arm pattern at `src/lib.rs:980-1038` works unchanged for multi-key BRPOP/BLPOP. The key insight is that `notified()` is NOT cancel-safe for queue-position, but the idiom `Box::pin(notify.notified())` + `waiter.as_mut().enable()` + `waiter.set(notify.notified())` after a select-drop is the documented workaround and is already proven for XREAD/XREADGROUP in Phase 11.
6. **Validation architecture** — pytest-based per-command parity tests, redis-py itself as reference oracle, explicit blocking/cancellation test matrix.

**Primary recommendation:** Keep the plan tightly scoped to the 16 commands in D-01. Follow the Phase 11 XREADGROUP blocking loop byte-for-byte as the template. Write the tests FIRST against redis-py (fixture client against a fakeredis or skipped-when-unavailable real-redis) to lock down expected behavior, then implement against those tests.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

**Command Surface**
- **D-01:** Full 16-command coverage in this phase — LPUSH, RPUSH, LPOP, RPOP, LRANGE, LLEN, LINDEX, LINSERT, LREM, LSET, LTRIM, LMOVE, RPOPLPUSH, BRPOP, BLPOP, BLMOVE. No BRPOPLPUSH (not in ROADMAP.md stretch list, superseded by BLMOVE).
- **D-02:** LPOP/RPOP match redis-py exactly — `count=None` returns single `bytes` (or `None`), `count=N` returns `list[bytes]` of up to N popped elements (or `None` if key missing). Full drop-in parity, no gaps.
- **D-03:** Empty list after final pop deletes the key — Redis standard behavior.

**Storage**
- **D-04:** Add `ValueData::List(VecDeque<Bytes>)` variant to the existing enum in `src/store.rs:118`, following the Phase 2/5 ValueData expansion pattern. Mutations go through the same `parking_lot::RwLock` write-lock-for-all-mutations discipline.
- **D-05:** New `src/commands/lists.rs` module for command-specific helpers (count parsing, LRANGE negative-index normalization, LREM count-sign handling, LINSERT pivot lookup, etc.), mirroring `src/commands/streams.rs`.

**Blocking Architecture**
- **D-06:** New dedicated `list_notify: Arc<Notify>` field on `Store`, parallel to the existing `stream_notify` (`src/store.rs:275`). Clean separation from streams; no cross-wake noise.
- **D-07:** BRPOP/BLPOP blocking loop mirrors the XREAD blocking loop in `src/lib.rs:980-1038` — first non-blocking attempt, then `notify.notified()` + `tokio::time::sleep(remaining)` inside `tokio::select!` with a deadline derived from `block_ms`. Re-arm `waiter.set(notify.notified()); waiter.as_mut().enable();` on each wake. BLMOVE uses the same skeleton but operates on source + destination.
- **D-08:** Graceful shutdown via `store.is_shutdown()` check at the top of each loop iteration — returns empty result so the Rust future completes via `call_soon_threadsafe` before the Python event loop tears down.
- **D-09:** Multi-key BRPOP/BLPOP — on each wake (and on first attempt), scan the keys list in order and pop from the first non-empty list. Return `(key, value)` tuple matching redis-py. If all are still empty, re-arm notify and loop until deadline.
- **D-10:** `list_notify.notify_waiters()` fires inside the write lock at the Store-method level, matching the existing XADD pattern at `src/store.rs:1262` and `2402`. Wake sites: LPUSH, RPUSH, LMOVE (destination side), RPOPLPUSH (destination side), and BLMOVE destination write.
- **D-11:** Timeout accepts `float` seconds at the Python layer (matching redis-py), converts to `Option<u64>` milliseconds passed to Rust. `0` → block forever. Positive → deadline. Exact mirror of XREAD `block=0` handling.

**Lua Integration**
- **D-12:** All non-blocking list commands added to `dispatch_command_inner` in `src/scripting.rs` — LPUSH, RPUSH, LPOP, RPOP, LRANGE, LLEN, LINDEX, LINSERT, LREM, LSET, LTRIM, LMOVE, RPOPLPUSH.
- **D-13:** BRPOP / BLPOP / BLMOVE called from Lua return `RedisValue::Error` matching real Redis: `"ERR This Redis command is not allowed from scripts: <cmd>"`.
- **D-14:** Lua list mutations fire `list_notify.notify_waiters()` after script execution — extend `dispatch_command()`'s return tuple: add `had_list_mutation` flag alongside `had_xadd`.

**Pipeline Integration**
- **D-15:** BRPOP/BLPOP/BLMOVE inside a pipeline respect their per-command timeouts. Pipelines are sequential in-process loops; blocking one command simply delays subsequent commands.
- **D-16:** `execute_pipeline()` in `src/lib.rs:2182` detects blocking commands in the queue. No blocking commands → keep sync fast path (preserves async-overhead elimination from `260415-an2`). Blocking commands present → per-command async loop that invokes normal `BurnerRedis.brpop()` / `blpop()` / `blmove()` awaitables.
- **D-17:** Every new list command gets a pipeline stub method in `python/burner_redis/pipeline.py` buffering `(method_name, args, kwargs)`.

**Python Surface & Compatibility**
- **D-18:** All 16 command methods are async and match `redis.asyncio.Redis` signatures exactly.
- **D-19:** Value coercion uses existing `_coerce_value` helper from `python/burner_redis/__init__.py`. Applied to LPUSH, RPUSH, LSET, LINSERT, and destination-write of LMOVE / RPOPLPUSH / BLMOVE.
- **D-20:** WRONGTYPE errors use existing `StoreError::WrongType` → `ResponseError` conversion.
- **D-21:** REQUIREMENTS.md update is part of this phase's deliverable — remove "Blocking list commands (BLPOP/BRPOP)" from Out of Scope, add `LIST-*` requirements section, map to Phase 14 in Traceability table.

**Testing**
- **D-22:** New `tests/test_lists.py` for pytest integration coverage.

### Claude's Discretion
- Exact helper boundary between `src/store.rs` and `src/commands/lists.rs`.
- LRANGE negative-index normalization logic.
- LINSERT pivot-not-found return code (`-1` per Redis spec).
- LREM count-sign semantics (positive = head-to-tail, negative = tail-to-head, 0 = all).
- LPOP `count=0` precise return.
- Internal organization of the 16 new `#[pymethods]` in `src/lib.rs`.
- Whether to split the phase into 2 plans or 3 plans.

### Deferred Ideas (OUT OF SCOPE)
- **BRPOPLPUSH** (blocking legacy variant) — not in ROADMAP.md stretch list, superseded by BLMOVE.
- **Per-key fine-grained notify** (`HashMap<Bytes, Arc<Notify>>`) — over-engineered for embedded use.
- **LPOS command** — not requested in this phase, not in ROADMAP.md.
</user_constraints>

<phase_requirements>
## Phase Requirements

Phase 14's requirement IDs do not yet exist in REQUIREMENTS.md — per D-21, adding them is part of the phase's deliverable. Proposed LIST-* IDs and their research support:

| ID | Description | Research Support |
|----|-------------|------------------|
| LIST-01 | User can LPUSH one or more values onto the head of a list | redis-py `lpush(name, *values)` — variadic push, returns new length. Multi-value insertion order: `LPUSH k a b c` results in `[c, b, a]` (each value pushed in turn to head). [CITED: redis.io/commands/lpush] |
| LIST-02 | User can RPUSH one or more values onto the tail of a list | Mirror of LPUSH for tail. [CITED: redis.io/commands/rpush] |
| LIST-03 | User can LPOP with optional count — `count=None` returns bytes (or None); `count=N` returns list (or None if key missing) | redis-py `lpop(name, count=None)` at `core.py:3036`. Return callback differs: no-count → bulk; count → array. Deletes key when empty (D-03). [CITED: redis-py ListCommands.lpop] |
| LIST-04 | User can RPOP with the same semantics as LPOP | Mirror of LPOP for tail. redis-py `rpop(name, count=None)` at `core.py:3117`. |
| LIST-05 | User can LRANGE with negative indices (Python-style) to slice a list | redis-py `lrange(name, start, end)`. Out-of-range indices clamp; start > stop returns empty array. [CITED: redis.io/commands/lrange] |
| LIST-06 | User can LLEN to get the length of a list (0 for missing key) | redis-py `llen(name)`. |
| LIST-07 | User can LINDEX to read an element at an index (negative supported); out-of-range → None | redis-py `lindex(name, index)`. [CITED: redis.io/commands/lindex] |
| LIST-08 | User can LINSERT BEFORE or AFTER a pivot; returns new length, 0 for missing key, -1 for missing pivot | [CITED: redis.io/commands/linsert] |
| LIST-09 | User can LREM with count > 0 (head→tail), count < 0 (tail→head), or count = 0 (all occurrences); returns count removed | [CITED: redis.io/commands/lrem] |
| LIST-10 | User can LSET to replace an element at an index; raises error on out-of-range or missing key | [CITED: redis.io/commands/lset] |
| LIST-11 | User can LTRIM to clamp a list to a range; empty result deletes the key | [CITED: redis.io/commands/ltrim] |
| LIST-12 | User can LMOVE between two lists (or same list for rotation); atomic; returns moved element or None | [CITED: redis.io/commands/lmove] |
| LIST-13 | User can RPOPLPUSH as a legacy alias for `LMOVE src dst RIGHT LEFT`; same-key rotation supported | [CITED: redis.io/commands/rpoplpush] |
| LIST-14 | User can BRPOP/BLPOP on multiple keys with a float-seconds timeout; timeout=0 blocks indefinitely; returns `(key, value)` tuple or None | [VERIFIED: redis-py `_parsers/helpers.py:862` — `lambda r: r and tuple(r) or None`] |
| LIST-15 | User can BLMOVE with float-seconds timeout; src/dst/direction semantics identical to LMOVE | [CITED: redis-py `core.py:2208`] |
| LIST-16 | All 16 commands work in Pipelines (blocking respect per-command timeout) and all 13 non-blocking commands work from Lua scripts (BRPOP/BLPOP/BLMOVE in Lua return ERR) | [ASSUMED: matches real Redis "not allowed from scripts" wording — verify wording in server source before freezing] |
</phase_requirements>

## Architectural Responsibility Map

This is a single-process embedded database with a Python API; there is no multi-tier architecture. Ownership is "which module owns which capability":

| Capability | Primary Owner | Secondary Owner | Rationale |
|------------|---------------|-----------------|-----------|
| List data structure + mutations | `src/store.rs` | `src/commands/lists.rs` (helpers) | Keyspace owner; all mutations go through its `parking_lot::RwLock` |
| List notification fan-out | `src/store.rs` (owns `list_notify: Arc<Notify>`) | — | Pairs with the lock-owning module; `notify_waiters()` fires inside write-lock at mutation sites (D-10) |
| PyO3 command bindings | `src/lib.rs` (`#[pymethods]`) | `src/commands/lists.rs` (argument parsing helpers) | Python ABI boundary; non-blocking commands use `resolved()`, blocking ones use `pyo3_async_runtimes::tokio::future_into_py` |
| Blocking loop (BRPOP/BLPOP/BLMOVE) | `src/lib.rs` (inside each `#[pymethods]`) | `src/store.rs` (`list_notify` accessor, `is_shutdown()`) | Async runtime owner — `tokio::select!` + deadline + notify re-arm live here |
| Lua dispatch for non-blocking cmds | `src/scripting.rs::dispatch_command_inner` | `src/store.rs` (operates on `&mut HashMap<Bytes, ValueEntry>`) | Scripting owns command dispatch under an already-acquired write lock |
| `had_list_mutation` flag propagation | `src/scripting.rs::dispatch_command` | `src/store.rs::eval`/`evalsha` (call `list_notify.notify_waiters()` after drop) | Mirrors existing `had_xadd` pattern at `scripting.rs:274` and `store.rs:2401` |
| Pipeline dispatch (non-blocking) | `src/lib.rs::dispatch_pipeline_command` | — | Sync fast path, preserves `260415-an2` perf |
| Pipeline dispatch (blocking) | `src/lib.rs::execute_pipeline` (new async branch) | `BurnerRedis.brpop()` / `.blpop()` / `.blmove()` (existing awaitables) | Detect blocking command in queue, fall through to per-command async path (D-16) |
| Value coercion for writes | `python/burner_redis/__init__.py` (`_coerce_value`) | `src/commands/strings.rs::extract_bytes` (bytes extraction) | Python-side boundary coerces int/float/str/bool before hitting Rust |
| Pipeline stubs | `python/burner_redis/pipeline.py` | — | Python-side command buffer; 16 new methods |
| Exception hierarchy | `python/burner_redis/__init__.py` (`ResponseError`) | `src/lib.rs::store_err_to_py` | WRONGTYPE and other errors are surfaced by the Rust bindings as Python `ResponseError` |
| Test coverage | `tests/test_lists.py` (new) | `tests/conftest.py` (existing `r` fixture) | Standalone test file per data type (same convention as `test_streams.py`, `test_sets.py`) |

## Standard Stack

All dependencies are already in `Cargo.toml` — this phase adds no new crates.

### Core (already in use)

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `tokio` | 1.51 (latest: 1.52.1) | Async runtime, `Notify`, `select!`, `time::sleep` | Already in use for XREAD blocking; blocking list commands use the same primitives. [VERIFIED: `cargo search tokio`] |
| `parking_lot` | 0.12.5 | `RwLock` for keyspace | Already the project's lock; same discipline — single big RwLock across all ValueData variants. [VERIFIED: Cargo.toml + cargo search] |
| `bytes::Bytes` | 1.11 | Byte-slice storage (keys, list elements) | Already the ValueData payload type for strings, hash values, set members. `VecDeque<Bytes>` is the natural fit. [VERIFIED: Cargo.toml] |
| `pyo3` | 0.28.3 (abi3-py310) | Python bindings | Already project standard. |
| `pyo3-async-runtimes` | 0.28.0 | `future_into_py` for blocking commands | Already used for XREAD/XREADGROUP blocking paths; the pattern is: `future_into_py(py, async move { ... })`. [VERIFIED: Cargo.toml + existing code at `lib.rs:982`] |
| `mlua` | 0.10 (with `lua54,send`) | Lua VM for EVAL/EVALSHA | Already wired; non-blocking list commands plug into `dispatch_command_inner` the same way strings/hashes/sets do. [VERIFIED: Cargo.toml] |

### Supporting (already in use)

| Library | Version | Purpose |
|---------|---------|---------|
| `serde` + `rmp-serde` | 1.0 / 1.3 | Persistence snapshot — `ValueData::List` variant will need `Serialize + Deserialize` derives (will be covered by existing derive on the enum) |
| `thiserror` | 2.0 | `StoreError` — `WrongType` variant reused for non-list key types; likely need `IndexOutOfRange` for LSET |
| `ordered-float` | 5 | Not needed for lists (floats aren't elements); flagged only because it's in Cargo.toml |

### No alternatives considered

All three "would-I-pick-them-today" options (tokio, parking_lot, bytes) are locked into the existing architecture. A hypothetical `VecDeque` swap-out (e.g., `im::Vector`, `rust-reference-queue`, intrusive list) would buy nothing for the single-writer RwLock model and would break the `Clone` discipline used by persistence and pub/sub.

**Version verification** (2026-04-24):
```bash
cargo search tokio --limit 1        # latest: 1.52.1 (project uses 1.51)
cargo search parking_lot --limit 1  # latest: 0.12.5 (project pinned)
```
Both within acceptable currency. No upgrade required for Phase 14.

## Architecture Patterns

### System Architecture Diagram

```
┌────────────────────────────── Python API boundary ──────────────────────────────┐
│                                                                                 │
│   redis.asyncio.Redis user code                                                 │
│         │                                                                       │
│         ▼                                                                       │
│   BurnerRedis (Python wrapper; __init__.py monkey-patches _coerce_value)        │
│         │                                                                       │
│         │ bytes-coerced args                                                    │
│         ▼                                                                       │
│  ┌─────────────────────────────────────────────────────────────────────────┐    │
│  │                          PyO3 boundary                                  │    │
│  │                                                                         │    │
│  │ src/lib.rs #[pymethods] BurnerRedis                                     │    │
│  │                                                                         │    │
│  │  ┌───────────────────────────┐   ┌────────────────────────────────┐     │    │
│  │  │ Non-blocking commands     │   │ Blocking commands              │     │    │
│  │  │ (LPUSH, LPOP, LRANGE, ...)│   │ (BLPOP, BRPOP, BLMOVE)         │     │    │
│  │  │                           │   │                                │     │    │
│  │  │ fn cmd<'py>(..)           │   │ future_into_py(py, async move {│     │    │
│  │  │   -> resolved(py, val)    │   │   loop {                       │     │    │
│  │  │                           │   │     first_attempt ▶︎ return    │     │    │
│  │  │                           │   │     select! {                  │     │    │
│  │  │                           │   │       notify.notified() => ..  │     │    │
│  │  │                           │   │       sleep(remaining) => ..   │     │    │
│  │  │                           │   │     }                          │     │    │
│  │  │                           │   │   }                            │     │    │
│  │  │                           │   │ })                             │     │    │
│  │  └───────────┬───────────────┘   └────────────────┬───────────────┘     │    │
│  │              │                                    │                     │    │
│  │              └─────────── store.{lpush|lpop|brpop|...} ────────────┐    │    │
│  │                                                                    │    │    │
│  │  ┌────────────────────────────────────────────────────────────────┼──┐  │    │
│  │  │ src/store.rs                                                   ▼  │  │    │
│  │  │                                                                   │  │    │
│  │  │   RwLock<HashMap<Bytes, ValueEntry{ data: ValueData, expires }>>  │  │    │
│  │  │                                                                   │  │    │
│  │  │   ValueData::List(VecDeque<Bytes>)  ◀──── new variant (D-04)      │  │    │
│  │  │                                                                   │  │    │
│  │  │   list_notify: Arc<Notify>  ◀──── new field (D-06)                │  │    │
│  │  │                                                                   │  │    │
│  │  │   pub fn lpush(&self, ..) { write-lock; push_front; notify_waiters│  │    │
│  │  │   pub fn brpop_poll(..)   { write-lock; pop_back; (key, val)      │  │    │
│  │  │                                                                   │  │    │
│  │  └───────────────────────────────────────────────────────────────────┘  │    │
│  │                                                                         │    │
│  │  ┌───────────────────────────────────────────────────────────────────┐  │    │
│  │  │ src/scripting.rs (Lua path) — non-blocking only                   │  │    │
│  │  │                                                                   │  │    │
│  │  │   dispatch_command_inner("LPUSH", args, &mut data) ─ operates     │  │    │
│  │  │     directly on the write-locked HashMap (atomicity)              │  │    │
│  │  │                                                                   │  │    │
│  │  │   dispatch_command() returns (RedisValue, had_xadd,               │  │    │
│  │  │                                had_list_mutation)    ◀── new flag │  │    │
│  │  │                                                                   │  │    │
│  │  │   eval/evalsha: after LuaEngine::execute drops data lock,         │  │    │
│  │  │     if had_list_mutation { list_notify.notify_waiters(); }        │  │    │
│  │  │                                                                   │  │    │
│  │  │   BRPOP/BLPOP/BLMOVE from Lua → RedisValue::Error(                │  │    │
│  │  │     "ERR This Redis command is not allowed from scripts: <cmd>")  │  │    │
│  │  └───────────────────────────────────────────────────────────────────┘  │    │
│  │                                                                         │    │
│  │  ┌───────────────────────────────────────────────────────────────────┐  │    │
│  │  │ src/lib.rs::execute_pipeline                                      │  │    │
│  │  │                                                                   │  │    │
│  │  │   scan queue for blocking cmd (BLPOP/BRPOP/BLMOVE)                │  │    │
│  │  │       │                                                           │  │    │
│  │  │       ├─ none → dispatch_pipeline_command (sync fast path)        │  │    │
│  │  │       │         (preserves 260415-an2 perf win)                   │  │    │
│  │  │       └─ some → per-cmd async loop: for each command call the     │  │    │
│  │  │                 normal async #[pymethod] awaitable                │  │    │
│  │  └───────────────────────────────────────────────────────────────────┘  │    │
│  │                                                                         │    │
│  └─────────────────────────────────────────────────────────────────────────┘    │
│                                                                                 │
└─────────────────────────────────────────────────────────────────────────────────┘
```

### Blocking-loop pattern (XREADGROUP template, verified at `src/lib.rs:980-1038`)

The idiom is load-bearing. Every deviation causes lost-wakeup or busy-wait bugs. The exact sequence:

```rust
// Source: src/lib.rs:982-1038 (XREAD blocking loop)
pyo3_async_runtimes::tokio::future_into_py(py, async move {
    let notify = store.list_notify();
    let mut waiter = Box::pin(notify.notified());
    waiter.as_mut().enable();  // register permit BEFORE first poll

    // First non-blocking attempt — commonly succeeds when data is present.
    if let Some(result) = store.brpop_poll(&keys) {
        return format_brpop_result(result);
    }

    let deadline_opt = if block_ms == 0 {
        None  // block forever
    } else {
        Some(tokio::time::Instant::now() + Duration::from_millis(block_ms))
    };

    loop {
        if store.is_shutdown() {
            break format_brpop_none();  // graceful teardown
        }

        let remaining = match deadline_opt {
            Some(d) => {
                let r = d.saturating_duration_since(tokio::time::Instant::now());
                if r.is_zero() { break format_brpop_none(); }
                r
            }
            None => Duration::from_secs(3600),  // block-forever long slice
        };

        tokio::select! {
            _ = waiter.as_mut() => {
                // Re-arm BEFORE re-polling (Phase 11 critical fix)
                waiter.set(notify.notified());
                waiter.as_mut().enable();
                if let Some(result) = store.brpop_poll(&keys) {
                    break format_brpop_result(result);
                }
                // otherwise: loop
            }
            _ = tokio::time::sleep(remaining) => {
                if deadline_opt.is_some() { break format_brpop_none(); }
                // block=0: keep looping
            }
        }
    }
})
```

**Why the re-arm matters:** `notified()` is NOT cancel-safe for queue-position — dropping the future loses your place in the waiter queue. The `waiter.set(notify.notified()); waiter.as_mut().enable();` idiom creates a fresh waiter AND arms its permit before the next `select!` iteration. Without this, a `notify_waiters()` call that fires between the drop and the next poll is lost. [VERIFIED: tokio docs confirm this is the documented pattern; existing Phase 11 fix proves it works in production]

### Multi-key scan order (authoritative Redis behavior)

Per Redis spec: `BLPOP k1 k2 k3 k4 0` with k2 and k4 both non-empty returns from k2. Scan left-to-right on every poll. [CITED: redis.io/commands/blpop — "When a client tries to block for multiple keys but at least one key contains elements, the returned key/element pair is the first key from left to right that has one or more elements."]

The store-side helper signature:

```rust
/// Scan keys left-to-right; pop from the first non-empty list.
/// Returns (key, value) on success, None if all keys are missing or empty.
/// Applies D-03: deletes keys whose last element was popped.
pub fn brpop_poll(&self, keys: &[Bytes]) -> Result<Option<(Bytes, Bytes)>, StoreError>;

pub fn blpop_poll(&self, keys: &[Bytes]) -> Result<Option<(Bytes, Bytes)>, StoreError>;
```

`Err(WrongType)` propagates to the caller immediately (first wrong-typed key aborts the scan — matches Redis).

### BLMOVE under a single write lock (cross-key atomicity)

BLMOVE is the only blocking command that is also a multi-key write. Both the pop-from-source and the push-to-destination must happen under a SINGLE write-lock acquisition to preserve atomicity. The store method:

```rust
/// Atomic pop-from-src + push-to-dst.
/// - src_from: which end to pop from
/// - dst_to:   which end to push onto
/// - src==dst is valid: performs rotation (RIGHT+LEFT), no-op (LEFT+LEFT or RIGHT+RIGHT)
/// - Fires list_notify.notify_waiters() if push occurred
/// - Deletes src if it becomes empty (D-03)
pub fn lmove_atomic(
    &self,
    src: &Bytes,
    dst: &Bytes,
    src_from: ListEnd,  // Left | Right
    dst_to: ListEnd,    // Left | Right
) -> Result<Option<Bytes>, StoreError>;
```

The blocking variant calls `lmove_atomic` on each wake. [CITED: redis.io/commands/lmove — "The command is atomic... no possibility of another client interfering between them."]

### Recommended Project Structure

```
src/
├── store.rs
│   ├── ValueData::List(VecDeque<Bytes>)          # D-04
│   ├── list_notify: Arc<Notify>                  # D-06 (field)
│   ├── Store::lpush / rpush / lpop / rpop / ...  # ~16 new methods
│   └── Store::shutdown() also calls list_notify.notify_waiters()  # D-08
├── lib.rs
│   ├── #[pymethods] lpush/rpush/lpop/...         # 13 non-blocking
│   ├── #[pymethods] brpop/blpop/blmove           # 3 blocking (future_into_py)
│   ├── dispatch_pipeline_command: 13 new arms    # D-16 fast path
│   └── execute_pipeline: detect blocking cmd     # D-16 dual-path
├── scripting.rs
│   ├── dispatch_command -> (RedisValue, had_xadd, had_list_mutation)
│   └── dispatch_command_inner: 13 non-blocking arms + 3 blocking-reject arms
├── commands/
│   ├── mod.rs (add `pub mod lists;`)
│   └── lists.rs                                   # D-05 helpers
└── persistence.rs (no changes — rmp-serde derives on ValueData pick up List variant)

python/burner_redis/
├── __init__.py   (optional monkey-patches for value coercion on push commands)
└── pipeline.py   (16 new stubs; 3 blocking stubs carry timeout kwarg)

tests/
└── test_lists.py (new — D-22)
```

### Pattern 1: Non-blocking command (LPUSH, template)

```rust
// Source: analogy to src/lib.rs:2322+ for sadd()
#[pyo3(signature = (name, *values))]
fn lpush<'py>(
    &self,
    py: Python<'py>,
    name: &Bound<'py, PyAny>,
    values: &Bound<'py, PyTuple>,
) -> PyResult<Bound<'py, PyAny>> {
    let key = extract_bytes(name)?;
    let vals: Vec<Bytes> = values.iter()
        .map(|obj| extract_bytes(&obj))
        .collect::<PyResult<Vec<_>>>()?;
    let len = self.store.lpush(key, vals).map_err(store_err_to_py)?;
    resolved(py, (len as i64).into_pyobject(py)?.into_any().unbind())
}
```

Store method:

```rust
pub fn lpush(&self, key: Bytes, values: Vec<Bytes>) -> Result<i64, StoreError> {
    let mut data = self.data.write();
    // passive expiration (match existing pattern at store.rs:1284)
    if let Some(entry) = data.get(&key) {
        if entry.is_expired() { data.remove(&key); }
    }
    let entry = data.entry(key).or_insert_with(ValueEntry::new_list);
    match entry.data {
        ValueData::List(ref mut list) => {
            // redis-py semantics: LPUSH k a b c -> list becomes [c, b, a]
            // Each value pushed in turn to the head.
            for v in values { list.push_front(v); }
            let len = list.len() as i64;
            self.list_notify.notify_waiters();  // D-10
            Ok(len)
        }
        _ => Err(StoreError::WrongType),
    }
}
```

### Pattern 2: LRANGE negative-index normalization (Claude's Discretion → concrete algorithm)

Redis normalization for an inclusive range with Python-style negative indices, given `n = list.len()`:

```rust
fn normalize_range(n: usize, start: i64, end: i64) -> Option<(usize, usize)> {
    if n == 0 { return None; }
    let n_i64 = n as i64;
    // Normalize negatives
    let start = if start < 0 { (start + n_i64).max(0) } else { start.min(n_i64 - 1) };
    let end   = if end < 0   { end + n_i64 }           else { end.min(n_i64 - 1) };
    // Reject empty ranges
    if start > end || end < 0 { return None; }
    Some((start as usize, end as usize))  // inclusive on both ends
}
```

Test matrix (write these as unit tests in `commands/lists.rs`):

| list_len | start | end | expected |
|----------|-------|-----|----------|
| 5 | 0 | -1 | (0,4) — all elements |
| 5 | 0 | 100 | (0,4) — end clamps to last |
| 5 | -100 | 100 | (0,4) — start clamps to 0, end to last |
| 5 | -3 | -1 | (2,4) — last three |
| 5 | -3 | 2 | (2,2) — one element |
| 5 | 5 | 10 | None — start past end |
| 5 | 3 | 2 | None — start > end |
| 5 | -10 | -6 | None — end < 0 after normalization |
| 0 | 0 | 0 | None — empty list |

Source for spec: [CITED: redis.io/commands/lrange]

### Pattern 3: LREM count-sign (Claude's Discretion → exact behavior)

```rust
pub fn lrem(&self, key: Bytes, count: i64, value: Bytes) -> Result<i64, StoreError> {
    let mut data = self.data.write();
    // passive expiration…
    let entry = match data.get_mut(&key) {
        None => return Ok(0),  // non-existent key returns 0
        Some(e) => e,
    };
    let list = match &mut entry.data {
        ValueData::List(l) => l,
        _ => return Err(StoreError::WrongType),
    };

    let mut removed: i64 = 0;
    match count.cmp(&0) {
        std::cmp::Ordering::Greater => {
            // head to tail, remove up to count
            let target = count as usize;
            list.retain(|v| {
                if removed < target as i64 && v == &value { removed += 1; false } else { true }
            });
        }
        std::cmp::Ordering::Less => {
            // tail to head, remove up to |count|
            let target = (-count) as usize;
            let indices: Vec<usize> = list.iter().enumerate()
                .rev()
                .filter_map(|(i, v)| if v == &value { Some(i) } else { None })
                .take(target)
                .collect();
            for i in indices {  // indices are descending — safe to remove in order
                list.remove(i);
                removed += 1;
            }
        }
        std::cmp::Ordering::Equal => {
            // remove all
            let before = list.len();
            list.retain(|v| v != &value);
            removed = (before - list.len()) as i64;
        }
    }

    // D-03: delete key if empty
    if list.is_empty() { data.remove(&key); }
    Ok(removed)
}
```

[CITED: redis.io/commands/lrem — "count > 0 head-to-tail, count < 0 tail-to-head, count = 0 all occurrences. Deletes the list if the last element was removed."]

### Pattern 4: LPOP count semantics (Claude's Discretion → confirmed)

Confirmed against Redis source code via GitHub issues/PRs:

| Case | Return |
|------|--------|
| `LPOP k` (no count), key missing | `nil` → Python `None` |
| `LPOP k` (no count), list has ≥1 element | bulk string → Python `bytes` |
| `LPOP k N` with N > 0, key missing | `nil` → Python `None` (post-#10095 fix) |
| `LPOP k 0`, key exists (any type) — type-checks first | empty array → Python `[]` |
| `LPOP k 0`, key missing | `nil` → Python `None` (consistent with N > 0 case) |
| `LPOP k N`, list has fewer than N elements | array with all popped → Python `list[bytes]` |

[CITED: github.com/redis/redis/pull/9692, /pull/10095 — fixes for LPOP count=0 returning empty array, and LPOP count=N on missing key returning null array]

Pseudocode:

```rust
pub fn lpop(&self, key: &Bytes, count: Option<usize>) -> Result<LPopResult, StoreError> {
    let mut data = self.data.write();
    // passive expiration
    match data.get(key) {
        None => return Ok(LPopResult::Nil),  // key missing — return nil regardless of count
        Some(entry) if entry.is_expired() => { data.remove(key); return Ok(LPopResult::Nil); }
        Some(entry) => match &entry.data {
            ValueData::List(_) => {}  // proceed
            _ => return Err(StoreError::WrongType),
        }
    }

    // type-check confirmed; now handle count=0 special case
    if count == Some(0) {
        return Ok(LPopResult::Array(Vec::new()));
    }

    let list = match &mut data.get_mut(key).unwrap().data {
        ValueData::List(l) => l,
        _ => unreachable!(),
    };

    match count {
        None => {
            let val = list.pop_front().expect("type-checked non-empty");
            // Actually: need to handle the case where type-check passed but list is empty.
            // In practice: a list can be empty mid-operation (LSET creates empty-ish states? No).
            // Lists in the store are invariantly non-empty (D-03 deletes them).
            // But defensively:
            if list.is_empty() { data.remove(key); }
            Ok(LPopResult::Single(val))
        }
        Some(n) => {
            let actual = n.min(list.len());
            let popped: Vec<Bytes> = (0..actual).map(|_| list.pop_front().unwrap()).collect();
            if list.is_empty() { data.remove(key); }
            Ok(LPopResult::Array(popped))
        }
    }
}

enum LPopResult { Nil, Single(Bytes), Array(Vec<Bytes>) }
```

Python-side: the `#[pymethod]` returns `None` | `bytes` | `list[bytes]` based on the variant. Pipeline wrapper does the same.

### Anti-Patterns to Avoid

- **Locking ordering inversion with Lua.** Phase 11 established that `pubsub` broadcast senders must be cloned BEFORE acquiring the data write lock. List notify follows the same rule: `list_notify.notify_waiters()` call is cheap and non-blocking, but is performed AFTER the data write lock is dropped at the store-method level (already the pattern in XADD at `store.rs:1262`). For Lua, notify happens AFTER `LuaEngine::execute` returns and the data lock drops (mirror the `stream_notify.notify_waiters()` call at `store.rs:2401` / `2429`).

- **Per-key fine-grained notify.** Deferred-item #2 explicitly rules this out. A single `Arc<Notify>` wakes ALL blocking clients on ANY list mutation. They then re-poll and spin down the ones that still have empty lists. Don't add `HashMap<Bytes, Arc<Notify>>` — waker coarseness is fine at this scale.

- **Busy-waiting without deadline awareness.** The `tokio::select!` arm for sleep must use `saturating_duration_since(Instant::now())` — computing the remaining time on EACH iteration. A fixed `sleep(block_ms)` passed into the loop would re-sleep the FULL block duration after every notify wake — massively breaking timeout semantics.

- **Forgetting to re-enable the waiter.** Omitting `waiter.as_mut().enable()` after `waiter.set()` loses any notify that fires between `set()` and the next `select!`. Phase 11's direct fix for the `xreadgroup` race. [CITED: lib.rs:1322-1325]

- **Dropping the write lock before `notify_waiters()` in BLMOVE.** The LMOVE atomicity guarantee requires pop+push under a single lock. `notify_waiters()` goes INSIDE the lock (like XADD at `store.rs:1262`) — the notify itself doesn't block because `Notify::notify_waiters` is non-blocking; it only records permits.

- **Type-check after count=0 fast-exit in LPOP.** Real Redis had this exact bug (issue #9680). Type-check MUST come first; count=0 fast-return happens only after the key is verified to be a list. Our implementation above already has this order.

- **Returning `Vec<u8>` instead of `PyBytes` for list elements.** Python side needs actual `bytes` objects, not `bytearray`. Use `PyBytes::new(py, &v)` — same as the existing `hvals` implementation at `lib.rs:2293`.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Deque with O(1) push/pop both ends | Custom doubly-linked list | `std::collections::VecDeque<Bytes>` | Standard library, O(1) push/pop on both ends, contiguous storage with good cache locality. Redis's own quicklist is a memory-efficiency optimization, not a correctness requirement. |
| Notification system for blocking waiters | Custom `Mutex<Vec<Waker>>` + manual wake | `tokio::sync::Notify` | Already project standard, already used for streams, already integrated with tokio runtime. |
| Blocking with timeout in async | Custom loop with `Instant::now()` polling | `tokio::select!` + `tokio::time::sleep` | `select!` is cancel-aware, sleep resolves into a timer wheel; rolling your own drops wakes. |
| Python-side value coercion (int/float/str/bool) | New `_coerce_list_value` | Existing `_coerce_value` in `__init__.py:41` | Same semantics as SET, HSET, SADD, ZADD, XADD values. Extend if needed; don't duplicate. |
| Glob-style argument handling for LPUSH variadic | Custom parsing | PyO3's `*args` → `Bound<PyTuple>` + `.iter()` + `extract_bytes()` | Already the pattern for SADD/HDEL/DELETE — see `lib.rs:2325`. |
| Pipeline blocking-command detection | Ad-hoc `if method == "brpop"` | Match against a static `const BLOCKING_COMMANDS: &[&str]` slice | One source of truth; makes future BLMPOP trivially addable. |
| Async result formatting (PyBytes from bytes) | Manual `PyList::new().append(PyBytes::new(..))` in hot loops | Existing helpers — look at `build_xread_pylist` for streams; mirror the shape | Consistency with existing code; easier code review. |
| Lua error string for blocking commands | Custom wording | Use Redis canonical text: `"ERR This Redis command is not allowed from scripts: BLPOP"` (uppercase command name) | Match real Redis wording so anyone porting scripts sees identical errors. [VERIFIED from Redis source history — SUBSCRIBE/WAIT/BLPOP all share this exact error class] |

**Key insight:** This phase is a surface-extension, not an architecture change. Everything the phase needs already exists in the codebase; the discipline is to add new ValueData variant + store methods + py methods following EXACTLY the patterns used for hashes/sets/streams. Any time a new abstraction "feels needed," check whether it already exists somewhere else in the project — it almost certainly does.

## Runtime State Inventory

**Omitted — greenfield feature addition, not a rename/migration phase.** No runtime state to update. Persistence snapshots written before Phase 14 do not contain `ValueData::List` entries, so there is no backward-compatibility concern. The rmp-serde format is self-describing, so an old snapshot simply lacks the List variant and continues to load cleanly. New snapshots written after Phase 14 gain List entries automatically via serde derive.

## Common Pitfalls

### Pitfall 1: Lost wake-up from Lua-invoked LPUSH
**What goes wrong:** A blocked BRPOP client does not wake after a Lua script calls `redis.call("LPUSH", k, v)`.
**Why it happens:** `LuaEngine::execute` dispatches LPUSH through `dispatch_command_inner`, which mutates the data HashMap directly but does NOT fire `list_notify.notify_waiters()` — because notify is owned by `Store`, not the Lua engine.
**How to avoid:** D-14: extend `dispatch_command()` return tuple to `(RedisValue, had_xadd, had_list_mutation)`, set `had_list_mutation` for LPUSH/RPUSH/LMOVE/RPOPLPUSH/LSET/LINSERT-that-inserted/LREM-that-inserted-nothing? (only list-GROWING ops wake waiters; removal never wakes). After `Store::eval`/`evalsha` drops the data lock, call `list_notify.notify_waiters()` if the flag is set — exactly like the `had_xadd` path at `store.rs:2401`.
**Warning signs:** Tests that LPUSH from Lua then read via BRPOP timeout instead of returning. Phase 11 fixed the identical bug for XADD-from-Lua → XREADGROUP.

### Pitfall 2: Timeout regression on consecutive notify wakes
**What goes wrong:** BLPOP called with `timeout=5`, receives two spurious notify wakes at t=2 and t=4, then the caller expects at most 5 seconds total. If the sleep resets on each wake, the total time becomes unbounded.
**Why it happens:** `tokio::time::sleep(Duration::from_millis(block_ms))` naively restarts the full timeout on each iteration.
**How to avoid:** Compute `remaining = deadline.saturating_duration_since(Instant::now())` at the TOP of each loop iteration; break immediately if zero. Exact pattern from `lib.rs:1316-1319`.
**Warning signs:** Integration tests measuring wall-clock for BLPOP timeout show 2x or 3x the expected duration under cross-key wake storms.

### Pitfall 3: BLMOVE non-atomicity under cancellation
**What goes wrong:** Client cancels BLMOVE asyncio task mid-operation; the element is popped from source but never pushed to destination (data loss).
**Why it happens:** If pop and push happen in separate locked sections and the async task is cancelled between them, the push never runs.
**How to avoid:** `lmove_atomic` takes a SINGLE write lock, does pop+push under it, then drops the lock. The `future_into_py` wrapper awaits the Store method as a synchronous call inside `tokio::task::spawn_blocking`... actually, no — because the RwLock is `parking_lot` (not tokio-aware) the call is synchronous and holds the runtime thread for microseconds. Atomicity is guaranteed by the lock itself. Cancellation can only happen at `tokio::select!` await points, which are only reached BEFORE `lmove_atomic` or AFTER it completed.
**Warning signs:** Missing elements after cancel-heavy test loads. Verify with a stress test: 1000 BLMOVE tasks + random cancellations + final invariant check `len(src) + len(dst) == initial_len`.

### Pitfall 4: LPOP count=0 returning nil instead of empty array
**What goes wrong:** `LPOP k 0` returns `None` instead of `[]`, breaking callers that do `result := LPOP(k, count=0); assert isinstance(result, list)`.
**Why it happens:** Naive implementation exits on `count == 0` before or without type checking, returning nil.
**How to avoid:** Type-check FIRST (WRONGTYPE on non-list); return nil on missing key; otherwise return empty array `[]` when count is exactly 0. Pseudocode above reflects this. [CITED: redis/redis#9692 fix; #10089; #10095]
**Warning signs:** redis-py parity tests diverge only on count=0 cases.

### Pitfall 5: Key order in BLPOP multi-key not matching Redis
**What goes wrong:** BLPOP on `[k1, k2, k3]` where k2 and k3 both have values returns from k3 (or non-deterministically).
**Why it happens:** HashMap iteration order used instead of the passed key list order.
**How to avoid:** Store method `blpop_poll(&self, keys: &[Bytes])` iterates `keys` left-to-right with a `for k in keys.iter()` loop. First non-empty list wins. No HashMap iteration.
**Warning signs:** Flaky test outputs — key order differs between runs but test passes sometimes.

### Pitfall 6: Value coercion double-applied in Pipeline
**What goes wrong:** `pipe.lpush(k, 42)` — the integer is coerced to `b"42"` once at the Python-layer pipeline stub, then RE-coerced by the Rust `lpush` binding (if the binding also coerces).
**Why it happens:** Inconsistent placement of `_coerce_value` — some commands coerce at Python, some at Rust.
**How to avoid:** Check existing pattern in `__init__.py` — `_coerce_set` monkey-patches `BurnerRedis.set` to coerce at Python layer BEFORE calling the Rust method. Do the same for the list push commands that accept values. Rust side uses `extract_bytes` which accepts already-coerced `bytes`/`str`.
**Warning signs:** Piped integer values produce doubled values (`b"42"` becomes `b"b'42'"`).

### Pitfall 7: Forgetting `is_shutdown()` check in BRPOP loop
**What goes wrong:** Python event loop closes; Rust future is parked on `notify.notified()`; `call_soon_threadsafe` fails → panic or hang.
**Why it happens:** Missing the `if store.is_shutdown() { break format_brpop_none(); }` at loop top.
**How to avoid:** Copy the exact shape from `lib.rs:1004-1006`. `shutdown()` also calls `list_notify.notify_waiters()` to wake parked waiters (D-08) — they then see the flag and return.
**Warning signs:** `test_graceful_shutdown.py`-style tests time out when BLPOP waiters don't exit cleanly.

### Pitfall 8: redis-py encoding difference on BLPOP tuple
**What goes wrong:** BLPOP returns `[key, value]` list instead of `(key, value)` tuple; user code that does `key, val = await r.blpop(...)` breaks when user code does `isinstance(r, tuple)`.
**Why it happens:** Python binding returns a `PyList` for BLPOP result; redis-py's callback returns `tuple(r)`.
**How to avoid:** Use `PyTuple::new(py, &[PyBytes::new(py, &key), PyBytes::new(py, &val)])`. [VERIFIED from redis-py source `_parsers/helpers.py:862`]
**Warning signs:** `redis.exceptions.DataError` or `TypeError` on tuple unpacking in pydocket tests.

## Code Examples

### Non-blocking command (LRANGE)

```rust
// In src/lib.rs #[pymethods] — analogous to lib.rs pattern for get/hvals
#[pyo3(signature = (name, start, end))]
fn lrange<'py>(
    &self,
    py: Python<'py>,
    name: &Bound<'py, PyAny>,
    start: i64,
    end: i64,
) -> PyResult<Bound<'py, PyAny>> {
    let key = extract_bytes(name)?;
    let elements = self.store.lrange(&key, start, end).map_err(store_err_to_py)?;
    let py_list: Vec<Vec<u8>> = elements.into_iter().map(|b| b.to_vec()).collect();
    resolved(py, py_list.into_pyobject(py)?.into_any().unbind())
}
```

### Blocking command (BRPOP, multi-key)

```rust
// Template: src/lib.rs:982-1038 (XREAD)
#[pyo3(signature = (keys, timeout=None))]
fn brpop<'py>(
    &self,
    py: Python<'py>,
    keys: &Bound<'py, PyAny>,
    timeout: Option<f64>,
) -> PyResult<Bound<'py, PyAny>> {
    // Normalize keys: accept single str/bytes, list, or tuple
    let key_list: Vec<Bytes> = normalize_keys(keys)?;
    // timeout None or 0 means block forever
    let block_ms: u64 = match timeout {
        None => 0,
        Some(t) if t < 0.0 => return Err(pyo3::exceptions::PyValueError::new_err(
            "timeout must be a non-negative number",
        )),
        Some(t) => (t * 1000.0) as u64,
    };
    let store = self.store.clone();

    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let notify = store.list_notify();
        let mut waiter = Box::pin(notify.notified());
        waiter.as_mut().enable();

        // First non-blocking attempt
        match store.brpop_poll(&key_list).map_err(store_err_to_py)? {
            Some((k, v)) => return format_brpop_result(k, v),
            None => {}
        }

        let deadline_opt = if block_ms == 0 {
            None
        } else {
            Some(tokio::time::Instant::now() + Duration::from_millis(block_ms))
        };

        loop {
            if store.is_shutdown() { return format_brpop_none(); }

            let remaining = match deadline_opt {
                Some(d) => {
                    let r = d.saturating_duration_since(tokio::time::Instant::now());
                    if r.is_zero() { return format_brpop_none(); }
                    r
                }
                None => Duration::from_secs(3600),
            };

            tokio::select! {
                _ = waiter.as_mut() => {
                    waiter.set(notify.notified());
                    waiter.as_mut().enable();
                    if let Some((k, v)) = store.brpop_poll(&key_list).map_err(store_err_to_py)? {
                        return format_brpop_result(k, v);
                    }
                }
                _ = tokio::time::sleep(remaining) => {
                    if deadline_opt.is_some() { return format_brpop_none(); }
                }
            }
        }
    })
}

// helper
fn format_brpop_result(key: Bytes, val: Bytes) -> PyResult<Py<PyAny>> {
    Python::attach(|py| {
        let tup = PyTuple::new(py, &[
            PyBytes::new(py, &key).into_any(),
            PyBytes::new(py, &val).into_any(),
        ])?;
        Ok(tup.into_any().unbind())
    })
}

fn format_brpop_none() -> PyResult<Py<PyAny>> {
    Python::attach(|py| Ok(py.None()))
}
```

### Lua dispatch extension (`src/scripting.rs`)

```rust
// Modify dispatch_command at line 268
fn dispatch_command(
    cmd: &str,
    args: &[Bytes],
    data: &mut HashMap<Bytes, ValueEntry>,
    pubsub_tx: Option<&broadcast::Sender<PubSubMessage>>,
) -> Result<(RedisValue, bool, bool), String> {
    let is_xadd = cmd == "XADD";
    let is_list_write = matches!(cmd,
        "LPUSH" | "RPUSH" | "LMOVE" | "RPOPLPUSH" | "LSET" | "LINSERT");
    let result = dispatch_command_inner(cmd, args, data, pubsub_tx)?;
    let success = !matches!(result, RedisValue::Error(_));
    let had_xadd = is_xadd && success;
    let had_list_mutation = is_list_write && success;
    Ok((result, had_xadd, had_list_mutation))
}

// In dispatch_command_inner — reject blocking commands
"BLPOP" | "BRPOP" | "BLMOVE" => {
    Ok(RedisValue::Error(format!(
        "ERR This Redis command is not allowed from scripts: {}",
        cmd
    )))
}

// Non-blocking: handle LPUSH/RPUSH/etc with standard match arms mirroring HSET/SADD patterns
```

### Pipeline stub (`python/burner_redis/pipeline.py`)

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
    self._commands.append(("blmove", (first_list, second_list, timeout),
                          {"src": src, "dest": dest}))
    return self
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `BRPOPLPUSH src dst timeout` | `BLMOVE src dst LEFT\|RIGHT LEFT\|RIGHT timeout` | Redis 6.2 | redis-py still exposes both; we skip BRPOPLPUSH (D-01). [CITED: redis.io/commands/rpoplpush] |
| `RPOPLPUSH src dst` | `LMOVE src dst RIGHT LEFT` | Redis 6.2 | redis-py keeps RPOPLPUSH as non-deprecated alias; we implement both (D-01). |
| `LPOP key 0` returns nil | `LPOP key 0` returns empty array | Redis 7.x (PRs #9692, #10095) | We mirror post-7.x behavior. [VERIFIED from Redis PR history] |
| `LPOP key N` on missing key returns nil-bulk | `LPOP key N` on missing key returns null-array | Redis 7.x (PR #10095) | Our implementation must handle the missing-key-with-count case as `None`. |
| BLPOP timeout as integer | BLPOP timeout as double | Redis 6.0 | Our Python binding accepts float, converts to milliseconds. [CITED: redis.io/commands/blpop] |

**Deprecated/outdated:**
- `rlua` crate — deprecated in favor of `mlua` (already using mlua).
- `pyo3-asyncio` — replaced by `pyo3-async-runtimes` (already using the new name).
- BRPOPLPUSH — legacy, superseded by BLMOVE. Per D-01, explicitly out of scope.

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | The exact Redis error string for BRPOP-from-Lua is `"ERR This Redis command is not allowed from scripts: BRPOP"` (uppercase command, matching the Redis server's `denyScriptCommand` class) | D-13 / Pattern 3 | Low — if wording differs slightly from real Redis (e.g., lowercase command, extra punctuation), Lua-portability tests fail on exact-string-match. Fix: grep Redis source for "This Redis command is not allowed from scripts" and copy verbatim. No behavior impact. |
| A2 | `had_list_mutation` should fire for LSET and LINSERT when they SUCCEED (not just LPUSH/RPUSH/LMOVE/RPOPLPUSH). LSET can replace an element at a position where the list was previously empty-at-that-index… no, LSET cannot grow a list. LINSERT can insert new elements → can grow. So: LSET does NOT need to fire notify (element count unchanged); LINSERT on success DOES fire. LPOP/RPOP/LREM/LTRIM SHRINK the list and cannot wake a waiter. | Code Examples → Lua dispatch | Medium — if LINSERT doesn't fire, a BRPOP waiter parked on an empty list will miss an LINSERT-from-Lua wake. But LINSERT on an empty list returns 0 (key doesn't exist), and a non-empty list already has elements so no one is parked. Need to verify edge case: can LINSERT ever turn an empty list into non-empty? Answer: No — LINSERT on non-existent key returns 0 and is a no-op. Conclusion: LPUSH/RPUSH/LMOVE(dst)/RPOPLPUSH(dst)/BLMOVE(dst) are the ONLY list-grow ops. LINSERT never grows from empty. Lock this down in the plan. |
| A3 | `VecDeque<Bytes>` performance is adequate for the expected Prefect workload (small lists, < 10K elements) | Standard Stack | Low — Redis's own quicklist is a memory-efficiency optimization for huge lists, not correctness. A 10K-element VecDeque has O(1) push/pop and O(N) index/insert/remove. We already accept O(N) for HSET/HVALS/SMEMBERS; same bound. |
| A4 | The `lmpop`/`blmpop` commands are NOT in scope for Phase 14 (not explicitly in D-01) | Command Surface | Low — D-01 lists the 16 exact commands; LMPOP/BLMPOP are in neither the required nor stretch list. Defer to a future phase. |
| A5 | Existing `_coerce_value` in `__init__.py` correctly handles all list-push value types (bytes, int, float, str, memoryview; rejects bool) | D-19 | Low — function is already verified for SET/HSET; lists accept the same value space. Re-use unchanged. |

**If empty:** Most claims verified; the 5 above need user/planner confirmation.

## Open Questions

1. **Should LMOVE's SAME-key rotation fire `list_notify.notify_waiters()`?**
   - What we know: The list's `len()` is unchanged (pop-one + push-one). No waiter becomes satisfiable.
   - What's unclear: A notification still costs only microseconds, and under-notification is a bug class; over-notification is merely mild wake-storm.
   - Recommendation: Fire unconditionally after any LMOVE/BLMOVE that mutates. Matches "errs-on-the-side-of-wakeups" philosophy. Cost is negligible; correctness is maximally conservative.

2. **Exact Redis error message for LSET out-of-range.**
   - What we know: Redis returns an error; redis-py raises `redis.exceptions.ResponseError`.
   - What's unclear: Exact string — `"ERR index out of range"` is the conventional wording; verify before locking.
   - Recommendation: Test against a live Redis instance OR grep Redis source for `"ERR index out of range"` — use the exact phrase. If fakeredis is in the test suite, use it as the oracle.

3. **Should the Python `BurnerRedis.blpop` accept both `timeout=0` (int) and `timeout=0.0` (float)?**
   - What we know: redis-py accepts `Optional[Number]` which is `Union[int, float]`.
   - What's unclear: The Rust binding type `f64` via `#[pyo3(signature = (..., timeout=None))]` — PyO3 converts Python int to f64 automatically when the Rust arg is `Option<f64>`. Verify with `.extract::<f64>()`.
   - Recommendation: Use `Option<f64>`; PyO3's auto-conversion handles int→float. Add a test for both.

4. **`blocking_wake_latency` budget.**
   - What we know: No existing SLA in the project.
   - What's unclear: Is there a max-latency budget for BRPOP wake-to-return (e.g., 10ms p99)?
   - Recommendation: Set informal budget of < 5ms p99 single-notify wake-to-return on a local test; document but don't gate merge on it in Phase 14.

5. **Does the plan need a Wave 0 test-infrastructure task?**
   - What we know: `tests/conftest.py` already exists, `pytest`/`pytest-asyncio` are in pyproject.toml, and the existing test pattern (`r` fixture) covers all needs.
   - What's unclear: Nothing — no Wave 0 infrastructure gap.
   - Recommendation: No Wave 0 needed. First test file is `tests/test_lists.py` directly.

## Environment Availability

Skipped — Phase 14 is a pure code/config addition to an existing Rust+Python project. No new external tools, runtimes, or services. Existing dev toolchain (`cargo`, `maturin develop`, `pytest`) is already verified by the in-progress phases.

## Validation Architecture

### Test Framework

| Property | Value |
|----------|-------|
| Framework | pytest 8.x + pytest-asyncio (asyncio_mode = auto) |
| Config file | `pyproject.toml` — `[tool.pytest.ini_options]` section (asyncio_mode, addopts) |
| Quick run command | `pytest tests/test_lists.py -x` |
| Full suite command | `pytest tests/ -x` (excludes `-m integration` by default per pyproject config) |
| Reference oracle | `redis.asyncio.Redis` API surface (redis-py installed as dev dependency — `pip install -e .[dev]`) |

### Phase Requirements → Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| LIST-01 | LPUSH single + multi-value insertion order | unit | `pytest tests/test_lists.py::test_lpush_single tests/test_lists.py::test_lpush_multiple_order -x` | ❌ Wave-1 |
| LIST-02 | RPUSH mirror of LPUSH | unit | `pytest tests/test_lists.py::test_rpush_single tests/test_lists.py::test_rpush_multiple_order -x` | ❌ Wave-1 |
| LIST-03 | LPOP no-count returns bytes/None; count returns list/None | unit | `pytest tests/test_lists.py::test_lpop_no_count tests/test_lists.py::test_lpop_with_count tests/test_lists.py::test_lpop_count_zero tests/test_lists.py::test_lpop_missing_key -x` | ❌ Wave-1 |
| LIST-04 | RPOP mirror | unit | `pytest tests/test_lists.py::test_rpop_* -x` | ❌ Wave-1 |
| LIST-05 | LRANGE negative/positive/out-of-range — 9-case matrix | unit | `pytest tests/test_lists.py::test_lrange_normalization -x` (parameterized) | ❌ Wave-1 |
| LIST-06 | LLEN returns 0 for missing, N for present | unit | `pytest tests/test_lists.py::test_llen -x` | ❌ Wave-1 |
| LIST-07 | LINDEX negative + out-of-range → None | unit | `pytest tests/test_lists.py::test_lindex -x` | ❌ Wave-1 |
| LIST-08 | LINSERT returns new length, -1 on missing pivot, 0 on missing key, WRONGTYPE | unit | `pytest tests/test_lists.py::test_linsert -x` | ❌ Wave-1 |
| LIST-09 | LREM count sign — positive, negative, zero matrix | unit | `pytest tests/test_lists.py::test_lrem_count_semantics -x` | ❌ Wave-1 |
| LIST-10 | LSET success + out-of-range error + missing key error | unit | `pytest tests/test_lists.py::test_lset -x` | ❌ Wave-1 |
| LIST-11 | LTRIM preserves range; empty result deletes key | unit | `pytest tests/test_lists.py::test_ltrim -x` | ❌ Wave-1 |
| LIST-12 | LMOVE cross-key + same-key rotation + direction matrix | unit | `pytest tests/test_lists.py::test_lmove -x` | ❌ Wave-1 |
| LIST-13 | RPOPLPUSH alias semantics | unit | `pytest tests/test_lists.py::test_rpoplpush -x` | ❌ Wave-1 |
| LIST-14 | BRPOP/BLPOP — blocking, timeout=0, multi-key order, cancellation, key-deletion on empty | integration | `pytest tests/test_lists.py::test_blpop_* tests/test_lists.py::test_brpop_* -x` | ❌ Wave-1 |
| LIST-15 | BLMOVE — blocking, timeout=0, same-key, cancellation | integration | `pytest tests/test_lists.py::test_blmove_* -x` | ❌ Wave-1 |
| LIST-16 | Pipeline (blocking + non-blocking mix) + Lua dispatch for 13 non-blocking + Lua-reject for 3 blocking + Lua-to-BRPOP wake-up path | integration | `pytest tests/test_lists.py::test_pipeline_lists tests/test_lists.py::test_lua_list_commands tests/test_lists.py::test_lua_brpop_wake -x` | ❌ Wave-1 |

### Critical Behavioral Test Matrix (all in `tests/test_lists.py`)

| Scenario | Expected | Why Critical |
|----------|----------|--------------|
| `r.lpush("k", "a", "b", "c")` then `r.lrange("k", 0, -1)` | `[b"c", b"b", b"a"]` | Multi-value LPUSH pushes each value to head in turn. Common drop-in replacement bug. |
| `r.lpop("k", count=0)` with key present | `[]` | Post-Redis-7 behavior per PR #9692. Drop-in breaks if we return None. |
| `r.lpop("missing", count=5)` | `None` | Post-PR #10095 null-array behavior. |
| `r.blpop(["k1", "k2"], timeout=0.1)` with empty keys | `None` | Timeout expiry — NOT empty tuple. |
| `r.blpop(["k1", "k2"], timeout=0)` then LPUSH on k2 | `(b"k2", b"value")` | Tuple, not list; specific key returned. |
| `r.blpop(["k1", "k2"], timeout=0)` where k2 and k1 both become non-empty simultaneously (via Lua) | `(b"k1", ...)` | Left-to-right scan order. |
| `r.brpop("k", timeout=0)` → cancel asyncio task | asyncio.CancelledError propagates; no list state corruption | Cancellation safety of tokio::select! |
| `r.linsert("k", "BEFORE", "missing_pivot", "new")` on populated list | `-1` | [CITED] |
| `r.lset("k", 100, "v")` on 3-element list | redis.exceptions.ResponseError | [CITED] |
| `r.ltrim("k", 5, 10)` on 3-element list | list deleted (LLEN=0, EXISTS=0) | [CITED] |
| `r.lmove("k", "k", "LEFT", "RIGHT")` rotation | element rotated; result bytes correct | [CITED] |
| `r.rpoplpush("empty", "dst")` | `None` | [CITED] |
| BRPOP while Lua script does `redis.call("LPUSH", ...)` | BRPOP wakes and returns | D-14 correctness |
| Pipeline `[set, blpop(timeout=0.1), set]` | 3 results; blpop returns None on timeout | D-15 pipeline blocking semantics |
| Pipeline all-non-blocking `[lpush, lrange, llen]` | Uses sync fast path (assert via timing) | D-16 perf regression guard |

### Sampling Rate

- **Per task commit:** `pytest tests/test_lists.py -x`
- **Per wave merge:** `pytest tests/ -x`
- **Phase gate:** Full suite green before `/gsd-verify-work`; specifically verify no regression in `tests/test_streams.py` (stream_notify path), `tests/test_scripting.py` (Lua), `tests/test_pipeline.py` (pipeline fast path), `tests/test_graceful_shutdown.py` (shutdown wakes list waiters)

### Wave 0 Gaps

None — existing test infrastructure covers all phase requirements.

- `tests/conftest.py` — existing `r` fixture provides `BurnerRedis()` instance per test. Reused unchanged.
- `pytest-asyncio` — configured via `asyncio_mode = "auto"` in pyproject.toml. No new config.
- `redis` dev dependency — installed via `[project.optional-dependencies.dev]`. Used for reference behavior in compat tests.

## Security Domain

Not applicable in the traditional sense. `burner-redis` is an embedded, in-process database with no network surface, no auth boundary, and no untrusted-data egress. Per REQUIREMENTS.md §"Out of Scope": "Network server / Redis wire protocol — Embedded in-process only" and "ACL / Authentication — Runs in-process, no auth boundary to protect."

However, two ASVS-adjacent concerns for this phase:

| Concern | Applies | Mitigation |
|---------|---------|-----------|
| V5 Input Validation — malformed ID arguments (e.g., `LSET k "not-an-int" v`) | yes | PyO3's `.extract::<i64>()` returns a PyResult that converts to a TypeError; validate via the binding layer, not at the store. Existing pattern in `lib.rs` for all integer args. |
| V6 Cryptography — N/A | no | No cryptographic operations in this phase. |
| V1 Architecture — Memory exhaustion via unbounded list growth | yes (mild) | A malicious in-process caller could LPUSH unbounded. Mitigated by (a) single-process scope — user IS the malicious party by assumption, (b) no DoS boundary to protect. Flag in plan as "informational, not blocker." |
| Race conditions across lock boundaries | yes | Addressed by D-10 (notify inside write lock), D-14 (notify after Lua lock drop). Phase 11 precedent. |

No `ASVS` category enforcement is required for Phase 14. No new attack surface introduced beyond the existing keyspace.

## Project Constraints (from CLAUDE.md)

Extracted from `./CLAUDE.md` — the planner must ensure tasks honor these:

1. **Rust edition 2024, Rust 1.85+.** New module `src/commands/lists.rs` must compile on 2024 edition. [VERIFIED: `Cargo.toml` specifies `edition = "2024"`]
2. **Python 3.10+ target.** `abi3-py310` feature in `Cargo.toml`; new `#[pymethods]` use PyO3 0.28.3 conventions. `Python::try_attach` (or `Python::attach` per 0.28) for GIL re-attach in async blocks. [VERIFIED: Cargo.toml]
3. **`redis.asyncio.Redis` compatibility is the governing discipline.** Every list method's signature must match redis-py exactly (positional vs keyword, defaults). Drop-in or nothing.
4. **`parking_lot::RwLock` for keyspace.** No `std::sync::RwLock`, no `tokio::sync::RwLock`, no `DashMap` — see Alternatives Considered in CLAUDE.md. [LOCKED by D-04]
5. **`mlua` 0.10 with `lua54,send` features.** Already in Cargo.toml; Lua dispatch uses the existing `dispatch_command_inner` harness. Don't add per-command Lua bindings.
6. **`bytes::Bytes` for all byte payloads.** `VecDeque<Bytes>` (not `VecDeque<Vec<u8>>`).
7. **`rmp-serde` persistence format.** New `ValueData::List(VecDeque<Bytes>)` variant picks up existing `Serialize + Deserialize` derive automatically. No persistence code changes required.
8. **`criterion` for benchmarking.** Not needed in Phase 14; existing bench suite is not yet in place per the project state. No regression gate required.
9. **GSD workflow enforcement.** Implementation must go through `/gsd-execute-phase` per CLAUDE.md.
10. **`commit_docs: true` in config.json.** RESEARCH.md will be committed by the orchestrator.

## Sources

### Primary (HIGH confidence)
- Local redis-py installation at `/Users/alexander/.cache/uv/archive-v0/b2CFuwZrXaIDnSNRPmZXY/redis/` — VERIFIED via `grep` and direct Read of `commands/core.py` (method signatures) and `_parsers/helpers.py` (BLPOP/BRPOP response callback at line 862). Source of truth for the Python API surface.
- https://redis.io/commands/blpop/ — CITED (BLPOP multi-key scan order, float timeout since Redis 6, tuple return shape, nil-on-timeout)
- https://redis.io/commands/lrange/ — CITED (negative index normalization, out-of-range behavior, inclusive bounds)
- https://redis.io/commands/lpush/ — CITED (multi-value push order: left-to-right insertion to head)
- https://redis.io/commands/linsert/ — CITED (return values: new length, 0, -1)
- https://redis.io/commands/lrem/ — CITED (count sign semantics, non-existent key = 0, empty list deleted)
- https://redis.io/commands/lmove/ — CITED (same-key rotation, atomicity, nil on missing src)
- https://redis.io/commands/ltrim/ — CITED (empty result deletes key, out-of-range handling)
- https://redis.io/commands/rpoplpush/ — CITED (deprecated since 6.2, superseded by LMOVE)
- https://redis.io/commands/lindex/ — CITED (negative index, nil on out-of-range or missing key)
- https://github.com/redis/redis/pull/9692 — CITED (LPOP count=0 returns empty array, not nil — Redis 7.x behavior)
- https://github.com/redis/redis/pull/10095 — CITED (LPOP count=N on missing key returns null array)
- https://docs.rs/tokio/latest/tokio/sync/struct.Notify.html — CITED (notified() cancel safety, re-arm pattern, notify_waiters semantics)
- `/Users/alexander/dev/prefectlabs/burner-redis/src/lib.rs:980-1038` — VERIFIED (XREAD blocking loop, direct template for BRPOP/BLPOP)
- `/Users/alexander/dev/prefectlabs/burner-redis/src/lib.rs:1284-1340` — VERIFIED (XREADGROUP blocking loop with re-arm at 1322-1325)
- `/Users/alexander/dev/prefectlabs/burner-redis/src/store.rs:118,275-302,1262,2402` — VERIFIED (ValueData enum, stream_notify pattern, notify_waiters call sites)
- `/Users/alexander/dev/prefectlabs/burner-redis/src/scripting.rs:126,274-279` — VERIFIED (had_xadd tracking, dispatch_command tuple shape)
- `/Users/alexander/dev/prefectlabs/burner-redis/Cargo.toml` — VERIFIED (dependency pins)

### Secondary (MEDIUM confidence)
- WebSearch verification of multi-key scan order (matches official docs)
- https://users.rust-lang.org/t/cancel-safety-in-async-and-tokio-select/92381 — CITED for tokio cancel safety discussion

### Tertiary (LOW confidence — flagged)
- Exact Redis "not allowed from scripts" error wording — A1 in Assumptions Log. Recommendation: verify against Redis source before freezing.
- Exact Redis "index out of range" wording for LSET — Open Question #2. Recommendation: verify against a live Redis instance or fakeredis.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — all libraries already in use, versions verified via cargo search
- Architecture (blocking loop pattern): HIGH — direct re-use of Phase 11's XREADGROUP fix, production-proven
- Storage model (VecDeque + ValueData variant): HIGH — proven pattern for Hash/Set/SortedSet/Stream
- redis-py command signatures: HIGH — read directly from installed source
- Redis server-side edge cases (LPOP count=0, LREM sign, LRANGE negative indices): HIGH — triangulated across redis.io docs, GitHub issues/PRs, and redis-py source
- Lua dispatch (had_list_mutation): HIGH — exact pattern analogue of had_xadd
- Pipeline blocking/non-blocking dual-path: HIGH — stated architecture in D-16, existing execute_pipeline structure in lib.rs:2182 is clearly split-ready
- Pitfalls: HIGH — direct observations from existing code and Phase 11 lessons
- Exact error wordings (A1, OQ2): LOW — documented as open questions; planner should resolve before implementation

**Research date:** 2026-04-24
**Valid until:** 2026-05-24 (30 days — ecosystem is stable, no imminent redis-py or tokio breaking changes)
