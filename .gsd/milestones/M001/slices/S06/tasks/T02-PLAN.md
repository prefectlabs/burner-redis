# T02: Add Python async bindings for EVAL, EVALSHA, SCRIPT LOAD, and SCRIPT EXISTS commands, plus a comprehensive pytest suite that validates Lua scripting end-to-end including redis.

**Slice:** S06 — **Milestone:** M001

## Description

Add Python async bindings for EVAL, EVALSHA, SCRIPT LOAD, and SCRIPT EXISTS commands, plus a comprehensive pytest suite that validates Lua scripting end-to-end including redis.call() dispatch to all data types.

Purpose: Expose the Lua scripting engine to Python so Prefect's atomic Lua scripts work as drop-in replacements for redis.asyncio.Redis eval/evalsha calls.

Output: Python methods in BurnerRedis class and `tests/test_scripting.py` with full coverage of all 5 LUA requirements.

## Legacy Source

---
phase: 06-lua-scripting
plan: 02
type: execute
wave: 2
depends_on: ["06-01"]
files_modified:
  - src/lib.rs
  - tests/test_scripting.py
autonomous: true
requirements:
  - LUA-01
  - LUA-02
  - LUA-03
  - LUA-04
  - LUA-05

must_haves:
  truths:
    - "User can EVAL a Lua script with KEYS and ARGV arrays and receive correct return values"
    - "User can EVALSHA to execute a cached script by its SHA1 hash"
    - "User can SCRIPT LOAD to cache a script and get its SHA1 hash"
    - "User can SCRIPT EXISTS to check whether one or more scripts are cached"
    - "Lua scripts calling redis.call() correctly interact with all data types (string, hash, set, sorted set, stream)"
    - "EVALSHA with unknown SHA1 raises NOSCRIPT error"
  artifacts:
    - path: "src/lib.rs"
      provides: "Python async methods for eval, evalsha, script_load, script_exists"
      contains: "fn eval"
    - path: "tests/test_scripting.py"
      provides: "Comprehensive pytest suite covering LUA-01 through LUA-05"
      contains: "test_eval"
  key_links:
    - from: "src/lib.rs"
      to: "src/store.rs"
      via: "store.eval(), store.evalsha(), store.script_load(), store.script_exists()"
      pattern: "store\\.eval\\|store\\.evalsha\\|store\\.script_load\\|store\\.script_exists"
    - from: "tests/test_scripting.py"
      to: "src/lib.rs"
      via: "await r.eval(), await r.evalsha(), await r.script_load(), await r.script_exists()"
      pattern: "await r\\.(eval|evalsha|script_load|script_exists)"
---

<objective>
Add Python async bindings for EVAL, EVALSHA, SCRIPT LOAD, and SCRIPT EXISTS commands, plus a comprehensive pytest suite that validates Lua scripting end-to-end including redis.call() dispatch to all data types.

Purpose: Expose the Lua scripting engine to Python so Prefect's atomic Lua scripts work as drop-in replacements for redis.asyncio.Redis eval/evalsha calls.

Output: Python methods in BurnerRedis class and `tests/test_scripting.py` with full coverage of all 5 LUA requirements.
</objective>

<execution_context>
@$HOME/.claude/get-shit-done/workflows/execute-plan.md
@$HOME/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@.planning/PROJECT.md
@.planning/ROADMAP.md
@.planning/STATE.md
@.planning/phases/06-lua-scripting/06-01-SUMMARY.md
@src/store.rs
@src/lib.rs
@src/scripting.rs
@tests/conftest.py
@tests/test_streams.py

<interfaces>
<!-- Key types and contracts from Plan 01 that the executor needs -->

From src/scripting.rs (created in Plan 01):
```rust
pub enum RedisValue {
    BulkString(Bytes),
    Integer(i64),
    Array(Vec<RedisValue>),
    Nil,
    Error(String),
    Status(String),
}

pub struct LuaEngine;

impl LuaEngine {
    pub fn sha1_hex(script: &str) -> String;
    pub fn execute(
        script: &str,
        keys: Vec<Bytes>,
        args: Vec<Bytes>,
        data: &mut HashMap<Bytes, ValueEntry>,
    ) -> Result<RedisValue, String>;
}
```

