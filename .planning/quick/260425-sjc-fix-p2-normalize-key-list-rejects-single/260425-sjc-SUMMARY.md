---
phase: quick-260425-sjc
plan: 01
subsystem: pyo3-bindings
tags:
  - rust
  - pyo3
  - python-bindings
  - blocking-list
  - blpop
  - brpop
  - bytes-like
  - normalize_key_list
  - redis-py-compat
requirements:
  - QUICK-260425-sjc
dependency_graph:
  requires:
    - quick-260425-r3r (BLPOP/BRPOP/BLMOVE async-wrapping fix — argument-shape gap left open by r3r is what this closes)
  provides:
    - normalize_key_list scalar guard covering all four redis-py Encoder bytes-like types (bytes / str / bytearray / memoryview)
  affects:
    - All BLPOP / BRPOP call sites that pass a single bytes-like key (memoryview / bytearray)
tech_stack:
  added: []
  patterns:
    - "Bytes-like scalar guard via is_instance_of:: before PySequence dispatch (PyString | PyBytes | PyByteArray as direct extract_bytes; PyMemoryView via .tobytes() materialization)"
key_files:
  created: []
  modified:
    - src/lib.rs
    - tests/test_lists.py
decisions:
  - "memoryview routes through call_method0(\"tobytes\") rather than relying on extract::<Vec<u8>>: PyO3's Vec<u8> extractor uses the buffer protocol directly and is fragile for non-contiguous / non-byte-format buffers; tobytes() always returns a fresh contiguous PyBytes regardless of buffer shape"
  - "Used keys.call_method0(\"tobytes\") on the PyAny directly rather than first downcasting to PyMemoryView — avoids introducing a new pyo3::types::PyAnyMethods::downcast deprecation warning (downcast → cast migration is a separate project-wide cleanup)"
  - "extract_bytes left untouched: its public contract (\"expected str or bytes\") is unchanged; memoryview is materialized at the call site so extract_bytes only ever sees a real PyBytes"
  - "Python wrapper layer (python/burner_redis/__init__.py) untouched: wrappers from quick-260425-r3r remain pure async-bridge shims; key normalization stays in Rust"
metrics:
  duration: ~6 min
  completed: 2026-04-25
commits:
  - 1b25790: fix(quick-260425-sjc) — fix + tests
---

# Quick 260425-sjc: Fix P2 normalize_key_list rejects single bytearray / memoryview keys — Summary

Closes the redis-py compat gap where `r.blpop(memoryview(b"k"))` and `r.blpop(bytearray(b"k"))` raised `TypeError: expected str or bytes` because `normalize_key_list` only special-cased `PyString` / `PyBytes` before falling through to the `PySequence` protocol — which iterated bytes-like scalars per byte (yielding `int`s) and crashed `extract_bytes`.

## What Changed

### `src/lib.rs::normalize_key_list` — function-level diff

```rust
 fn normalize_key_list(keys: &Bound<'_, PyAny>) -> PyResult<Vec<Bytes>> {
-    // str / bytes are sequences too — handle them as a single-key case first.
+    // Bytes-like SCALARS: handle as single-key BEFORE PySequence dispatch.
+    // redis-py's Encoder accepts str / bytes / bytearray / memoryview as
+    // a single key. All four are also valid Python sequences, so without
+    // this guard the PySequence branch below would iterate them per byte
+    // (yielding `int`s) and crash in extract_bytes.
     if keys.is_instance_of::<pyo3::types::PyString>()
         || keys.is_instance_of::<pyo3::types::PyBytes>()
+        || keys.is_instance_of::<pyo3::types::PyByteArray>()
     {
         return Ok(vec![extract_bytes(keys)?]);
     }
+    // memoryview: materialize via .tobytes() for safety against
+    // non-contiguous / non-byte-format buffers, then extract.
+    if keys.is_instance_of::<pyo3::types::PyMemoryView>() {
+        let bytes_obj = keys.call_method0("tobytes")?;
+        return Ok(vec![extract_bytes(&bytes_obj)?]);
+    }
     // Try list/tuple via sequence protocol.
     if let Ok(seq) = keys.downcast::<pyo3::types::PySequence>() {
         …
     }
-    // Fallback: treat as a single key (will error if not str/bytes).
+    // Fallback: treat as a single key (will error if not str/bytes-like).
     Ok(vec![extract_bytes(keys)?])
 }
```

