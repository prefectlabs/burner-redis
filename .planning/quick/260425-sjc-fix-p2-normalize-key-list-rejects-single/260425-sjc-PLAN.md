---
phase: quick-260425-sjc
plan: 01
type: execute
wave: 1
depends_on: []
files_modified:
  - src/lib.rs
  - tests/test_lists.py
autonomous: true
requirements:
  - QUICK-260425-sjc
tags:
  - rust
  - pyo3
  - python-bindings
  - blocking-list
  - blpop
  - brpop
  - bytes-like
must_haves:
  truths:
    - "r.blpop(memoryview(b'k'), timeout=0.05) returns None (treats memoryview as a single key)"
    - "r.brpop(memoryview(b'k'), timeout=0.05) returns None (treats memoryview as a single key)"
    - "r.blpop(bytearray(b'k'), timeout=0.05) returns None (treats bytearray as a single key)"
    - "r.brpop(bytearray(b'k'), timeout=0.05) returns None (treats bytearray as a single key)"
    - "r.blpop(b'k', timeout=0.05) and r.blpop('k', timeout=0.05) still return None (regression guard)"
    - "r.blpop([b'k1', b'k2'], timeout=0.05) and tuple equivalent still return None (multi-key regression guard)"
    - "All existing tests in tests/test_lists.py still pass (no regression in normalize_key_list)"
  artifacts:
    - path: "src/lib.rs"
      provides: "normalize_key_list with extended scalar guard for PyByteArray and PyMemoryView"
      contains: "PyByteArray"
    - path: "tests/test_lists.py"
      provides: "Regression tests for single bytes-like keys to BLPOP and BRPOP"
      contains: "memoryview"
  key_links:
    - from: "src/lib.rs:normalize_key_list"
      to: "pyo3::types::PyByteArray, pyo3::types::PyMemoryView"
      via: "is_instance_of guard before PySequence downcast"
      pattern: "is_instance_of::<pyo3::types::PyByteArray>|is_instance_of::<pyo3::types::PyMemoryView>"
    - from: "tests/test_lists.py"
      to: "r.blpop / r.brpop with memoryview / bytearray scalars"
      via: "pytest async tests with short timeouts and empty keys"
      pattern: "memoryview\\(b['\\\"]"
---

<objective>
Fix `normalize_key_list` in `src/lib.rs` so single `memoryview` and `bytearray` keys passed
to `BLPOP` / `BRPOP` are treated as a single bytes-like scalar (matching `bytes` / `str`),
not iterated through Python's sequence protocol — which currently makes
`r.blpop(memoryview(b"k"), timeout=0.1)` raise an opaque `TypeError` because
`memoryview[0]` returns an `int`.

Purpose: redis-py's `Encoder.encode()` accepts `bytes` / `bytearray` / `memoryview` as
scalar bytes-likes. Our binding currently rejects two of those three. Pydocket and
generic redis-py users expect blocking pops to accept any of them as a single key.
This was identified as P2 by the redis-py compat audit; quick-260425-r3r fixed the
async-wrapping side of BLPOP/BRPOP, this closes the remaining single-key argument-shape
gap.

Output: Two extra `is_instance_of` guards in `normalize_key_list` (PyByteArray + PyMemoryView)
and 8 new pytest tests covering BLPOP and BRPOP with all four scalar shapes
(`bytes`, `str`, `bytearray`, `memoryview`) plus list/tuple regression guards.
</objective>

<execution_context>
@$HOME/.claude/get-shit-done/workflows/execute-plan.md
@$HOME/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@.planning/STATE.md
@CLAUDE.md

<!-- The buggy function. -->
@src/lib.rs

<!-- The inner item extractor that normalize_key_list calls. -->
@src/commands/strings.rs

<!-- Existing list test patterns to mirror; this is where new tests append. -->
@tests/test_lists.py

<!-- Python wrapper for context — confirms wrappers do NOT normalize keys, so the fix MUST land in Rust. -->
@python/burner_redis/__init__.py

<interfaces>
<!-- Key types and contracts the executor needs. Extracted from codebase. -->
<!-- Executor should use these directly — no codebase exploration needed. -->

