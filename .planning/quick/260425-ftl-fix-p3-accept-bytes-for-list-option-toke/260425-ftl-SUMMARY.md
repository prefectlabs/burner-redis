---
phase: 260425-ftl
plan: 01
subsystem: list-commands / pyo3-binding
tags: [compat, redis-py, pyo3, lists, drop-in]
dependency_graph:
  requires:
    - "Phase 14 list command set (LINSERT / LMOVE / BLMOVE pymethods + pipeline dispatch)"
    - "extract_bytes helper pattern in src/commands/strings.rs"
  provides:
    - "extract_token_str(obj) -> PyResult<String> — single-source-of-truth str-or-bytes decoder for list option tokens"
    - "P3-list-option-tokens-accept-bytes requirement satisfied"
  affects:
    - "linsert / lmove / blmove pymethods (signature change: option-token params now take Bound<PyAny>)"
    - "dispatch_pipeline_command linsert / lmove arms (kwargs now decode via extract_token_str)"
tech_stack:
  added: []
  patterns:
    - "Option<&Bound<'py, PyAny>> with default fallback (replaces str-default in pyo3 signature when the param needs Bound)"
    - "Lossy UTF-8 fallback feeding into the existing parse_* syntax-error path (no TypeError leak)"
key_files:
  created: []
  modified:
    - src/commands/strings.rs
    - src/lib.rs
    - tests/test_lists.py
decisions:
  - "extract_token_str returns owned String (not &str) — needed because bytes path allocates; matches extract_bytes ownership model"
  - "Invalid UTF-8 routes through from_utf8_lossy → parse_linsert_where/parse_list_end → StoreError::Syntax → ResponseError; never raises PyTypeError. This preserves the real-Redis unknown-option-token error class."
  - "lmove/blmove signature switched from str defaults (src=\"LEFT\", dest=\"RIGHT\") to Option<&Bound<PyAny>> with None default + manual fallback inside the body, because PyO3 default-value attributes require a Rust string literal not compatible with Bound."
  - "extract_token_str placed next to extract_bytes in src/commands/strings.rs (not in a new module) — single helpers file is the established pattern."
metrics:
  duration: 12min
  tasks: 2
  files: 3
  completed_date: 2026-04-25
---

# Quick Task 260425-ftl: Fix P3 — accept bytes for list option tokens (LINSERT/LMOVE/BLMOVE) Summary

**One-liner:** Add `extract_token_str` helper and route LINSERT `where` / LMOVE+BLMOVE `src`/`dest` (direct pymethods + pipeline dispatch arms) through it so pre-encoded bytes from redis-py / Pipeline no longer hit a TypeError at the PyO3 boundary.

## What changed

### Helper

**`src/commands/strings.rs::extract_token_str`** — new `pub fn` placed immediately after `extract_bytes`:

```rust
pub fn extract_token_str(obj: &Bound<'_, PyAny>) -> PyResult<String> {
    if let Ok(s) = obj.extract::<String>() { return Ok(s); }
    if let Ok(b) = obj.extract::<Vec<u8>>() {
        return Ok(String::from_utf8(b).unwrap_or_else(|e| {
            String::from_utf8_lossy(e.as_bytes()).into_owned()
        }));
    }
    Err(pyo3::exceptions::PyTypeError::new_err("expected str or bytes"))
}
```

### Five sites patched (post-commit line numbers in `src/lib.rs`)

| # | Site                                            | Line(s)        | Change                                                                                                |
|---|-------------------------------------------------|----------------|-------------------------------------------------------------------------------------------------------|
| 1 | Import                                          | 15             | Added `extract_token_str` to the `commands::strings` use list                                         |
| 2 | `fn linsert` pymethod                           | 2383–2401      | `r#where: &str` → `&Bound<'py, PyAny>`; decode via `extract_token_str` before `parse_linsert_where`   |
| 3 | `fn lmove` pymethod                             | 2467–2497      | `src/dest: &str` → `Option<&Bound<'py, PyAny>>` w/ None default; fallback to `"LEFT"`/`"RIGHT"`        |
| 4 | `fn blmove` pymethod                            | 2762–2788      | Same Option-pattern as `lmove`                                                                        |
| 5 | `dispatch_pipeline_command "linsert"` arm       | 3714–3731      | Replace `args.get_item(1)?.extract::<String>()?` with `extract_token_str(&args.get_item(1)?)?`        |
| 6 | `dispatch_pipeline_command "lmove"` arm         | 3764–3795      | Replace `v.extract::<String>()` with `extract_token_str(&v)` for both `src` and `dest` kwargs        |

