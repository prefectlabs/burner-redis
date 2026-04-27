---
phase: 14-add-support-for-the-redis-list-data-type-required-commands-l
type: code-review-external
status: open
reviewed: 2026-04-25
depth: focused
provenance: external review (post-CI-fix follow-up)
files_reviewed:
  - python/burner_redis/__init__.py
  - python/burner_redis/pipeline.py
  - src/lib.rs
  - src/store.rs
tally:
  critical: 0
  high: 0
  medium: 0
  low: 0
  p2: 7
---

# Phase 14 External Code Review: Redis List Data Type (P2 Round)

**Summary:** The patch adds substantial list support, but several Redis-compatibility
edge cases produce wrong results, skipped pipeline commands, or indefinite hangs.
These are user-visible behavioral bugs not covered by the new tests.

All findings are severity **P2** (medium-priority correctness gaps in Redis-py
parity — not crashes, but observable divergence from real Redis behavior).

---

## P2-01 — Continue executing blocking pipelines after errors

**File:** `python/burner_redis/pipeline.py:98-100`

**Problem:** When a pipeline contains `blpop`/`brpop`/`blmove`, the slow path raises
immediately on the first command error if `raise_on_error=True`. That differs from
the fast path and `redis-py` semantics: later queued commands are never executed.

**Example:**
```python
pipe.blpop(...).lset('missing', 0, 'x').set('after', '1')
# Currently leaves "after" unset; should execute all commands and then raise the
# first error.
```

**Fix:** In the blocking-aware branch, capture errors per-command into the results
list (mirroring the fast path) and only raise after all commands have been
attempted, raising the first captured error if `raise_on_error=True`.

---

## P2-02 — Return nil for empty LMOVE sources before checking dst type

**File:** `src/store.rs:3241-3244`

**Problem:** When the source key is missing or empty and the destination exists with
a non-list type, Redis returns nil/no-op for `LMOVE`/`RPOPLPUSH` because no element
is moved. The current pre-check raises `WRONGTYPE` before checking whether the
source can produce an element.

**Example:**
```python
await r.set('string_dst', 'x')
await r.rpoplpush('missing', 'string_dst')
# Currently errors WRONGTYPE; should return None (nil).
```

**Fix:** Reorder checks. First confirm the source has a poppable element (return
`None` if missing/empty). Only validate the destination once there is a value to
move. Keep the destination type-check just before the push to preserve atomicity.

---

## P2-03 — Preserve finite sub-millisecond blocking timeouts

**File:** `src/lib.rs:322`

**Problem:** For any positive timeout below 1ms, `(t * 1000.0) as u64` truncates to
`0`, and the blocking list commands interpret `0` as "block forever". A call such
as `await r.blpop(['k'], timeout=0.0005)` therefore never times out unless data
arrives.

**Fix:** Round positive durations up to at least one millisecond (e.g. `((t *
1000.0).ceil() as u64).max(1)`), or use `Duration::from_secs_f64` and pass through
the `Option<Duration>` directly, keeping exact zero (or `None`) as the only
infinite-timeout signal.

---

## P2-04 — Reject empty key lists for BLPOP/BRPOP

**File:** `src/lib.rs:300-306` (likely `normalize_key_list` or its caller)

**Problem:** If `keys` is an empty list or tuple, normalization returns an empty
vector. `blpop([], timeout=0)` / `brpop([], timeout=0)` then enters a wait loop
that can never be satisfied; finite timeouts return `None`. Redis treats blocking
pops with no keys as a wrong-number-of-arguments error.

**Fix:** Validate that the normalized key list is non-empty before starting the
blocking future. Raise a `redis-py`-compatible error (likely `ResponseError`
mapping to `ERR wrong number of arguments for 'blpop' command` or similar).

---

## P2-05 — Reject LPUSH/RPUSH without values

**File:** `src/store.rs:2779-2788` (and corresponding pipeline arm)

**Problem:** When `values` is empty, direct and pipeline `lpush`/`rpush` create a
persistent empty list key, notify waiters, and return `0`. Redis rejects
`LPUSH`/`RPUSH` with only a key as a wrong-number-of-arguments error and leaves
the key absent.

**Example:**
```python
await r.lpush('k')
await r.exists('k')  # currently returns 1; should remain 0
```

**Fix:** In `Store::lpush`/`Store::rpush`, return early with a `ResponseError` (or
equivalent) when `values` is empty — before any mutation or notify. Mirror the
same arity check in the pipeline arms. Audit Lua dispatch for the same gap.

---

## P2-06 — Coerce the LINSERT pivot value

**File:** `python/burner_redis/__init__.py:123-125` (pipeline stub also affected)

**Problem:** `redis-py` encodes every command argument, including the LINSERT
`refvalue` pivot, so numeric pivots are legal. The current wrapper forwards
`refvalue` raw.

**Example:**
```python
await r.rpush('k', 42)
await r.linsert('k', 'AFTER', 42, 'x')
# Currently raises TypeError; should match b'42' and insert.
```

**Fix:** Apply `_coerce_value(refvalue)` in the LINSERT monkey-patch wrapper
alongside the value being inserted. Apply the same coercion in
`python/burner_redis/pipeline.py`'s `linsert` stub.

---

## P2-07 — Coerce LREM values before extracting bytes

**File:** `src/lib.rs:2407-2409` (and Python wrappers)

**Problem:** The new LREM binding extracts `value` as only `str`/`bytes` and no
Python wrapper coerces it, so `await r.lrem('k', 0, 42)` fails with `TypeError`.
`redis-py` encodes ints/floats for `value` arguments just like LPUSH and LSET.

**Fix:** Add `_coerce_value` to the LREM Python wrapper for both direct and
pipeline calls. Alternatively (or additionally) widen the PyO3 extraction to
accept `int`/`float`/`bool` and coerce at the boundary (matches LPUSH/LSET
treatment).

---

## Notes

- Findings are P2 (medium): observable Redis-compat divergence, not crashes or
  data-loss bugs. Each is independently testable.
- Tests should be added alongside fixes — these are exactly the edge cases the
  Phase 14 test suite missed.
- Apply on `phase-14-redis-lists` branch; commit prefix `fix(14): P2-NN ...`
  to match the established pattern (M-01..M-05, H-01).