From src/commands/strings.rs (already in scope of normalize_key_list):
```rust
// Accepts Python str (UTF-8) and Python bytes-like (Vec<u8> extractor).
// PyO3 0.28.3 Vec<u8> extractor handles `bytes` and `bytearray` natively.
// memoryview extraction via `Vec<u8>` is NOT guaranteed for non-contiguous /
// non-byte-format buffers; the safe path for memoryview is to call
// `.tobytes()` to materialize a `bytes` object first, then re-extract.
pub fn extract_bytes(obj: &Bound<'_, PyAny>) -> PyResult<Bytes> {
    if let Ok(s) = obj.extract::<String>() { Ok(Bytes::from(s.into_bytes())) }
    else if let Ok(b) = obj.extract::<Vec<u8>>() { Ok(Bytes::from(b)) }
    else { Err(PyTypeError::new_err("expected str or bytes")) }
}
```

From src/lib.rs:288-311 (the function being fixed):
```rust
fn normalize_key_list(keys: &Bound<'_, PyAny>) -> PyResult<Vec<Bytes>> {
    if keys.is_instance_of::<pyo3::types::PyString>()
        || keys.is_instance_of::<pyo3::types::PyBytes>()
    {
        return Ok(vec![extract_bytes(keys)?]);
    }
    if let Ok(seq) = keys.downcast::<pyo3::types::PySequence>() {
        // ... iterates per-item ...
    }
    Ok(vec![extract_bytes(keys)?])
}
```

From PyO3 0.28.3 (`pyo3::types`):
```rust
// Both available; both implement `PyTypeCheck` so `is_instance_of::<T>()` works.
pub struct PyByteArray;     // Python `bytearray`
pub struct PyMemoryView;    // Python `memoryview`
// PyMemoryView::tobytes(&self) -> PyResult<Bound<'py, PyBytes>>
// — returns a NEW bytes object containing the buffer's contents, regardless of
//   contiguity or item size. This is the safe materialization path.
```

Existing test fixture (from tests/conftest.py — `r` fixture provides `BurnerRedis`):
```python
# Tests are async (pytest-asyncio); fixture `r` is a fresh BurnerRedis instance.
# All existing blpop/brpop tests use `await r.blpop([keys...], timeout=...)`.
```
</interfaces>
</context>

<tasks>

