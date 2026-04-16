---
quick_id: 260416-cea
completed: 2026-04-16
commits:
  - a37e299 test(quick-260416-cea): add failing tests for xread(block=N) blocking
  - ab2ab1d fix(quick-260416-cea): implement xread(block=N) with stream_notify wakeup
  - 615a503 test(quick-260416-cea): add failing tests for xread '$' stream ID
  - 48d2723 fix(quick-260416-cea): accept '$' as stream ID in xread (resolved at call time)
  - c903816 test(quick-260416-cea): add failing tests for xinfo_stream
  - f3fbb84 fix(quick-260416-cea): add xinfo_stream method with str-keyed dict
files_modified:
  - src/lib.rs
  - src/store.rs
  - tests/test_streams.py
tests_added: 18
tests_passing: 372 (+ 1 skipped, 30 deselected)
---

# Quick 260416-cea: Fix three redis-py stream compat gaps (XREAD block, XREAD $, XINFO STREAM)

Closed the three remaining stream-API gaps between BurnerRedis and redis-py so that downstream consumers (pydocket, Prefect) can drop their workarounds.

## Gaps closed

1. **`xread(block=N)` now blocks and wakes on xadd** — mirrors the existing
   `xreadgroup(block=N)` plumbing using `store.stream_notify()` +
   `tokio::select!` on `notify.notified()` vs a `tokio::time::sleep` deadline.
   `block=None` preserves the sync fast path; `block=0` blocks indefinitely.
2. **`xread` accepts `$` (and `b'$'`) as a stream ID** — resolved to
   `stream.last_id` at call time (not wakeup time) via new
   `Store::stream_last_id`. Missing streams resolve to `(0, 0)`.
   `xreadgroup` still rejects `$` with the Redis-canonical
   `ERR the $ ID meaning is only valid within XREAD`.
3. **`BurnerRedis.xinfo_stream(name)` added** — returns a str-keyed dict
   matching redis-py: `length`, `radix-tree-keys`, `radix-tree-nodes`,
   `last-generated-id`, `groups`, `first-entry`, `last-entry`. Missing key
   raises `redis.exceptions.ResponseError('ERR no such key ...')`; wrong type
   raises `WRONGTYPE`.

## Files changed and why

- **`src/store.rs`**
  - Added `Store::stream_last_id(&Bytes) -> Option<StreamId>` so lib.rs can
    resolve `$` at call time without exposing store internals.
  - Added `pub struct XInfoStreamSnapshot` (length, last_id, groups_count,
    first_entry, last_entry).
  - Added `Store::xinfo_stream(&Bytes) -> Result<Option<_>, StoreError>`:
    `Ok(None)` = missing/expired key (becomes "no such key" at lib.rs),
    `Err(WrongType)` = non-stream value, `Ok(Some(..))` = populated snapshot.

- **`src/lib.rs`**
  - Refactored `xread` pymethod: extracted `build_xread_pylist` (GIL version)
    and `format_xread_result` (tokio-async version). Added the blocking path
    mirroring `xreadgroup` (see task-decision note below).
  - Removed `#[allow(unused_variables)]` from the `block` parameter — it's
    now meaningful.
  - Added `$` branch to the id-parse loop in both the top-level `xread`
    pymethod and the pipeline `xread` dispatch.
  - Added explicit `$` rejection at the top of `xreadgroup` id-parse loop.
  - Added `xinfo_stream` pymethod + `build_xinfo_stream_dict` helper.
  - Re-exported `XInfoStreamSnapshot` in the `use store::` line.

- **`tests/test_streams.py`**
  - New section `# --- XREAD '$' ID ---` (6 tests)
  - New section `# --- XREAD Blocking ---` (6 tests)
  - New section `# --- XINFO STREAM ---` (6 tests)

## DRY decision for xread / xreadgroup blocking loops

**Kept as parallel-and-commented duplication.** A shared helper was evaluated
and rejected:

- The two closures have different return semantics: `xread` returns `None` on
  empty results (to match the sync fast path), `xreadgroup` returns an empty
  list. Collapsing the two would force the helper to know about the
  "None-on-empty" convention, effectively leaking shape back into the helper.
- The Store call sites (`store.xread(&keys, &ids, count)` vs
  `store.xreadgroup(&group, &consumer, &keys, &id_strs, count)`) have
  different arities, forcing `Box<dyn FnMut(...) -> ...>` trait gymnastics
  that would obscure more than they share.

Instead, both blocking loops carry a comment referencing each other so that
future maintainers updating one remembers to update the other.

## Deviation from plan

The plan called for Task 2 tests to all be RED before implementation.
Reality: the `$` resolution in `src/lib.rs xread` was most naturally written
inside the same id-parse loop I was editing for Task 1's blocking path, so
5 of 6 `$` tests already passed when Task 2 tests were committed. Only the
`xreadgroup` rejection test was still RED. I documented this in the Task 2
RED commit message and kept the commit chain (test -> fix per gap) to match
the 522cd3c/a15cfad convention. The Task 2 "fix" commit therefore contains
the remaining pieces: explicit `$` rejection in xreadgroup and pipeline
xread `$` support.

## New tests (18 total)

**XREAD blocking (6):**
- `test_xread_block_returns_new_entries` — concurrent xadd wakes the reader
  < 1s.
- `test_xread_block_timeout_returns_empty` — returns `None` after timeout.
- `test_xread_block_none_is_non_blocking` — sync fast path < 50ms.
- `test_xread_block_yields_to_event_loop` — tick counter advances >= 5 during
  the block (proves no GIL starvation).
- `test_xread_block_zero_blocks_until_data` — `block=0` returns new entry
  without timing out.
- `test_xread_block_multiple_streams` — xadd into any of the watched streams
  wakes the reader.

**XREAD `$` (6):**
- `test_xread_dollar_id_returns_only_new_entries` — `$` excludes pre-call
  entries.
- `test_xread_dollar_id_as_bytes` — `b'$'` works identically to `'$'`.
- `test_xread_dollar_id_non_blocking_returns_none` — `$` + no block +
  no new data -> None.
- `test_xread_dollar_id_on_missing_stream` — `$` on a non-existent stream
  resolves to (0, 0) and returns None (does not raise).
- `test_xread_dollar_id_resolved_at_call_time` — `$` pinned at call time, not
  re-resolved on wakeup.
- `test_xreadgroup_dollar_id_still_rejected` — `$` remains xread-only.

**XINFO STREAM (6):**
- `test_xinfo_stream_basic` — str keys, length/last-generated-id/groups/
  first-entry/last-entry/radix-tree-* all present.
- `test_xinfo_stream_multiple_entries` — length and first/last reflect 3
  entries.
- `test_xinfo_stream_with_groups` — groups count reflects 2 groups.
- `test_xinfo_stream_empty_stream` — `mkstream=True` empty: length 0,
  first/last None, groups 1.
- `test_xinfo_stream_missing_key_raises` — ResponseError "no such key".
- `test_xinfo_stream_wrong_type_raises` — WRONGTYPE.

## Verification

- `uv run pytest tests/ -q` -> 372 passed, 1 skipped, 30 deselected.
- `uv run pytest tests/test_streams.py -q` -> 93 passed.
- `uv run maturin develop --release` -> builds clean (no new warnings; the
  10 pre-existing dead-code warnings are unrelated to this change).

## Downstream impact

pydocket and Prefect can now drop all three special-case workarounds for
BurnerRedis stream behavior — `xread(block=N)`, `xread({'s': '$'})`, and
`client.xinfo_stream(...)` all behave exactly like `redis.asyncio.Redis`.
No known stream-API compatibility gap remains.

## Self-Check: PASSED

- All 6 planned commits present in git log (a37e299, ab2ab1d, 615a503,
  48d2723, c903816, f3fbb84).
- `src/lib.rs`, `src/store.rs`, `tests/test_streams.py` all modified.
- All 18 new tests pass; full suite of 372 passes.