(The plan tracked these as 5 sites; 1+2+3+4+5+6 = 6 edits because the import line is also a one-line change. The "5 call sites" count from the plan refers to consumer sites 2–6 — i.e. one helper, five consumers, exactly as planned.)

### Tests added

**`tests/test_lists.py`** — 11 new async tests appended at file end. All pass; existing 122 tests still pass (513 tests across the whole suite still pass).

| Coverage                                | Test                                              |
|------------------------------------------|---------------------------------------------------|
| LINSERT `b"BEFORE"`                      | `test_linsert_bytes_where_before`                 |
| LINSERT `b"AFTER"`                       | `test_linsert_bytes_where_after`                  |
| LINSERT lowercase bytes (case-insens.)   | `test_linsert_bytes_where_lowercase`              |
| LINSERT unknown-token bytes → syntax err | `test_linsert_bytes_where_unknown_token`          |
| LINSERT invalid UTF-8 bytes → syntax err | `test_linsert_bytes_where_invalid_utf8`           |
| LMOVE bytes cross-key                    | `test_lmove_bytes_tokens_cross_key`               |
| LMOVE bytes same-key rotation            | `test_lmove_bytes_tokens_same_key_rotation`       |
| LMOVE bytes lowercase                    | `test_lmove_bytes_tokens_lowercase`               |
| BLMOVE bytes (timeout=1.0)               | `test_blmove_bytes_tokens`                        |
| Pipeline LINSERT bytes (dispatch arm)    | `test_pipeline_linsert_bytes_where`               |
| Pipeline LMOVE bytes (dispatch arm)      | `test_pipeline_lmove_bytes_tokens`                |

The unknown-token and invalid-UTF-8 tests both confirm — through `pytest.raises(Exception, match="syntax")` — that the error path produces a ResponseError (the existing `StoreError::Syntax` wrapper), **not** a `TypeError`. This was the explicit must-have artifact of the plan.

## Pipeline tests: pass status

The plan's Step-4 fallback note was a precaution in case Pipeline-layer Python wrappers pre-coerced the option tokens to str. They do not — `Pipeline.linsert(name, where, refvalue, value)` (pipeline.py:237) buffers `where` unchanged, and `Pipeline.lmove(...)` buffers `src`/`dest` unchanged. So both pipeline tests pass unconditionally; no SUMMARY note about wrapper-blocking is required.

## Commits

| Task | Commit  | Type    | Files                                              |
|------|---------|---------|----------------------------------------------------|
| 1    | 559fb1d | feat    | src/commands/strings.rs, src/lib.rs                |
| 2    | 3ec5e8c | test    | tests/test_lists.py                                |

## Verification

- `cargo check`: clean (only pre-existing dead-code/deprecation warnings, none introduced by this change)
- `maturin develop --release`: builds + installs editable wheel
- `pytest tests/test_lists.py`: 133/133 pass (122 existing + 11 new)
- Full `pytest tests/` (excl. integration): 513/513 pass
- Smoke test: `r.linsert('k', b'BEFORE', 'a', 'b')`, `r.lmove(..., src=b'LEFT')`, `r.blmove(..., src=b'LEFT')` all succeed

## Deviations from Plan

None — plan executed exactly as written. The plan's Step-4 wrapper-blocked fallback for the pipeline tests turned out to be unneeded (Python pipeline wrappers do not pre-coerce option tokens), so both pipeline tests pass without modification.

## Self-Check: PASSED

- [x] `src/commands/strings.rs` modified (extract_token_str added)
- [x] `src/lib.rs` modified (import + 5 sites)
- [x] `tests/test_lists.py` modified (11 new tests appended)
- [x] Commit 559fb1d exists in git log
- [x] Commit 3ec5e8c exists in git log
- [x] cargo check clean; full test suite green
