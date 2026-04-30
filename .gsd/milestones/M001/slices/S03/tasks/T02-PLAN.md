# T02: Implement async Python methods for all sorted set commands on BurnerRedis with comprehensive pytest coverage.

**Slice:** S03 — **Milestone:** M001

## Description

Implement async Python methods for all sorted set commands on BurnerRedis with comprehensive pytest coverage.

Purpose: Expose the Store engine's sorted set operations as async Python methods matching redis.asyncio.Redis signatures, enabling Prefect's lease expiration tracking and causal event ordering patterns.

Output: 6 new async methods on BurnerRedis + test suite validating all Phase 3 requirements (ZSET-01 through ZSET-06).

## Legacy Source

---
phase: 03-sorted-set-commands
plan: 02
type: execute
wave: 2
depends_on: [03-01]
files_modified:
  - src/lib.rs
  - tests/test_sorted_sets.py
autonomous: true
requirements: [ZSET-01, ZSET-02, ZSET-03, ZSET-04, ZSET-05, ZSET-06]

must_haves:
  truths:
    - "User can ZADD members with scores and ZREM members from a sorted set"
    - "User can ZRANGE by index range, receiving members in sorted order"
    - "User can ZRANGEBYSCORE by score range with -inf/+inf support"
    - "User can ZRANGESTORE to copy a range result into a new key"
    - "User can ZREMRANGEBYSCORE to remove all members within a score range"
    - "ZADD supports NX/XX/GT/LT/CH flags matching redis-py signatures"
    - "ZRANGE and ZRANGEBYSCORE support withscores returning (member, score) tuples"
    - "WRONGTYPE error is raised when operating on wrong key type from Python"
    - "All methods are async-compatible and match redis.asyncio.Redis signatures"
  artifacts:
    - path: "src/lib.rs"
      provides: "Async zadd/zrem/zrange/zrangebyscore/zrangestore/zremrangebyscore methods on BurnerRedis"
      contains: "fn zadd"
    - path: "tests/test_sorted_sets.py"
      provides: "Comprehensive pytest suite for sorted set commands"
      min_lines: 100
  key_links:
    - from: "src/lib.rs"
      to: "src/store.rs"
      via: "store.zadd/zrem/zrange/zrangebyscore/zrangestore/zremrangebyscore calls"
      pattern: "store\\.(zadd|zrem|zrange|zrangebyscore|zrangestore|zremrangebyscore)"
    - from: "src/lib.rs"
      to: "pyo3_async_runtimes"
      via: "future_into_py async bridge"
      pattern: "future_into_py"
    - from: "tests/test_sorted_sets.py"
      to: "python/burner_redis/__init__.py"
      via: "from burner_redis import BurnerRedis"
      pattern: "from burner_redis import"
---

<objective>
Implement async Python methods for all sorted set commands on BurnerRedis with comprehensive pytest coverage.

Purpose: Expose the Store engine's sorted set operations as async Python methods matching redis.asyncio.Redis signatures, enabling Prefect's lease expiration tracking and causal event ordering patterns.

Output: 6 new async methods on BurnerRedis + test suite validating all Phase 3 requirements (ZSET-01 through ZSET-06).
</objective>

<execution_context>
@$HOME/.claude/get-shit-done/workflows/execute-plan.md
@$HOME/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@.planning/PROJECT.md
@.planning/ROADMAP.md
@.planning/STATE.md
@.planning/phases/03-sorted-set-commands/03-01-SUMMARY.md

<interfaces>
<!-- Key types and contracts from Plan 01 that the executor needs. -->

From src/store.rs (after Plan 01):
```rust
#[derive(Clone, Debug)]
pub struct SortedSet {
    pub by_score: BTreeMap<(OrderedFloat<f64>, Bytes), ()>,
    pub by_member: HashMap<Bytes, f64>,
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("WRONGTYPE Operation against a key holding the wrong kind of value")]
    WrongType,
}

impl Store {
    // Sorted set operations
    pub fn zadd(&self, key: Bytes, members: Vec<(f64, Bytes)>, nx: bool, xx: bool, gt: bool, lt: bool, ch: bool) -> Result<i64, StoreError>;
    pub fn zrem(&self, key: &Bytes, members: &[Bytes]) -> Result<i64, StoreError>;
    pub fn zrange(&self, key: &Bytes, start: i64, stop: i64, withscores: bool) -> Result<Vec<(Bytes, Option<f64>)>, StoreError>;
    pub fn zrangebyscore(&self, key: &Bytes, min: f64, max: f64, withscores: bool) -> Result<Vec<(Bytes, Option<f64>)>, StoreError>;
    pub fn zrangestore(&self, dst: Bytes, src: &Bytes, min: f64, max: f64) -> Result<i64, StoreError>;
    pub fn zremrangebyscore(&self, key: &Bytes, min: f64, max: f64) -> Result<i64, StoreError>;
}
```