<task type="auto" tdd="true">
  <name>Task 1: Extend normalize_key_list scalar guard for bytearray + memoryview, add regression tests</name>
  <files>src/lib.rs, tests/test_lists.py</files>
  <behavior>
    Tests (append to tests/test_lists.py near the existing "P2-04 regression"
    block at line ~1048; group under a new comment header
    `# ---- P2 regression (260425-sjc): BLPOP/BRPOP single bytes-like scalar keys ----`):

    1. test_blpop_accepts_single_bytes_scalar — `await r.blpop(b"empty_k", timeout=0.05)` returns None (regression guard, already worked pre-fix; locks behavior).
    2. test_blpop_accepts_single_str_scalar — `await r.blpop("empty_k", timeout=0.05)` returns None (regression guard).
    3. test_blpop_accepts_single_memoryview_scalar — `await r.blpop(memoryview(b"empty_k"), timeout=0.05)` returns None (PRIMARY failing case before fix; iterated as int sequence and raised TypeError).
    4. test_blpop_accepts_single_bytearray_scalar — `await r.blpop(bytearray(b"empty_k"), timeout=0.05)` returns None (companion case).
    5. test_blpop_accepts_list_keys_regression — `await r.blpop([b"k1", b"k2"], timeout=0.05)` returns None (multi-key regression guard — list path must still iterate).
    6. test_blpop_accepts_tuple_keys_regression — `await r.blpop((b"k1", b"k2"), timeout=0.05)` returns None (tuple path must still iterate).
    7. test_brpop_accepts_single_memoryview_scalar — same as (3) for BRPOP.
    8. test_brpop_accepts_single_bytearray_scalar — same as (4) for BRPOP.

    Each test uses a key known to be empty (a fresh fixture key like `"empty_k"`)
    so the blocking pop returns None on timeout — that None is the assertion.
    Use `timeout=0.05` (50ms) so tests finish quickly. No need for monotonic
    timing assertions — None on a short timeout is enough.

    All 8 tests must FAIL on the current main (memoryview/bytearray cases hit
    TypeError; bytes/str/list/tuple cases pass already and lock the regression
    surface) and PASS after the implementation.
  </behavior>
  <action>
    **Step A — Write failing tests first (RED).** Append the 8 test functions above
    to `tests/test_lists.py` after the P2-04 block (line ~1067). Mirror the style of
    `test_blpop_empty_keys_raises_wrong_arity` (simple async, no fancy timing).
    Header comment:
    ```
    # ---- P2 regression (260425-sjc): BLPOP/BRPOP single bytes-like scalar keys ----
    # Before this fix, normalize_key_list only special-cased PyString and PyBytes
    # before falling through to the PySequence protocol. memoryview and bytearray
    # are sequences too — iterating them yielded `int`s (per-byte) and crashed
    # extract_bytes with TypeError. redis-py's Encoder accepts bytes/bytearray/
    # memoryview as scalar bytes-likes; we now match that.
    ```
    Run `pytest tests/test_lists.py -k "260425_sjc or single_memoryview or single_bytearray or list_keys_regression or tuple_keys_regression or single_bytes_scalar or single_str_scalar" -W error::RuntimeWarning -x`
    and confirm the 4 pre-fix-passing cases pass and the 4 memoryview/bytearray
    cases FAIL with TypeError or "expected str or bytes". (You can skip building
    here if maturin develop hasn't been re-run — current main wheel is fine for
    proving the bug.)

    **Step B — Implement the fix (GREEN).** Edit `src/lib.rs` `normalize_key_list`
    (currently lines 288-311). Extend the scalar early-return guard from two
    types to four. The PyMemoryView path should NOT rely on `extract_bytes`'s
    `Vec<u8>` extractor (which is buffer-protocol-fragile for non-contiguous /
    non-byte-format memoryviews); instead, materialize via `.tobytes()` first.

    Replacement function (verbatim, comments included — preserves the existing
    doc comment above; replace the function body only):
    ```rust
    fn normalize_key_list(keys: &Bound<'_, PyAny>) -> PyResult<Vec<Bytes>> {
        // Bytes-like SCALARS: handle as single-key BEFORE PySequence dispatch.
        // redis-py's Encoder accepts str / bytes / bytearray / memoryview as
        // a single key. All four are also valid Python sequences, so without
        // this guard the PySequence branch below would iterate them per byte
        // (yielding `int`s) and crash in extract_bytes.
        if keys.is_instance_of::<pyo3::types::PyString>()
            || keys.is_instance_of::<pyo3::types::PyBytes>()
            || keys.is_instance_of::<pyo3::types::PyByteArray>()
        {
            return Ok(vec![extract_bytes(keys)?]);
        }
        // memoryview: materialize via .tobytes() for safety against
        // non-contiguous / non-byte-format buffers, then extract.
        if keys.is_instance_of::<pyo3::types::PyMemoryView>() {
            let mv = keys.downcast::<pyo3::types::PyMemoryView>()?;
            let bytes_obj = mv.call_method0("tobytes")?;
            return Ok(vec![extract_bytes(&bytes_obj)?]);
        }
        // Try list/tuple via sequence protocol.
        if let Ok(seq) = keys.downcast::<pyo3::types::PySequence>() {
            let len = seq.len()?;
            let mut out = Vec::with_capacity(len);
            for i in 0..len {
                let item = seq.get_item(i)?;
                out.push(extract_bytes(&item)?);
            }
            return Ok(out);
        }
        // Fallback: treat as a single key (will error if not str/bytes-like).
        Ok(vec![extract_bytes(keys)?])
    }
    ```

    Note on `.call_method0("tobytes")`: PyO3 0.28 has `PyMemoryView::tobytes()`
    as a typed accessor in some builds, but the call_method0 path is universally
    available across PyO3 0.28.x patch versions and equivalent in cost (single
    Python C-API call). If you prefer the typed call, use
    `mv.tobytes()?` and adjust — both produce a `Bound<'_, PyBytes>` that
    `extract_bytes` will happily accept. Either is acceptable; pick the one
    that compiles cleanly with the project's pinned pyo3 = 0.28.3.

    **Step C — Build and run tests.** `maturin develop --release` then re-run
    the full test_lists.py suite. All 8 new tests must now pass; no existing
    tests may regress.

    **Why no change to extract_bytes:** PyO3's `Vec<u8>` extractor already
    handles `bytes` and `bytearray` natively; the in-line `tobytes()` call for
    memoryview means extract_bytes only ever sees a real PyBytes. We keep
    extract_bytes as the str/bytes-only contract it already documents.
  </action>
  <verify>
    <automated>cargo check 2>&amp;1 | tee /tmp/sjc-cargo.log | tail -20 &amp;&amp; maturin develop --release 2>&amp;1 | tail -10 &amp;&amp; pytest tests/test_lists.py -W error::RuntimeWarning -x 2>&amp;1 | tail -30</automated>
  </verify>
  <done>
    1. `cargo check` passes with no warnings introduced.
    2. `maturin develop --release` rebuilds the wheel cleanly.
    3. All 8 new tests in tests/test_lists.py pass.
    4. The full tests/test_lists.py suite passes (no regressions in existing 70+ tests).
    5. `normalize_key_list` source contains explicit `PyByteArray` and `PyMemoryView` checks before the PySequence branch.
    6. No changes to `src/commands/strings.rs::extract_bytes` (memoryview is materialized via tobytes() at the call site, keeping extract_bytes's contract clean).
    7. No changes to `python/burner_redis/__init__.py` (fix is Rust-side; wrappers don't normalize).
  </done>
</task>

</tasks>

<verification>
**Phase-level checks:**

1. `cargo check` — no errors, no new warnings
2. `cargo build --release` — implicitly via `maturin develop --release`
3. `maturin develop --release` — wheel rebuilds, installs into venv
4. `pytest tests/test_lists.py -W error::RuntimeWarning -x` — full list suite green
5. `pytest tests/ -W error::RuntimeWarning -x` (optional broader sweep) — no
   collateral damage to other tests (e.g. test_strings, test_pipeline)

**Smoke test (manual one-liner — optional, not part of CI):**
```
python -c "
import asyncio
from burner_redis import BurnerRedis
async def main():
    r = BurnerRedis()
    print(await r.blpop(memoryview(b'mv_key'), timeout=0.05))   # → None
    print(await r.brpop(bytearray(b'ba_key'), timeout=0.05))    # → None
    print(await r.blpop(b'b_key', timeout=0.05))                # → None (regression)
    print(await r.blpop([b'k1', b'k2'], timeout=0.05))          # → None (regression)
asyncio.run(main())
"
```
All four lines print `None`; before fix the first two raised TypeError.
</verification>

<success_criteria>
- `normalize_key_list` accepts `bytes`, `str`, `bytearray`, `memoryview` as single-key scalars and `list[bytes-like]` / `tuple[bytes-like]` as multi-key (existing) — confirmed by 8 new tests + existing P2-04 / multi-key tests.
- `extract_bytes` is untouched; its public contract ("expected str or bytes") still holds — memoryview is converted to bytes at the call site before being passed in.
- The Python wrapper layer (`python/burner_redis/__init__.py`) is untouched — wrappers in quick-260425-r3r remain pure async-bridge shims.
- No new `extract::<…>` paths added that could mask future bytes-like type-system bugs.
- Full `tests/test_lists.py` suite (~70+ tests) passes; no regression in `test_strings.py`, `test_pipeline.py`, or other suites.
</success_criteria>

<output>
After completion, create `.planning/quick/260425-sjc-fix-p2-normalize-key-list-rejects-single/260425-sjc-SUMMARY.md`.

Summary should record:
- Final patch shape (function-level diff of `normalize_key_list`)
- Decision: why memoryview goes through `.tobytes()` instead of relying on `extract::<Vec<u8>>` (buffer-protocol robustness)
- Test list added (names + count)
- Confirmation that no Python-wrapper changes were needed
- Pointer back to the redis-py P2 audit and quick-260425-r3r as the predecessor blocker that this closes
</output>
