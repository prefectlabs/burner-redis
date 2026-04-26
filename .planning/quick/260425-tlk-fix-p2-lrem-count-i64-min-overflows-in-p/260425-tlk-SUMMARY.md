---
phase: 260425-tlk
plan: 01
subsystem: commands/lists
tags:
  - bugfix
  - p2
  - overflow
  - lrem
  - regression-test
one_liner: "Use i64::unsigned_abs() in parse_lrem_count so LREM count=i64::MIN no longer panics on -count overflow."
dependency_graph:
  requires:
    - parse_lrem_count helper in src/commands/lists.rs
  provides:
    - Panic-free LREM count parsing for the full i64 input domain
  affects:
    - src/commands/lists.rs
    - tests/test_lists.py
tech_stack:
  added: []
  patterns:
    - "i64::unsigned_abs() over (-x) as u64 / (-x) as usize for total negation on signed→unsigned conversion"
key_files:
  created: []
  modified:
    - src/commands/lists.rs
    - tests/test_lists.py
decisions:
  - "Used `count.unsigned_abs() as usize` (returns u64, lossless on 64-bit targets — the only platforms shipped via maturin) over `count.checked_neg()` or i128 widening — minimal one-call replacement and total over all i64 inputs"
metrics:
  duration: "~2 min"
  completed: "2026-04-25"
  tasks: 1
  files_changed: 2
  commits:
    - 51a0451
requirements:
  - QUICK-260425-tlk
---

# Quick Task 260425-tlk: Fix P2 LREM count i64::MIN overflow Summary

**One-liner:** Replaced the panicking `(-count) as usize` expression in `parse_lrem_count` with `count.unsigned_abs() as usize` so that an `LREM` call with `count = i64::MIN` (`-9223372036854775808`) returns cleanly instead of panicking inside the Rust parser.

## Source change (one expression)

**File:** `src/commands/lists.rs`, line 69, in `pub fn parse_lrem_count(count: i64) -> LremDirection`

Before:
```rust
std::cmp::Ordering::Less => LremDirection::Tail((-count) as usize),
```

After:
```rust
std::cmp::Ordering::Less => LremDirection::Tail(count.unsigned_abs() as usize),
```

`i64::unsigned_abs()` returns `u64` and is total over the full `i64` input domain. For all `x != i64::MIN`, `x.unsigned_abs() == (-x) as u64` (semantic equivalence preserved). For `x == i64::MIN`, it returns `2^63 == 9_223_372_036_854_775_808_u64`, where the old `-x` would have overflowed (panic in debug profile / wrap-and-truncate in release).

The `as usize` cast is lossless on every target this crate ships to — maturin builds manylinux x86_64/aarch64 + macOS x86_64/arm64 wheels (per Phase 09 decisions), all 64-bit.

## Tests added

### Rust unit test

`src/commands/lists.rs::tests::parse_lrem_count_i64_min_no_overflow` (added immediately after `parse_lrem_count_sign`):

- Asserts `parse_lrem_count(i64::MIN) == LremDirection::Tail(i64::MIN.unsigned_abs() as usize)` — would have panicked under the old code in `cargo test`'s debug profile (overflow-checks ON).
- Asserts the magnitude unwraps to `9_223_372_036_854_775_808_u64 == 2^63`.

### Python regression test

`tests/test_lists.py::test_lrem_count_i64_min_no_panic` (added immediately after `test_lrem_missing_key`, before the `# LIST-10: LSET` section header):

- `await r.lrem("missing", -9223372036854775808, b"v") == 0` — exercises the path through PyO3 → `BurnerRedis.lrem` → `Store::lrem` → `parse_lrem_count` against the `--release` wheel built by maturin. Must return 0 with no traceback / no panic.

## Audit: `normalize_range_indices`

Audited `normalize_range_indices` (lines 86-101) per the plan's interfaces note. It uses additive offsets only (`start + n`, `stop + n`) — no `-x` negation patterns and no analogous overflow path. **No change required.** Left untouched per the constraint "DO NOT change unrelated code."

## Verification command results

| Command | Result |
|---------|--------|
| `cargo test --lib commands::lists -- --nocapture` | **7 passed; 0 failed** (incl. new `parse_lrem_count_i64_min_no_overflow`) |
| `cargo check` | **Finished `dev` profile** with no errors and no new warnings on changed file (pre-existing warnings in `src/lib.rs` and `src/store.rs` only) |
| `uv run maturin develop --release` | **Built wheel `burner_redis-0.1.5-cp310-abi3-macosx_11_0_arm64.whl`**, installed editable |
| `uv run pytest tests/test_lists.py -W error::RuntimeWarning -x` | **152 passed in 2.57s** (incl. new `test_lrem_count_i64_min_no_panic`) |

## Deviations from Plan

None — plan executed exactly as written. Single atomic commit covering source fix + Rust test + Python test, as specified in `<success_criteria>`.

## Self-Check: PASSED

- `src/commands/lists.rs` line 69 contains `count.unsigned_abs() as usize` — VERIFIED via Edit tool diff.
- `src/commands/lists.rs` `mod tests` contains `parse_lrem_count_i64_min_no_overflow` — VERIFIED via cargo test output (`test commands::lists::tests::parse_lrem_count_i64_min_no_overflow ... ok`).
- `tests/test_lists.py` contains `test_lrem_count_i64_min_no_panic` — VERIFIED via pytest -k filter (`tests/test_lists.py::test_lrem_count_i64_min_no_panic PASSED`).
- Commit `51a0451` exists in git history with both files staged.