From src/lib.rs (existing pattern):
```rust
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict};
use std::collections::HashSet as StdHashSet;
use std::sync::Arc;

use commands::strings::{extract_bytes, extract_expiry};
use store::{Store, StoreError};

fn store_err_to_py(e: StoreError) -> PyErr {
    match e {
        StoreError::WrongType => {
            pyo3::exceptions::PyException::new_err(e.to_string())
        }
    }
}

#[pyclass]
pub struct BurnerRedis {
    store: Arc<Store>,
}

// Pattern: clone Arc<Store>, extract Python args, call future_into_py
// All methods use future_into_py(py, async move { ... })
```

From tests/conftest.py:
```python
@pytest.fixture
def r():
    """Create a fresh BurnerRedis instance for each test."""
    return BurnerRedis()
```

redis-py ZADD signature: `zadd(name, mapping, nx=False, xx=False, gt=False, lt=False, ch=False)`
- mapping is a dict of {member: score} (note: member is key, score is value in the dict)
redis-py ZRANGE signature: `zrange(name, start, end, withscores=False)`
redis-py ZRANGEBYSCORE signature: `zrangebyscore(name, min, max, withscores=False)`
redis-py ZRANGESTORE signature: `zrangestore(dest, name, start, end)`
redis-py ZREM signature: `zrem(name, *values)`
redis-py ZREMRANGEBYSCORE signature: `zremrangebyscore(name, min, max)`
</interfaces>
</context>

<tasks>

<task type="auto">
  <name>Task 1: Implement async sorted set methods on BurnerRedis</name>
  <files>src/lib.rs</files>
  <read_first>src/lib.rs, src/store.rs, src/commands/strings.rs</read_first>
  <action>
Add the following async methods to the `#[pymethods] impl BurnerRedis` block in `src/lib.rs`, following the established pattern (clone Arc Store, extract args, future_into_py). Add a `// -- Sorted Set Commands --` section header comment.

1. **ZADD** matching redis-py: `zadd(name, mapping, nx=False, xx=False, gt=False, lt=False, ch=False)`
   ```rust
   #[pyo3(signature = (name, mapping, nx=false, xx=false, gt=false, lt=false, ch=false))]
   fn zadd<'py>(
       &self,
       py: Python<'py>,
       name: &Bound<'py, PyAny>,
       mapping: &Bound<'py, PyDict>,
       nx: bool,
       xx: bool,
       gt: bool,
       lt: bool,
       ch: bool,
   ) -> PyResult<Bound<'py, PyAny>>
   ```
   - Extract `name` with `extract_bytes`.
   - Iterate over `mapping` dict items. For each (member_key, score_value):
     - Extract member with `extract_bytes(&k)`
     - Extract score with `v.extract::<f64>()`
     - Collect into `Vec<(f64, Bytes)>` as `(score, member)` pairs.
   - Call `store.zadd(name_bytes, members, nx, xx, gt, lt, ch)` inside the async block.
   - Map error with `store_err_to_py`.
   - Returns i64.

2. **ZREM** matching redis-py: `zrem(name, *values)`
   ```rust
   #[pyo3(signature = (name, *values))]
   fn zrem<'py>(
       &self,
       py: Python<'py>,
       name: &Bound<'py, PyAny>,
       values: &Bound<'py, pyo3::types::PyTuple>,
   ) -> PyResult<Bound<'py, PyAny>>
   ```
   - Extract name, extract variadic members from PyTuple via `extract_bytes`.
   - Call `store.zrem(&name_bytes, &members)`.
   - Returns i64.