From src/store.rs (updated in Plan 01):
```rust
pub struct Store {
    data: RwLock<HashMap<Bytes, ValueEntry>>,
    scripts: RwLock<HashMap<String, String>>,
}

impl Store {
    pub fn script_load(&self, script: &str) -> String;
    pub fn script_exists(&self, shas: &[String]) -> Vec<bool>;
    pub fn eval(&self, script: &str, keys: Vec<Bytes>, args: Vec<Bytes>) -> Result<RedisValue, String>;
    pub fn evalsha(&self, sha: &str, keys: Vec<Bytes>, args: Vec<Bytes>) -> Result<RedisValue, String>;
}
```

From src/lib.rs (existing pattern):
```rust
pub struct BurnerRedis {
    store: Arc<Store>,
}

// Async method pattern:
fn command<'py>(&self, py: Python<'py>, ...) -> PyResult<Bound<'py, PyAny>> {
    let store = self.store.clone();
    pyo3_async_runtimes::tokio::future_into_py(py, async move { ... })
}
```
</interfaces>
</context>

<tasks>

<task type="auto">
  <name>Task 1: Add Python async methods for eval, evalsha, script_load, script_exists</name>
  <files>src/lib.rs</files>
  <read_first>src/lib.rs, src/store.rs, src/scripting.rs</read_first>
  <action>
1. Add import for scripting module types at the top of lib.rs:
   ```rust
   use scripting::RedisValue;
   ```

2. Add a helper function to convert `RedisValue` to a Python object. This function must handle recursive conversion for nested arrays:

   ```rust
   fn redis_value_to_py(py: Python<'_>, val: RedisValue) -> PyResult<PyObject> {
       match val {
           RedisValue::BulkString(b) => Ok(PyBytes::new(py, &b).into()),
           RedisValue::Integer(n) => Ok(n.into_pyobject(py)?.into()),
           RedisValue::Nil => Ok(py.None()),
           RedisValue::Status(s) => Ok(PyBytes::new(py, s.as_bytes()).into()),
           RedisValue::Error(msg) => Err(pyo3::exceptions::PyException::new_err(msg)),
           RedisValue::Array(items) => {
               let py_items: PyResult<Vec<PyObject>> = items
                   .into_iter()
                   .map(|item| redis_value_to_py(py, item))
                   .collect();
               Ok(pyo3::types::PyList::new(py, &py_items?)?.into())
           }
       }
   }
   ```

   Add necessary imports: `use pyo3::types::PyBytes;`

