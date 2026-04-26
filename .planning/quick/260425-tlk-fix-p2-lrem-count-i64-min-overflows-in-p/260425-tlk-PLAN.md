---
phase: 260425-tlk
plan: 01
type: execute
wave: 1
depends_on: []
files_modified:
  - src/commands/lists.rs
  - tests/test_lists.py
autonomous: true
requirements:
  - QUICK-260425-tlk
must_haves:
  truths:
    - "parse_lrem_count(i64::MIN) does not panic in debug or release builds"
    - "parse_lrem_count(i64::MIN) returns LremDirection::Tail with the correct unsigned magnitude (9223372036854775808)"
    - "Python caller r.lrem('missing', -9223372036854775808, b'v') returns 0 with no panic, no exception"
  artifacts:
    - path: src/commands/lists.rs
      provides: "Panic-free LREM count parsing using i64::unsigned_abs() at line 69"
      contains: "count.unsigned_abs()"
    - path: tests/test_lists.py
      provides: "Python regression test for LREM with i64::MIN count"
      contains: "9223372036854775808"
  key_links:
    - from: "src/commands/lists.rs::parse_lrem_count"
      to: "i64::unsigned_abs()"
      via: "stdlib intrinsic"
      pattern: "count\\.unsigned_abs\\(\\) as usize"
---

<objective>
Replace the panicking `(-count) as usize` expression in `parse_lrem_count` with `count.unsigned_abs() as usize` so that `i64::MIN` no longer causes overflow (panic in debug, wrap-then-cast in release).

Purpose: A Python caller passing `count = -9223372036854775808` to `r.lrem()` currently triggers an arithmetic overflow inside the Rust parser because `-i64::MIN` is mathematically unrepresentable in i64. `i64::unsigned_abs()` returns `u64` and is total over all i64 inputs.

Output: One-character-class fix in `src/commands/lists.rs` line 69, one Rust unit test, one Python regression test. Single atomic commit.
</objective>

<execution_context>
@$HOME/.claude/get-shit-done/workflows/execute-plan.md
@$HOME/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@.planning/STATE.md
@CLAUDE.md
@src/commands/lists.rs
@tests/test_lists.py

<interfaces>
<!-- Existing test conventions, extracted directly from src/commands/lists.rs and tests/test_lists.py. -->
<!-- Use these exactly — no codebase exploration needed. -->

From src/commands/lists.rs (lines 34-39, 65-72):
```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LremDirection {
    Head(usize),
    Tail(usize),
    All,
}

// CURRENT (buggy):
pub fn parse_lrem_count(count: i64) -> LremDirection {
    match count.cmp(&0) {
        std::cmp::Ordering::Greater => LremDirection::Head(count as usize),
        std::cmp::Ordering::Less => LremDirection::Tail((-count) as usize),  // line 69 — overflows on i64::MIN
        std::cmp::Ordering::Equal => LremDirection::All,
    }
}
```

From src/commands/lists.rs (lines 138-143) — existing test pattern in `mod tests`:
```rust
#[test]
fn parse_lrem_count_sign() {
    assert_eq!(parse_lrem_count(3), LremDirection::Head(3));
    assert_eq!(parse_lrem_count(-2), LremDirection::Tail(2));
    assert_eq!(parse_lrem_count(0), LremDirection::All);
}
```

From tests/test_lists.py (lines 167-168) — test idiom near other LREM tests:
```python
async def test_lrem_missing_key(r):
    assert await r.lrem("missing", 0, "v") == 0
```

Audit note: `normalize_range_indices` (lines 86-101) is SAFE. It uses `start + n` and `stop + n` (additive offset from tail), never `-start`, so there is no analogous overflow path to fix.
</interfaces>
</context>

<tasks>

<task type="auto" tdd="true">
  <name>Task 1: Fix parse_lrem_count overflow on i64::MIN; add Rust + Python regression tests</name>
  <files>src/commands/lists.rs, tests/test_lists.py</files>
  <behavior>
    Rust unit test (in src/commands/lists.rs `mod tests`, alongside existing `parse_lrem_count_sign`):
      - parse_lrem_count(i64::MIN) MUST NOT panic (debug build, overflow-checks ON)
      - parse_lrem_count(i64::MIN) == LremDirection::Tail(i64::MIN.unsigned_abs() as usize)
      - The unsigned magnitude equals 9223372036854775808 (== 2^63)

    Python regression test (in tests/test_lists.py, immediately after `test_lrem_missing_key`):
      - `await r.lrem("missing", -9223372036854775808, b"v")` returns 0
      - No panic, no exception, no traceback from the Rust layer
  </behavior>
  <action>