3. **ZRANGE** matching redis-py: `zrange(name, start, end, withscores=False)`
   ```rust
   #[pyo3(signature = (name, start, end, withscores=false))]
   fn zrange<'py>(
       &self,
       py: Python<'py>,
       name: &Bound<'py, PyAny>,
       start: i64,
       end: i64,
       withscores: bool,
   ) -> PyResult<Bound<'py, PyAny>>
   ```
   - Extract name, pass start/end/withscores directly.
   - Call `store.zrange(&name_bytes, start, end, withscores)`.
   - Convert result: if `withscores` is false, return `Vec<Vec<u8>>` (just the members as bytes, discarding the None scores). If `withscores` is true, return `Vec<(Vec<u8>, f64)>` (member-score tuples). PyO3 converts `Vec<(Vec<u8>, f64)>` to `list[tuple[bytes, float]]` automatically.
   - Handle the conversion inside the async block:
     ```rust
     let results = store.zrange(&name_bytes, start, end, withscores).map_err(store_err_to_py)?;
     if withscores {
         // Return list of (bytes, float) tuples
         let tuples: Vec<(Vec<u8>, f64)> = results.into_iter()
             .map(|(m, s)| (m.to_vec(), s.unwrap_or(0.0)))
             .collect();
         Ok(tuples)  // This won't work directly -- need Python interop
     }
     ```
   - IMPORTANT: Since PyO3 future_into_py requires a single return type, and we need to return different types based on `withscores`, we need a workaround. The simplest approach: always return the data as a Python object constructed manually. Use `Python::with_gil` inside the async block to build the return value:

   REVISED APPROACH: Return two different types is tricky with PyO3 generics. Instead, use `Py<PyAny>` as the return type inside the async block:
   ```rust
   pyo3_async_runtimes::tokio::future_into_py(py, async move {
       let results = store.zrange(&name_bytes, start, end, withscores).map_err(store_err_to_py)?;
       Python::with_gil(|py| {
           if withscores {
               let list: Vec<(Vec<u8>, f64)> = results.into_iter()
                   .map(|(m, s)| (m.to_vec(), s.unwrap_or(0.0)))
                   .collect();
               Ok(list.into_pyobject(py)?.into_any().unbind())
           } else {
               let list: Vec<Vec<u8>> = results.into_iter()
                   .map(|(m, _)| m.to_vec())
                   .collect();
               Ok(list.into_pyobject(py)?.into_any().unbind())
           }
       })
   })
   ```
   Note: You may need `use pyo3::IntoPyObject;` or `use pyo3::conversion::IntoPyObject;` for the `.into_pyobject()` method. If `into_pyobject` is not available in PyO3 0.28.3, use the alternative `pythonize` approach or return via `PyList`. Check the PyO3 0.28.x API. The simplest fallback: use `pyo3::types::PyList::new()` to build the list manually, or use `.into_py(py)` / `.to_object(py)`:
   ```rust
   // Fallback approach using to_object:
   if withscores {
       let list: Vec<(Vec<u8>, f64)> = results.into_iter()
           .map(|(m, s)| (m.to_vec(), s.unwrap_or(0.0)))
           .collect();
       Ok(list.to_object(py))
   } else {
       let list: Vec<Vec<u8>> = results.into_iter()
           .map(|(m, _)| m.to_vec())
           .collect();
       Ok(list.to_object(py))
   }
   ```
   Use whichever approach compiles cleanly with PyO3 0.28.3. The key requirement: without `withscores`, return `list[bytes]`; with `withscores`, return `list[tuple[bytes, float]]`.