3. Add a `// -- Scripting Commands --` section to BurnerRedis #[pymethods] with these methods:

   **eval method** matching redis-py signature `eval(script, numkeys, *keys_and_args)`:
   ```rust
   #[pyo3(signature = (script, numkeys, *keys_and_args))]
   fn eval<'py>(
       &self,
       py: Python<'py>,
       script: String,
       numkeys: usize,
       keys_and_args: &Bound<'py, pyo3::types::PyTuple>,
   ) -> PyResult<Bound<'py, PyAny>>
   ```
   - Extract `numkeys` keys from the beginning of keys_and_args tuple, remaining are args.
   - Convert each key/arg to Bytes using extract_bytes.
   - Call `store.eval(&script, keys, args)`.
   - On Ok(RedisValue), use `Python::with_gil` or `Python::try_attach` inside the async block to convert via `redis_value_to_py`. Since `future_into_py` gives us an async block, and we need GIL to construct Python objects, use `Python::with_gil(|py| redis_value_to_py(py, result))` inside the async move block.
   - On Err(msg), return `PyException::new_err(msg)`.

   **evalsha method** matching redis-py signature `evalsha(sha, numkeys, *keys_and_args)`:
   ```rust
   #[pyo3(signature = (sha, numkeys, *keys_and_args))]
   fn evalsha<'py>(
       &self,
       py: Python<'py>,
       sha: String,
       numkeys: usize,
       keys_and_args: &Bound<'py, pyo3::types::PyTuple>,
   ) -> PyResult<Bound<'py, PyAny>>
   ```
   - Same pattern as eval but calls `store.evalsha(&sha, keys, args)`.
   - NOSCRIPT error from store should propagate as Python exception.

   **script_load method** (not in redis-py's async interface but needed):
   ```rust
   fn script_load<'py>(
       &self,
       py: Python<'py>,
       script: String,
   ) -> PyResult<Bound<'py, PyAny>>
   ```
   - Call `store.script_load(&script)`.
   - Return the SHA1 hex string as Python str.

   **script_exists method**:
   ```rust
   #[pyo3(signature = (*args))]
   fn script_exists<'py>(
       &self,
       py: Python<'py>,
       args: &Bound<'py, pyo3::types::PyTuple>,
   ) -> PyResult<Bound<'py, PyAny>>
   ```
   - Extract each arg as a String (SHA1 hex).
   - Call `store.script_exists(&shas)`.
   - Return as Python list of bools.

IMPORTANT: The redis-py `eval()` signature is `eval(script, numkeys, *keys_and_args)` where the first `numkeys` positional args after numkeys are KEYS and the rest are ARGV. Follow this exact signature.

IMPORTANT: Use `future_into_py` for all four methods to maintain the async pattern even though the underlying Store calls are synchronous (the write lock acquisition may block briefly). This keeps the API consistent with all other BurnerRedis methods.
  </action>
  <verify>
    <automated>cd /Users/desertaxle/dev/prefectlabs/burner-redis && cargo build 2>&1 | tail -10</automated>
  </verify>
  <acceptance_criteria>
    - grep -q "fn eval" src/lib.rs
    - grep -q "fn evalsha" src/lib.rs
    - grep -q "fn script_load" src/lib.rs
    - grep -q "fn script_exists" src/lib.rs
    - grep -q "redis_value_to_py" src/lib.rs
    - grep -q "numkeys" src/lib.rs
    - grep -q "NOSCRIPT\|noscript\|store.evalsha" src/lib.rs
    - cargo build succeeds with no errors
  </acceptance_criteria>
  <done>BurnerRedis has eval, evalsha, script_load, and script_exists async methods matching redis-py signatures. RedisValue-to-Python conversion handles all types including nested arrays. eval/evalsha accept (script/sha, numkeys, *keys_and_args) splitting KEYS from ARGV. Compiles without errors.</done>
</task>

<task type="auto">
  <name>Task 2: Comprehensive pytest suite for Lua scripting commands</name>
  <files>tests/test_scripting.py</files>
  <read_first>tests/conftest.py, tests/test_streams.py, src/lib.rs, src/scripting.rs</read_first>
  <action>
Create `tests/test_scripting.py` with the following structure:

```python
"""Tests for Lua scripting commands: EVAL, EVALSHA, SCRIPT LOAD, SCRIPT EXISTS.

Covers requirements: LUA-01, LUA-02, LUA-03, LUA-04, LUA-05.
"""
import hashlib
import pytest
from burner_redis import BurnerRedis
```

All tests are async, use the `r` fixture from conftest.py.

**LUA-01 (EVAL with KEYS and ARGV):**

- `test_eval_return_string`: `eval("return 'hello'", 0)` returns `b"hello"`
- `test_eval_return_integer`: `eval("return 42", 0)` returns `42`
- `test_eval_return_nil`: `eval("return nil", 0)` returns `None` (false in Lua -> nil in Redis -> None in Python)
- `test_eval_return_table_array`: `eval("return {1, 2, 3}", 0)` returns `[1, 2, 3]`
- `test_eval_return_false`: `eval("return false", 0)` returns `None`
- `test_eval_keys_and_argv`: `eval("return {KEYS[1], ARGV[1]}", 1, "mykey", "myarg")` returns `[b"mykey", b"myarg"]`
- `test_eval_numkeys_zero`: `eval("return ARGV[1]", 0, "arg1")` returns `b"arg1"` (all args are ARGV when numkeys=0)
- `test_eval_multiple_keys`: `eval("return {KEYS[1], KEYS[2]}", 2, "k1", "k2")` returns `[b"k1", b"k2"]`

**LUA-02 (EVALSHA):**

- `test_evalsha_after_script_load`: Load a script via `script_load`, then `evalsha(sha, 0)` returns the expected result
- `test_evalsha_after_eval`: Run `eval(script, 0)`, compute SHA1 in Python (`hashlib.sha1(script.encode()).hexdigest()`), then `evalsha(sha, 0)` works (auto-cache)
- `test_evalsha_unknown_sha_raises`: `evalsha("deadbeef" * 5, 0)` raises an exception containing "NOSCRIPT"
- `test_evalsha_with_keys_and_args`: Load a script that uses KEYS/ARGV, evalsha with keys and args returns correct result

**LUA-03 (redis.call() and redis.pcall()):**

String commands via redis.call():
- `test_redis_call_set_get`: `eval("redis.call('SET', KEYS[1], ARGV[1]); return redis.call('GET', KEYS[1])", 1, "foo", "bar")` returns `b"bar"`
- `test_redis_call_del`: Set a key, then `eval("return redis.call('DEL', KEYS[1])", 1, "foo")` returns `1`
- `test_redis_call_exists`: Set a key, then `eval("return redis.call('EXISTS', KEYS[1])", 1, "foo")` returns `1`

Hash commands via redis.call():
- `test_redis_call_hset_hget`: `eval("redis.call('HSET', KEYS[1], 'field1', ARGV[1]); return redis.call('HGET', KEYS[1], 'field1')", 1, "myhash", "val1")` returns `b"val1"`
- `test_redis_call_hdel`: Set hash field, then eval HDEL returns 1
- `test_redis_call_hvals`: Set hash fields, then eval HVALS returns array of values

Set commands via redis.call():
- `test_redis_call_sadd_smembers`: `eval("redis.call('SADD', KEYS[1], ARGV[1], ARGV[2]); return redis.call('SMEMBERS', KEYS[1])", 1, "myset", "a", "b")` returns a list containing `b"a"` and `b"b"` (order may vary)
- `test_redis_call_sismember`: Add member, eval SISMEMBER returns 1 for member, 0 for non-member
- `test_redis_call_srem`: Add then remove member via eval, returns 1

Sorted set commands via redis.call():
- `test_redis_call_zadd_zrange`: `eval("redis.call('ZADD', KEYS[1], '1.0', 'a', '2.0', 'b'); return redis.call('ZRANGE', KEYS[1], '0', '-1')", 1, "zs")` returns `[b"a", b"b"]`
- `test_redis_call_zrem`: Add sorted set member, eval ZREM returns 1
- `test_redis_call_zrangebyscore`: Add members with scores, eval ZRANGEBYSCORE returns members in range
- `test_redis_call_zremrangebyscore`: Add members, eval ZREMRANGEBYSCORE returns count removed

Stream commands via redis.call():
- `test_redis_call_xadd_xread`: `eval("local id = redis.call('XADD', KEYS[1], '*', 'f1', 'v1'); return id", 1, "stream")` returns a stream ID bytes value

redis.pcall():
- `test_redis_pcall_success`: `eval("local ok, err = pcall(function() return redis.call('SET', KEYS[1], 'v') end); return redis.call('GET', KEYS[1])", 1, "k")` returns `b"v"`
- `test_redis_pcall_error`: `eval("return redis.pcall('HSET', KEYS[1], 'f', 'v')", 1, "k")` after setting k as a string should return error table (the Python representation depends on how error tables are converted -- may be None or raise; test the actual behavior)
- `test_redis_call_wrongtype_raises`: Set k as string, then `eval("return redis.call('HSET', KEYS[1], 'f', 'v')", 1, "k")` should raise an exception containing "WRONGTYPE"
- `test_redis_call_unknown_command`: `eval("return redis.call('FLUSHALL')", 0)` should raise an exception containing "unknown command"

**LUA-04 (SCRIPT LOAD):**

- `test_script_load_returns_sha1`: `script_load("return 1")` returns a 40-character hex string
- `test_script_load_sha1_matches_python`: SHA1 from script_load matches `hashlib.sha1(script.encode()).hexdigest()`
- `test_script_load_idempotent`: Loading the same script twice returns the same SHA1

**LUA-05 (SCRIPT EXISTS):**

- `test_script_exists_loaded`: Load a script, `script_exists(sha)` returns `[True]`
- `test_script_exists_not_loaded`: `script_exists("deadbeef" * 5)` returns `[False]`
- `test_script_exists_multiple`: Load one script, `script_exists(sha1, "unknown")` returns `[True, False]`
- `test_script_exists_after_eval`: Run eval with a script, check script_exists with computed SHA1 returns `[True]`

**Regression:**

- `test_full_regression`: At the end, run the full test suite to confirm no regressions. This is handled by the verify command below, not as a test case.

IMPORTANT: For tests that check exception messages, use `pytest.raises(Exception, match="...")` with the expected substring (e.g., "WRONGTYPE", "NOSCRIPT", "unknown command").

IMPORTANT: All string/key/value arguments to eval must be passed as positional args after numkeys, NOT as keyword args. Match redis-py calling convention: `r.eval(script, numkeys, key1, key2, arg1, arg2)`.
  </action>
  <verify>
    <automated>cd /Users/desertaxle/dev/prefectlabs/burner-redis && maturin develop 2>&1 | tail -3 && python -m pytest tests/test_scripting.py -x -v 2>&1 | tail -40</automated>
  </verify>
  <acceptance_criteria>
    - grep -q "LUA-01" tests/test_scripting.py
    - grep -q "LUA-02" tests/test_scripting.py
    - grep -q "LUA-03" tests/test_scripting.py
    - grep -q "LUA-04" tests/test_scripting.py
    - grep -q "LUA-05" tests/test_scripting.py
    - grep -q "test_eval_return_string" tests/test_scripting.py
    - grep -q "test_evalsha_after_script_load" tests/test_scripting.py
    - grep -q "test_redis_call_set_get" tests/test_scripting.py
    - grep -q "test_script_load_returns_sha1" tests/test_scripting.py
    - grep -q "test_script_exists_loaded" tests/test_scripting.py
    - grep -q "test_redis_call_wrongtype_raises" tests/test_scripting.py
    - grep -q "test_redis_pcall" tests/test_scripting.py
    - grep -q "hashlib" tests/test_scripting.py
    - python -m pytest tests/test_scripting.py passes
    - python -m pytest tests/ passes (full regression)
  </acceptance_criteria>
  <done>Comprehensive pytest suite in tests/test_scripting.py covers all 5 LUA requirements. Tests validate: EVAL with various return types and KEYS/ARGV (LUA-01), EVALSHA with loaded and auto-cached scripts (LUA-02), redis.call() dispatching to string/hash/set/sorted-set/stream commands plus redis.pcall() error handling (LUA-03), SCRIPT LOAD returning correct SHA1 (LUA-04), SCRIPT EXISTS checking cache for single and multiple scripts (LUA-05). Full regression suite passes with zero regressions.</done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|----------|-------------|
| Python -> Rust | User-supplied Lua script text, keys, and args cross into Rust for execution |

## STRIDE Threat Register

| Threat ID | Category | Component | Disposition | Mitigation Plan |
|-----------|----------|-----------|-------------|-----------------|
| T-06-05 | Tampering | EVALSHA hash lookup | mitigate | SHA1 hex validated as 40-char hex string; lookup returns NOSCRIPT error for unknown hashes |
| T-06-06 | Denial of Service | Large script text | accept | No script size limit; in-process library, user controls own input |
</threat_model>

<verification>
1. `cargo build` compiles without errors
2. `maturin develop` installs the updated Python package
3. `python -m pytest tests/test_scripting.py -v` all scripting tests pass
4. `python -m pytest tests/ -v` full regression suite passes
5. EVAL returns correct types (string, integer, nil, array)
6. EVALSHA works with both SCRIPT LOAD and auto-cached scripts
7. redis.call() dispatches correctly to all data types
8. SCRIPT EXISTS returns correct boolean list
</verification>

<success_criteria>
- eval() accepts (script, numkeys, *keys_and_args) matching redis-py
- evalsha() accepts (sha, numkeys, *keys_and_args) matching redis-py
- script_load() returns 40-char SHA1 hex string
- script_exists() returns list[bool] for multiple SHAs
- RedisValue to Python conversion handles all types recursively
- Tests cover all 5 LUA requirements with multiple test cases each
- Full test suite (all prior phases) passes with zero regressions
</success_criteria>

<output>
After completion, create `.planning/phases/06-lua-scripting/06-02-SUMMARY.md`
</output>