1. **Edit `src/commands/lists.rs` line 69**: change
   ```rust
   std::cmp::Ordering::Less => LremDirection::Tail((-count) as usize),
   ```
   to
   ```rust
   std::cmp::Ordering::Less => LremDirection::Tail(count.unsigned_abs() as usize),
   ```
   `i64::unsigned_abs()` returns `u64` and is total — including `i64::MIN` → `2^63`. The cast `u64 as usize` is lossless on 64-bit targets (the only platforms this crate ships for via maturin manylinux/macOS/aarch64 wheels). No other lines change.

2. **Add Rust unit test** in the existing `#[cfg(test)] mod tests` block in `src/commands/lists.rs` (after `parse_lrem_count_sign` at line ~143):
   ```rust
   #[test]
   fn parse_lrem_count_i64_min_no_overflow() {
       // Regression: -i64::MIN overflows in i64; use unsigned_abs() instead.
       // This test runs under cargo test (debug profile, overflow-checks ON)
       // and proves we never compute -i64::MIN as i64.
       let result = parse_lrem_count(i64::MIN);
       assert_eq!(
           result,
           LremDirection::Tail(i64::MIN.unsigned_abs() as usize)
       );
       // Also verify the magnitude is the expected 2^63.
       if let LremDirection::Tail(n) = result {
           assert_eq!(n as u64, 9_223_372_036_854_775_808_u64);
       } else {
           panic!("expected LremDirection::Tail, got {:?}", result);
       }
   }
   ```

3. **Add Python regression test** in `tests/test_lists.py` immediately after `test_lrem_missing_key` (line 168), before the `# LIST-10: LSET` section header:
   ```python
   async def test_lrem_count_i64_min_no_panic(r):
       # P2 regression: count = i64::MIN previously overflowed inside
       # parse_lrem_count via `-count`. Must return 0 cleanly on a
       # missing key, not panic.
       assert await r.lrem("missing", -9223372036854775808, b"v") == 0
   ```

4. **Do NOT modify** `normalize_range_indices` (lines 86-101). Audit confirms it uses additive offsets (`start + n`, `stop + n`) — no `-x` negation patterns and no analogous overflow bug. Leave it alone per the constraint "DO NOT change unrelated code."

5. Build the release wheel so the new Python test exercises the fixed binary, then run pytest.
  </action>
  <verify>
    <automated>cargo test --lib commands::lists -- --nocapture &amp;&amp; cargo check &amp;&amp; maturin develop --release &amp;&amp; pytest tests/test_lists.py -W error::RuntimeWarning -x</automated>
  </verify>
  <done>
    - Line 69 of `src/commands/lists.rs` reads `LremDirection::Tail(count.unsigned_abs() as usize)`.
    - `cargo test --lib commands::lists` passes including the new `parse_lrem_count_i64_min_no_overflow` test (proves no panic in debug profile with overflow-checks ON).
    - `cargo check` succeeds with no warnings on the changed file.
    - `maturin develop --release` builds and installs the wheel.
    - `pytest tests/test_lists.py -W error::RuntimeWarning -x` passes including the new `test_lrem_count_i64_min_no_panic`.
    - No edits to `normalize_range_indices` or any unrelated code.
    - No new dependencies, no API changes, no signature changes.
  </done>
</task>

</tasks>

<verification>
- Rust: `cargo test --lib commands::lists` runs in debug profile, where `overflow-checks = true` is the default. The new test would have panicked under the old code; it now passes.
- Rust: existing `parse_lrem_count_sign` continues to pass (regression net for the positive/negative/zero arms).
- Rust: `cargo check` ensures no warnings introduced.
- Python: `pytest tests/test_lists.py -W error::RuntimeWarning -x` runs the full list test suite against the rebuilt wheel including the new i64::MIN regression test. The `-x` flag stops at the first failure so we'd catch any incidental break.
- The fix is local to one expression — semantic equivalence for all non-MIN inputs is preserved by `i64::unsigned_abs()`'s contract: for any `x: i64` where `x != i64::MIN`, `x.unsigned_abs() == (-x) as u64`.
</verification>

<success_criteria>
- `parse_lrem_count(i64::MIN)` returns `LremDirection::Tail(9223372036854775808_usize)` without panicking in debug or release builds.
- `await r.lrem("missing", -9223372036854775808, b"v")` returns `0` from the Python API, no traceback.
- All existing list-related Rust unit tests and Python tests pass unchanged.
- Two test files modified, one source file modified, one expression replaced. No other changes.
- Single atomic git commit covering: source fix + Rust test + Python test.
</success_criteria>

<output>
After completion, create `.planning/quick/260425-tlk-fix-p2-lrem-count-i64-min-overflows-in-p/260425-tlk-SUMMARY.md` recording:
- The one-line source change with before/after.
- The two new test names and their assertions.
- Confirmation that `normalize_range_indices` was audited and required no change (uses additive offsets, not negation).
- Verification command results.
</output>