4. **ZRANGEBYSCORE** matching redis-py: `zrangebyscore(name, min, max, withscores=False)`
   ```rust
   #[pyo3(signature = (name, min, max, withscores=false))]
   fn zrangebyscore<'py>(
       &self,
       py: Python<'py>,
       name: &Bound<'py, PyAny>,
       min: &Bound<'py, PyAny>,
       max: &Bound<'py, PyAny>,
       withscores: bool,
   ) -> PyResult<Bound<'py, PyAny>>
   ```
   - Extract name.
   - Parse `min` and `max`: redis-py accepts both float and string ("-inf", "+inf", "inf"). Create a helper function (either inline or in sorted_sets.rs command module) to convert:
     ```rust
     fn parse_score_bound(obj: &Bound<'_, PyAny>) -> PyResult<f64> {
         if let Ok(f) = obj.extract::<f64>() {
             return Ok(f);
         }
         if let Ok(s) = obj.extract::<String>() {
             match s.as_str() {
                 "-inf" => return Ok(f64::NEG_INFINITY),
                 "+inf" | "inf" => return Ok(f64::INFINITY),
                 _ => {}
             }
             // Try parsing as float string
             if let Ok(f) = s.parse::<f64>() {
                 return Ok(f);
             }
         }
         Err(pyo3::exceptions::PyValueError::new_err("min/max must be a float or '-inf'/'+inf'"))
     }
     ```
     Put this helper in `src/commands/sorted_sets.rs` and export it, or define it inline in lib.rs. Putting it in sorted_sets.rs follows the pattern of strings.rs having extract_bytes.
   - Call `store.zrangebyscore(&name_bytes, min_f64, max_f64, withscores)`.
   - Convert results same as ZRANGE (list[bytes] or list[tuple[bytes, float]]).

5. **ZRANGESTORE** matching redis-py: `zrangestore(dest, name, start, end)`
   Note: redis-py's `zrangestore` uses ZRANGESTORE which in Redis 6.2+ supports BY SCORE. For our use case (Prefect), the score-based version is what matters. Match redis-py signature:
   ```rust
   #[pyo3(signature = (dest, name, start, end))]
   fn zrangestore<'py>(
       &self,
       py: Python<'py>,
       dest: &Bound<'py, PyAny>,
       name: &Bound<'py, PyAny>,
       start: &Bound<'py, PyAny>,
       end: &Bound<'py, PyAny>,
   ) -> PyResult<Bound<'py, PyAny>>
   ```
   - Extract dest and name with `extract_bytes`.
   - Parse start and end as score bounds using the same `parse_score_bound` helper (they are score values for ZRANGESTORE BYSCORE).
   - Call `store.zrangestore(dst_bytes, &src_bytes, min, max)`.
   - Returns i64.

6. **ZREMRANGEBYSCORE** matching redis-py: `zremrangebyscore(name, min, max)`
   ```rust
   #[pyo3(signature = (name, min, max))]
   fn zremrangebyscore<'py>(
       &self,
       py: Python<'py>,
       name: &Bound<'py, PyAny>,
       min: &Bound<'py, PyAny>,
       max: &Bound<'py, PyAny>,
   ) -> PyResult<Bound<'py, PyAny>>
   ```
   - Extract name, parse min/max with `parse_score_bound`.
   - Call `store.zremrangebyscore(&name_bytes, min_f64, max_f64)`.
   - Returns i64.

For the `parse_score_bound` helper function: add it to `src/commands/sorted_sets.rs` and import it in lib.rs:
```rust
// In src/commands/sorted_sets.rs, add:
use pyo3::prelude::*;
use pyo3::types::PyAny;

/// Parse a score bound from a Python object.
/// Accepts float, int, or string ("-inf", "+inf", "inf").
pub fn parse_score_bound(obj: &Bound<'_, PyAny>) -> PyResult<f64> {
    // Try float first
    if let Ok(f) = obj.extract::<f64>() {
        return Ok(f);
    }
    // Try string for -inf/+inf
    if let Ok(s) = obj.extract::<String>() {
        match s.as_str() {
            "-inf" => return Ok(f64::NEG_INFINITY),
            "+inf" | "inf" => return Ok(f64::INFINITY),
            _ => {}
        }
        if let Ok(f) = s.parse::<f64>() {
            return Ok(f);
        }
    }
    Err(pyo3::exceptions::PyValueError::new_err(
        "min/max must be a float or '-inf'/'+inf'",
    ))
}
```

Then in lib.rs, import: `use commands::sorted_sets::parse_score_bound;`