### Decision: why memoryview goes through `.tobytes()`

`extract_bytes` already accepts `bytes` and `bytearray` natively via PyO3's `Vec<u8>` extractor, but PyO3's `Vec<u8>` extractor goes through the buffer protocol directly. That extractor is **only safe for contiguous, byte-format (`B`) buffers** — non-contiguous or non-byte-format `memoryview`s (e.g. a slice of a `numpy.ndarray` of `int32`) either error out or silently mis-extract.

Calling `keys.call_method0("tobytes")` on the memoryview always returns a fresh contiguous `PyBytes` regardless of the buffer's stride, format, or contiguity — Python's standard memoryview-to-bytes contract. We then pass that real `PyBytes` to `extract_bytes`, which means `extract_bytes`'s public contract ("expected str or bytes") is unchanged: callers cannot leak a memoryview into it.

A secondary consideration: a typed `PyMemoryView::tobytes()` accessor does not exist in pyo3 0.28.3, so `call_method0("tobytes")` is the universally-available path. (Confirmed against the pinned pyo3 source under `~/.cargo/registry/src/.../pyo3-0.28.3/src/types/memoryview.rs` — the only public methods are `from()` and `try_from()`.)

A tertiary consideration: I called `call_method0("tobytes")` directly on the `&Bound<'_, PyAny>` rather than first downcasting to `&Bound<'_, PyMemoryView>`. This avoids introducing a new `pyo3::types::PyAnyMethods::downcast` deprecation warning (PyO3 0.28 deprecated `downcast` in favor of `Bound::cast`; the project still has 10 pre-existing call sites using the deprecated form, but a project-wide migration is out of scope for this quick task).

## Tests Added

8 new regression tests appended to `tests/test_lists.py` (line 1078 onward), grouped under the comment header `# ---- P2 regression (260425-sjc): BLPOP/BRPOP single bytes-like scalar keys ----`:

| # | Test name                                          | Shape                | Purpose                          |
|---|----------------------------------------------------|----------------------|----------------------------------|
| 1 | `test_blpop_accepts_single_bytes_scalar`           | `b"k"`               | regression guard (already worked) |
| 2 | `test_blpop_accepts_single_str_scalar`             | `"k"`                | regression guard (already worked) |
| 3 | `test_blpop_accepts_single_memoryview_scalar`      | `memoryview(b"k")`   | **primary failing case before fix** |
| 4 | `test_blpop_accepts_single_bytearray_scalar`       | `bytearray(b"k")`    | **secondary failing case before fix** |
| 5 | `test_blpop_accepts_list_keys_regression`          | `[b"k1", b"k2"]`     | multi-key list regression guard  |
| 6 | `test_blpop_accepts_tuple_keys_regression`         | `(b"k1", b"k2")`     | multi-key tuple regression guard |
| 7 | `test_brpop_accepts_single_memoryview_scalar`      | `memoryview(b"k")`   | BRPOP mirror of (3)              |
| 8 | `test_brpop_accepts_single_bytearray_scalar`       | `bytearray(b"k")`    | BRPOP mirror of (4)              |

Each test asserts `await r.{blpop,brpop}(<scalar>, timeout=0.05) is None` against an empty/nonexistent key — `None` on a short timeout is the assertion.

### TDD gate evidence