Update src/commands/sorted_sets.rs (which was a doc-only module from Plan 01) to include this function. This means Plan 01's task 2 creates the doc-only file, and this task adds the function to it. Since this task lists src/lib.rs as its file, note that it will ALSO modify src/commands/sorted_sets.rs to add the helper function. This is acceptable since sorted_sets.rs was just created in Plan 01 as a doc-only module.
  </action>
  <verify>
    <automated>cd /Users/desertaxle/dev/prefectlabs/burner-redis && cargo test 2>&1 | tail -10 && maturin develop 2>&1 | tail -5 && python -c "from burner_redis import BurnerRedis; print('import OK')"</automated>
  </verify>
  <acceptance_criteria>
    - grep -q "fn zadd" src/lib.rs (zadd method defined)
    - grep -q "fn zrem" src/lib.rs (zrem method defined)
    - grep -q "fn zrange" src/lib.rs (zrange method defined)
    - grep -q "fn zrangebyscore" src/lib.rs (zrangebyscore method defined)
    - grep -q "fn zrangestore" src/lib.rs (zrangestore method defined)
    - grep -q "fn zremrangebyscore" src/lib.rs (zremrangebyscore method defined)
    - grep -q "future_into_py" src/lib.rs (async bridge used)
    - grep -q "store_err_to_py" src/lib.rs (error conversion used)
    - grep -q "parse_score_bound" src/commands/sorted_sets.rs (helper function exists)
    - grep -q "parse_score_bound" src/lib.rs (helper imported)
    - grep -q "withscores" src/lib.rs (withscores parameter handled)
    - cargo test passes
    - maturin develop succeeds
    - Python import succeeds
  </acceptance_criteria>
  <done>
    - All 6 async methods (zadd/zrem/zrange/zrangebyscore/zrangestore/zremrangebyscore) defined on BurnerRedis
    - Methods follow established pattern: clone Arc Store, extract_bytes/parse_score_bound, future_into_py
    - ZADD accepts mapping dict with NX/XX/GT/LT/CH flags matching redis-py signature
    - ZRANGE and ZRANGEBYSCORE support withscores parameter returning list[tuple[bytes, float]]
    - ZRANGEBYSCORE and ZREMRANGEBYSCORE accept "-inf"/"+inf" string bounds
    - parse_score_bound helper in sorted_sets.rs module
    - WRONGTYPE errors converted to Python exceptions
    - Crate compiles, maturin develop works, Python import succeeds
  </done>
</task>

<task type="auto">
  <name>Task 2: Create comprehensive pytest suite for sorted set commands</name>
  <files>tests/test_sorted_sets.py</files>
  <read_first>tests/test_sets.py, tests/conftest.py, src/lib.rs</read_first>
  <action>
Create `tests/test_sorted_sets.py` with comprehensive tests covering ZSET-01 through ZSET-06. Follow the established pattern from test_sets.py and test_hashes.py: async test functions using the `r` fixture from conftest.py, pytest-asyncio auto mode.

```python
"""Tests for sorted set commands: ZADD, ZREM, ZRANGE, ZRANGEBYSCORE, ZRANGESTORE, ZREMRANGEBYSCORE.

Covers requirements: ZSET-01, ZSET-02, ZSET-03, ZSET-04, ZSET-05, ZSET-06.
"""
import pytest
from burner_redis import BurnerRedis
```

Tests (all async, using `r` fixture):

**ZSET-01 (ZADD):**
- `test_zadd_new_members` -- `zadd("z", {"a": 1.0, "b": 2.0, "c": 3.0})` returns 3
- `test_zadd_update_existing` -- zadd same member with new score returns 0 (not new), but score is updated (verify via zrange withscores)
- `test_zadd_nx_flag` -- zadd with nx=True only adds new members, skips existing. Add "a" with score 1, then zadd {"a": 5, "b": 2} nx=True returns 1 (only b added), "a" still has score 1.0
- `test_zadd_xx_flag` -- zadd with xx=True only updates existing members. Add "a" with score 1, then zadd {"a": 5, "b": 2} xx=True returns 0 (default counts new), "a" has score 5.0, "b" not added
- `test_zadd_gt_flag` -- zadd with gt=True only updates if new score > old. Add "a" score 5, then zadd {"a": 3} gt=True -- "a" still 5.0. Then zadd {"a": 7} gt=True -- "a" now 7.0
- `test_zadd_lt_flag` -- zadd with lt=True only updates if new score < old. Add "a" score 5, then zadd {"a": 7} lt=True -- "a" still 5.0. Then zadd {"a": 3} lt=True -- "a" now 3.0
- `test_zadd_ch_flag` -- zadd with ch=True returns count of changed (new + updated). Add "a" score 1, then zadd {"a": 2, "b": 3} ch=True returns 2 (a changed + b new)
- `test_zadd_bytes_input` -- zadd with bytes keys/members works
- `test_zadd_wrongtype` -- zadd on a string key raises WRONGTYPE

**ZSET-02 (ZREM):**
- `test_zrem_existing_members` -- removes members, returns count removed
- `test_zrem_nonexistent_members` -- returns 0 for members not in set
- `test_zrem_missing_key` -- returns 0 for non-existent key
- `test_zrem_wrongtype` -- raises WRONGTYPE on wrong type
- `test_zrem_verify_removal` -- after zrem, member no longer appears in zrange

**ZSET-03 (ZRANGE):**
- `test_zrange_full_range` -- `zrange("z", 0, -1)` returns all members in score order as list[bytes]
- `test_zrange_subset` -- `zrange("z", 1, 2)` returns correct slice
- `test_zrange_negative_indices` -- `zrange("z", -2, -1)` returns last 2 members
- `test_zrange_withscores` -- `zrange("z", 0, -1, withscores=True)` returns list of (bytes, float) tuples
- `test_zrange_empty_key` -- returns empty list for non-existent key
- `test_zrange_out_of_range` -- out-of-bounds indices return empty or clamped result
- `test_zrange_returns_list_type` -- confirms return type is list
- `test_zrange_score_ordering` -- members with different scores are returned in ascending score order
- `test_zrange_wrongtype` -- raises WRONGTYPE on wrong type

**ZSET-04 (ZRANGEBYSCORE):**
- `test_zrangebyscore_range` -- returns members within score range
- `test_zrangebyscore_inf` -- `zrangebyscore("z", "-inf", "+inf")` returns all members
- `test_zrangebyscore_withscores` -- returns (bytes, float) tuples when withscores=True
- `test_zrangebyscore_no_matches` -- score range with no matches returns empty list
- `test_zrangebyscore_empty_key` -- returns empty list for non-existent key
- `test_zrangebyscore_float_bounds` -- accepts float values for min/max
- `test_zrangebyscore_wrongtype` -- raises WRONGTYPE on wrong type

**ZSET-05 (ZRANGESTORE):**
- `test_zrangestore_basic` -- copies score range to new key, returns count stored
- `test_zrangestore_verify_dest` -- verify destination key contains correct members via zrange
- `test_zrangestore_empty_range` -- empty range returns 0
- `test_zrangestore_missing_src` -- missing source returns 0
- `test_zrangestore_overwrites_dest` -- if dest already exists, it is replaced
- `test_zrangestore_wrongtype` -- raises WRONGTYPE if source is wrong type

**ZSET-06 (ZREMRANGEBYSCORE):**
- `test_zremrangebyscore_basic` -- removes members in score range, returns count
- `test_zremrangebyscore_verify_remaining` -- verify remaining members via zrange
- `test_zremrangebyscore_no_matches` -- no matches returns 0
- `test_zremrangebyscore_all_members` -- removing full range removes all members
- `test_zremrangebyscore_missing_key` -- missing key returns 0
- `test_zremrangebyscore_wrongtype` -- raises WRONGTYPE on wrong type

For WRONGTYPE tests, use established pattern:
```python
await r.set("strkey", "value")
with pytest.raises(Exception, match="WRONGTYPE"):
    await r.zadd("strkey", {"member": 1.0})
```