- **RED phase** (against pre-fix wheel): tests 3, 4, 7, 8 failed with `TypeError: expected str or bytes`; tests 1, 2, 5, 6 passed (locking the regression surface). Confirmed via `pytest -k "single_memoryview or single_bytearray or ..." -W error::RuntimeWarning` → `4 failed, 4 passed`.
- **GREEN phase** (after fix + `maturin develop --release`): all 8 new tests pass; full `tests/test_lists.py` suite (151 tests) green; broader `tests/` sweep (excluding integration) → 531 passed, 1 skipped, no regressions.

## No Python-Wrapper Changes Needed

`python/burner_redis/__init__.py` (which holds the `_async_blpop` / `_async_brpop` / `_async_blmove` wrappers from quick-260425-r3r) is **unchanged**. Those wrappers do not normalize keys — they pass `keys` straight through to the Rust binding. Confirmed by reading the wrapper bodies:

```python
async def _async_blpop(self, keys, timeout=None):
    return await _original_blpop(self, keys, timeout=timeout)
```

The fix is purely Rust-side, which is correct: key normalization must happen at the binding boundary so the wrapper layer remains a thin async-bridge shim.

## Verification

| Check                                                                  | Result                  |
|------------------------------------------------------------------------|-------------------------|
| `cargo check`                                                          | clean — 11 pre-existing warnings, **0 new warnings** |
| `maturin develop --release`                                            | wheel rebuilt cleanly   |
| `pytest tests/test_lists.py -W error::RuntimeWarning -x`               | **151 passed**          |
| `pytest tests/ -W error::RuntimeWarning -x --ignore=tests/integration` | **531 passed, 1 skipped** |
| Smoke test (4-line one-liner from plan)                                | all 4 lines print `None` |

## Deviations from Plan

**None — plan executed exactly as written**, with one minor cleanup that the plan explicitly permits:

The plan's reference implementation showed `let mv = keys.downcast::<PyMemoryView>()?; let bytes_obj = mv.call_method0("tobytes")?;`. I collapsed that to `let bytes_obj = keys.call_method0("tobytes")?;` — `call_method0` is defined on `PyAnyMethods` and works without a typed downcast, since we've already proven it's a memoryview via `is_instance_of::<PyMemoryView>()`. This avoids introducing a new `downcast`-deprecation warning (the plan's `<done>` criterion #1 requires "no warnings introduced"). The plan's <action> block explicitly says "pick the one that compiles cleanly" — this picks the cleaner-warning variant.

Net effect identical: memoryview → tobytes() → real PyBytes → extract_bytes. Buffer-protocol robustness preserved.

## Predecessor / Audit Pointer

- redis-py P2 compat audit: identified two BLPOP/BRPOP gaps — async-wrapping (closed by quick-260425-r3r) and argument-shape (closed by **this** task). The audit also flagged the BLMOVE async-wrapping case (closed by r3r). With this task merged, the BLPOP/BRPOP/BLMOVE redis-py argument-shape compat surface for blocking list ops is fully covered by `normalize_key_list` (single-key path) + the existing PySequence branch (multi-key path).
- Predecessor: quick-260425-r3r (commit `3b94835`) — wrapped `blpop` / `brpop` / `blmove` as `async def` coroutines for `redis.asyncio` compat; left the argument-shape gap that this task closes.

## Self-Check: PASSED

- `src/lib.rs` modified: FOUND (`git show HEAD --stat` lists `src/lib.rs | 13 ++++++++++++-`)
- `tests/test_lists.py` modified: FOUND (`git show HEAD --stat` lists `tests/test_lists.py | 51 +++++++++++++++++++++++++++++++++++++++++++++++++++`)
- Commit `1b25790` exists: FOUND (verified via `git rev-parse --short HEAD`)
- All 8 new tests pass against built wheel: FOUND (verified via `pytest -k`)
- `normalize_key_list` source contains explicit `PyByteArray` and `PyMemoryView` checks: FOUND (verified via `grep` on src/lib.rs)
- `src/commands/strings.rs::extract_bytes` unchanged: FOUND (no diff to that file)
- `python/burner_redis/__init__.py` unchanged: FOUND (no diff to that file)