For score assertions with withscores, compare tuples:
```python
result = await r.zrange("z", 0, -1, withscores=True)
assert result == [(b"a", 1.0), (b"b", 2.0), (b"c", 3.0)]
```
  </action>
  <verify>
    <automated>cd /Users/desertaxle/dev/prefectlabs/burner-redis && maturin develop 2>&1 | tail -3 && python -m pytest tests/test_sorted_sets.py -v 2>&1 | tail -40</automated>
  </verify>
  <acceptance_criteria>
    - test -f tests/test_sorted_sets.py (file exists)
    - grep -q "ZSET-01" tests/test_sorted_sets.py (requirement ZSET-01 referenced)
    - grep -q "ZSET-02" tests/test_sorted_sets.py (requirement ZSET-02 referenced)
    - grep -q "ZSET-03" tests/test_sorted_sets.py (requirement ZSET-03 referenced)
    - grep -q "ZSET-04" tests/test_sorted_sets.py (requirement ZSET-04 referenced)
    - grep -q "ZSET-05" tests/test_sorted_sets.py (requirement ZSET-05 referenced)
    - grep -q "ZSET-06" tests/test_sorted_sets.py (requirement ZSET-06 referenced)
    - grep -q "test_zadd" tests/test_sorted_sets.py (zadd tests present)
    - grep -q "test_zrem" tests/test_sorted_sets.py (zrem tests present)
    - grep -q "test_zrange" tests/test_sorted_sets.py (zrange tests present)
    - grep -q "test_zrangebyscore" tests/test_sorted_sets.py (zrangebyscore tests present)
    - grep -q "test_zrangestore" tests/test_sorted_sets.py (zrangestore tests present)
    - grep -q "test_zremrangebyscore" tests/test_sorted_sets.py (zremrangebyscore tests present)
    - grep -q "WRONGTYPE" tests/test_sorted_sets.py (wrongtype tests present)
    - grep -q "withscores" tests/test_sorted_sets.py (withscores tests present)
    - grep -q "inf" tests/test_sorted_sets.py (infinity bound tests present)
    - All pytest tests pass with 0 failures
    - python -m pytest tests/ passes (full suite including strings, hashes, sets still green)
  </acceptance_criteria>
  <done>
    - tests/test_sorted_sets.py has 35+ tests covering ZSET-01 through ZSET-06
    - ZADD tests cover all flags: NX, XX, GT, LT, CH with correct return value semantics
    - ZRANGE tests verify score ordering, index ranges, negative indices, withscores tuples
    - ZRANGEBYSCORE tests cover float bounds, -inf/+inf string bounds, withscores
    - ZRANGESTORE tests verify destination key contents and count return
    - ZREMRANGEBYSCORE tests verify removal count and remaining members
    - WRONGTYPE errors tested for all commands
    - Full test suite (strings + hashes + sets + sorted sets) passes with no regressions
  </done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|----------|-------------|
| Python args -> Rust Store | User-provided scores, members, min/max bounds cross from Python into Rust |
| Score string parsing | "-inf"/"+inf" strings parsed to f64::NEG_INFINITY/INFINITY |

## STRIDE Threat Register

| Threat ID | Category | Component | Disposition | Mitigation Plan |
|-----------|----------|-----------|-------------|-----------------|
| T-03-04 | Information Disclosure | WRONGTYPE error message | accept | Error message matches Redis verbatim -- no internal state leaked |
| T-03-05 | Tampering | Score bound parsing | mitigate | parse_score_bound validates input is float or recognized string; raises PyValueError for invalid input |
| T-03-06 | Tampering | ZADD mapping extraction | mitigate | Validate mapping is a dict via PyO3 PyDict extraction; score extracted as f64 with type error on invalid |
</threat_model>

<verification>
- `maturin develop` compiles and installs the package
- `python -m pytest tests/` passes with all tests green (strings + hashes + sets + sorted sets)
- WRONGTYPE is raised correctly when operating on wrong type
- Return types match redis-py: zadd->int, zrem->int, zrange->list[bytes] or list[tuple[bytes,float]], zrangebyscore same, zrangestore->int, zremrangebyscore->int
- ZADD flags (NX/XX/GT/LT/CH) produce correct behavior
- Score bounds accept both float and string ("-inf", "+inf")
</verification>

<success_criteria>
- All 6 async Python methods work and match redis.asyncio.Redis signatures
- ZADD returns count of new members (or changed if CH=true) with correct NX/XX/GT/LT flag behavior
- ZRANGE and ZRANGEBYSCORE return list[bytes] without withscores, list[tuple[bytes, float]] with withscores
- ZRANGEBYSCORE accepts "-inf" and "+inf" string bounds
- ZRANGESTORE stores range result and returns count
- ZREMRANGEBYSCORE removes and returns count
- WRONGTYPE exceptions raised for cross-type operations
- Full test suite passes with no regressions
- All Phase 3 requirements (ZSET-01 through ZSET-06) verified
</success_criteria>

<output>
After completion, create `.planning/phases/03-sorted-set-commands/03-02-SUMMARY.md`
</output>
