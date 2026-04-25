---
phase: 260425-ftl
plan: 01
type: execute
wave: 1
depends_on: []
files_modified:
  - src/commands/strings.rs
  - src/lib.rs
  - tests/test_lists.py
autonomous: true
requirements:
  - P3-list-option-tokens-accept-bytes
must_haves:
  truths:
    - "r.linsert('k', b'BEFORE', pivot, value) succeeds with the same result as the str form"
    - "r.linsert('k', b'AFTER', pivot, value) succeeds with the same result as the str form"
    - "r.lmove(src, dst, src=b'LEFT', dest=b'RIGHT') succeeds with the same result as the str form"
    - "r.blmove(src, dst, timeout=1.0, src=b'LEFT', dest=b'RIGHT') succeeds with the same result as the str form"
    - "Pipeline LINSERT and LMOVE through execute_command accept bytes for option tokens"
    - "Lowercase / mixed-case bytes (b'before', b'Left') are accepted (case-insensitive, mirrors str path)"
    - "Invalid UTF-8 bytes (e.g. b'\\xff') for option tokens raise the same ResponseError surface as an unknown str token"
    - "Existing str-based call sites continue to work unchanged"
    - "cargo check passes; pytest tests/test_lists.py passes"
  artifacts:
    - path: "src/commands/strings.rs"
      provides: "extract_token_str helper that accepts str or bytes and returns String"
      contains: "fn extract_token_str"
    - path: "src/lib.rs"
      provides: "linsert / lmove / blmove pymethods + execute_command linsert/lmove arms accepting str-or-bytes option tokens"
      contains: "extract_token_str"
    - path: "tests/test_lists.py"
      provides: "Bytes-token coverage for LINSERT, LMOVE, BLMOVE"
      contains: "b\"BEFORE\""
  key_links:
    - from: "src/lib.rs linsert/lmove/blmove pymethods"
      to: "src/commands/strings.rs::extract_token_str"
      via: "direct call replacing &str signature parameter"
      pattern: "extract_token_str\\("
    - from: "extract_token_str output"
      to: "src/commands/lists.rs::parse_linsert_where / parse_list_end"
      via: "owned String passed as &str — existing case-insensitive parsers untouched"
      pattern: "parse_(linsert_where|list_end)\\(&"
---

<objective>
Fix P3: list-command option tokens (LINSERT where, LMOVE/BLMOVE src/dest) currently reject `bytes` at the PyO3 boundary because their pymethod parameters are typed `&str`. Real Redis + redis-py accept either; redis-py encodes str→bytes before sending. Once a user invokes a Pipeline or builds with `Redis.execute_command(...)` while passing pre-encoded tokens, our binding raises `TypeError` before reaching the case-insensitive parsers.

Add a small `extract_token_str(obj) -> PyResult<String>` helper next to the existing `extract_bytes` in `src/commands/strings.rs`, then change all five call sites in `src/lib.rs` to take `&Bound<'py, PyAny>` for the option token, decode via the helper, and pass the resulting `&str` into the existing `parse_linsert_where` / `parse_list_end` (which stay byte-identical). Cover the new path with Python integration tests.

Purpose: Drop-in `redis.asyncio.Redis` compatibility — Prefect/pydocket and any redis-py user that passes pre-encoded bytes for these tokens must work without a TypeError.

Output: 1 helper, 5 patched call sites, 3 new test functions, no behavior change for existing str callers.
</objective>

<execution_context>
@$HOME/.claude/get-shit-done/workflows/execute-plan.md
@$HOME/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@./CLAUDE.md
@src/commands/strings.rs
@src/commands/lists.rs

<interfaces>
<!-- Existing relevant signatures the executor will use directly. -->

From src/commands/strings.rs (existing — pattern to mirror):
```rust
pub fn extract_bytes(obj: &Bound<'_, PyAny>) -> PyResult<Bytes> {
    if let Ok(s) = obj.extract::<String>() {
        Ok(Bytes::from(s.into_bytes()))
    } else if let Ok(b) = obj.extract::<Vec<u8>>() {
        Ok(Bytes::from(b))
    } else {
        Err(pyo3::exceptions::PyTypeError::new_err("expected str or bytes"))
    }
}
```

From src/commands/lists.rs (existing — unchanged, consumed by helper output):
```rust
pub fn parse_list_end(s: &str) -> Result<ListEnd, StoreError>;          // "LEFT" / "RIGHT" (case-insensitive)
pub fn parse_linsert_where(s: &str) -> Result<InsertPosition, StoreError>; // "BEFORE" / "AFTER" (case-insensitive)
// Both return StoreError::Syntax(format!("ERR syntax error: expected ..., got {}", s)) on unknown tokens.
// store_err_to_py converts StoreError::Syntax → ResponseError on the Python side.
```

The five call sites in src/lib.rs that need updating (Task 1):
- `fn linsert` — line 2382 — change `r#where: &str` to `&Bound<'py, PyAny>`, decode with `extract_token_str` before `parse_linsert_where`
- `fn lmove` — line 2464 — change `src: &str, dest: &str` to `&Bound<'py, PyAny>` each
- `fn blmove` — line 2750 — change `src: &str, dest: &str` to `&Bound<'py, PyAny>` each
- `execute_command` `"linsert"` arm — line 3697 — replace `args.get_item(1)?.extract::<String>()?` with `extract_token_str(&args.get_item(1)?)?`
- `execute_command` `"lmove"` arm — lines 3750/3756 — replace `v.extract::<String>()` with `extract_token_str(&v)` for both `src` and `dest` kwargs (preserves the None / default-token fallback)

Note on signature defaults: PyO3's `#[pyo3(signature = (..., src="LEFT", dest="RIGHT"))]` requires the parameter to be a Rust value, not a Bound. Replace string defaults with `None` and `Option<&Bound<'py, PyAny>>`, then fall back to `"LEFT"` / `"RIGHT"` when `None`. This preserves the redis-py default-arg behavior:
```rust
#[pyo3(signature = (first_list, second_list, src=None, dest=None))]
fn lmove<'py>(
    &self,
    py: Python<'py>,
    first_list: &Bound<'py, PyAny>,
    second_list: &Bound<'py, PyAny>,
    src: Option<&Bound<'py, PyAny>>,
    dest: Option<&Bound<'py, PyAny>>,
) -> PyResult<Bound<'py, PyAny>> {
    let src_str = match src { Some(o) => extract_token_str(o)?, None => "LEFT".to_string() };
    let dst_str = match dest { Some(o) => extract_token_str(o)?, None => "RIGHT".to_string() };
    let src_end = parse_list_end(&src_str).map_err(store_err_to_py)?;
    let dst_end = parse_list_end(&dst_str).map_err(store_err_to_py)?;
    // ... rest unchanged
}
```
Apply the same `Option<&Bound<'py, PyAny>>` pattern to `blmove`. For `linsert`, `r#where` has no default (it's required positional), so use plain `&Bound<'py, PyAny>`.
</interfaces>
</context>

<tasks>

<task type="auto" tdd="true">
  <name>Task 1: Add extract_token_str helper and apply to all 5 list option-token sites</name>
  <files>src/commands/strings.rs, src/lib.rs</files>
  <behavior>
    Behavior contract (verified by Task 2 Python tests):
    - extract_token_str accepts a Python str → returns the String unchanged (e.g. "BEFORE" → Ok("BEFORE".to_string()))
    - extract_token_str accepts Python bytes containing valid UTF-8 → decodes and returns the String (e.g. b"BEFORE" → Ok("BEFORE".to_string()))
    - extract_token_str on Python bytes with invalid UTF-8 (e.g. b"\xff") → returns a StoreError::Syntax-shaped error so it surfaces through `store_err_to_py` as a ResponseError matching the unknown-token path (NOT a TypeError). Concretely: map the from_utf8 failure to `pyo3::exceptions::PyTypeError::new_err(...)` is WRONG — instead, return an error that, when converted, raises ResponseError. Easiest approach: convert from_utf8 error into a String "<invalid-utf8>" and let parse_linsert_where / parse_list_end emit the syntax error themselves. See implementation note below.
    - extract_token_str on neither str nor bytes (e.g. an int) → returns PyTypeError "expected str or bytes" (matches extract_bytes behavior)
    - linsert / lmove / blmove pymethods: when called with bytes tokens (b"BEFORE", b"LEFT", etc.), behave identically to the str form
    - Pipeline-style execute_command("linsert", ...) and execute_command("lmove", ..., src=b"LEFT") accept bytes tokens
    - Existing str callers unchanged (no regression)
  </behavior>
  <action>
    **Step 1 — Add the helper to `src/commands/strings.rs`:**
    Place immediately after `extract_bytes` (before `extract_expiry`). Implementation:
    ```rust
    /// Extract a list-command option token (e.g. "BEFORE"/"AFTER", "LEFT"/"RIGHT")
    /// from a Python object that is either str or bytes. Bytes are decoded as UTF-8.
    /// On invalid UTF-8 we return a placeholder string that the downstream
    /// case-insensitive parsers (parse_linsert_where / parse_list_end) will reject
    /// via StoreError::Syntax → ResponseError, matching real-Redis semantics for
    /// unknown option tokens.
    pub fn extract_token_str(obj: &Bound<'_, PyAny>) -> PyResult<String> {
        if let Ok(s) = obj.extract::<String>() {
            return Ok(s);
        }
        if let Ok(b) = obj.extract::<Vec<u8>>() {
            return Ok(String::from_utf8(b).unwrap_or_else(|e| {
                // Use the lossy form so the parser sees *something* it can echo back
                // in its error message; the parser will still reject it as unknown.
                String::from_utf8_lossy(e.as_bytes()).into_owned()
            }));
        }
        Err(pyo3::exceptions::PyTypeError::new_err(
            "expected str or bytes",
        ))
    }
    ```
    Rationale for not erroring on invalid UTF-8 directly: real Redis treats unknown option tokens as a syntax error (ResponseError). If we raise PyTypeError on `b"\xff"` we'd diverge — `b"BEFORE\xff"` and `b"\xff"` should both produce the same ResponseError surface as an unknown str token. Using `from_utf8_lossy` on the error's bytes feeds *something* into the existing parser, which then emits `StoreError::Syntax(...)` → `store_err_to_py` → `ResponseError`. This is the cleanest reuse of the existing error pipeline (per Phase 14 decision: ResponseError class wraps StoreError::Syntax).

    **Step 2 — Update the import in `src/lib.rs` line 15:**
    ```rust
    use commands::strings::{extract_bytes, extract_expiry, extract_token_str};
    ```

    **Step 3 — Patch `fn linsert` (lib.rs:2382-2400):**
    - Change `r#where: &str` to `r#where: &Bound<'py, PyAny>`
    - Replace `parse_linsert_where(r#where)` with `parse_linsert_where(&extract_token_str(r#where)?)`

    **Step 4 — Patch `fn lmove` (lib.rs:2463-2486):**
    - Change `#[pyo3(signature = (first_list, second_list, src="LEFT", dest="RIGHT"))]` to `#[pyo3(signature = (first_list, second_list, src=None, dest=None))]`
    - Change params `src: &str, dest: &str` to `src: Option<&Bound<'py, PyAny>>, dest: Option<&Bound<'py, PyAny>>`
    - Decode with the helper, falling back to defaults when None:
      ```rust
      let src_str = match src { Some(o) => extract_token_str(o)?, None => "LEFT".to_string() };
      let dst_str = match dest { Some(o) => extract_token_str(o)?, None => "RIGHT".to_string() };
      let src_end = parse_list_end(&src_str).map_err(store_err_to_py)?;
      let dst_end = parse_list_end(&dst_str).map_err(store_err_to_py)?;
      ```

    **Step 5 — Patch `fn blmove` (lib.rs:2749-2758):**
    - Same Option-pattern as `lmove`. The body inside `future_into_py` already moves `src_end`/`dst_end` after parsing — ordering does not change.

    **Step 6 — Patch `execute_command` `"linsert"` arm (lib.rs:3694-3710):**
    - Replace `let where_str: String = args.get_item(1)?.extract()?;` with:
      ```rust
      let where_str = extract_token_str(&args.get_item(1)?)?;
      ```

    **Step 7 — Patch `execute_command` `"lmove"` arm (lib.rs:3743-3774):**
    - Replace each `.map(|v| v.extract::<String>())` chain with `.map(|v| extract_token_str(&v))`. Final shape:
      ```rust
      let src_str: String = kwargs
          .get_item("src")?
          .and_then(|v| if v.is_none() { None } else { Some(v) })
          .map(|v| extract_token_str(&v))
          .transpose()?
          .unwrap_or_else(|| "LEFT".to_string());
      // ditto for dest_str
      ```

    **Step 8 — Audit other list pymethods near the touched code for similar `&str` option tokens:**
    Run `grep -n 'fn lpush\|fn rpush\|fn lpop\|fn rpop\|fn lpos\|fn lrem\|fn lset\|fn ltrim\|fn rpoplpush\|fn brpop\|fn blpop' src/lib.rs` and verify none take a string-typed *option token* (not a key/value/count). Per the constraints, `lpop count` is `i64`, not a token — leave alone. Confirm no other site needs the helper. **Do NOT** apply the helper to keys/values/pivots — those already use `extract_bytes`.

    **Verification gate before Task 2:**
    Run `cargo check` and confirm the crate compiles. Run `maturin develop` (or `pip install -e .` if maturin develop is the established dev workflow) so the Python tests in Task 2 pick up the new binding.
  </action>
  <verify>
    <automated>cargo check 2>&amp;1 | tail -20</automated>
  </verify>
  <done>
    - `extract_token_str` exists in src/commands/strings.rs and is `pub`
    - `src/lib.rs` line 15 imports it alongside `extract_bytes` and `extract_expiry`
    - All 5 sites use `extract_token_str` (no remaining `&str` parameter for an option token in linsert/lmove/blmove pymethods or their execute_command arms)
    - `cargo check` is clean (no warnings about unused imports / dead code introduced by this change)
    - `cargo build --release` succeeds (or `maturin develop` succeeds — whichever the project uses; check Cargo.toml / pyproject.toml)
  </done>
</task>

<task type="auto" tdd="true">
  <name>Task 2: Add Python integration tests for bytes option tokens</name>
  <files>tests/test_lists.py</files>
  <behavior>
    Test cases (added to tests/test_lists.py, async functions using the existing `r` fixture from conftest.py):
    - LINSERT with `b"BEFORE"` produces the same list as the existing str-based test_linsert
    - LINSERT with `b"AFTER"` inserts after the pivot
    - LINSERT with lowercase bytes `b"before"` works (case-insensitive parity with the str path)
    - LMOVE with bytes tokens for src/dest in cross-key, same-key, and default-fallback shapes
    - BLMOVE with bytes tokens for src/dest succeeds within timeout
    - LINSERT with an unknown token bytes (e.g. b"SIDEWAYS") raises the same exception class as the unknown-str case (ResponseError or "syntax" message) — proves the error path reuses the existing parser, NOT a TypeError
    - LINSERT with invalid UTF-8 bytes (b"\xff") raises the same exception class — proves UTF-8 fallback feeds into the syntax-error path, NOT a TypeError leak
    - Pipeline LINSERT and LMOVE through `execute_command` (or the pipeline interface) accept bytes tokens — covers the dispatch arms
  </behavior>
  <action>
    **Step 1 — Append a new section to `tests/test_lists.py`** at the bottom of the file (after the last test). Use the same async-function + `r` fixture pattern visible in lines 13-216. Look at the existing `test_linsert` (line 135) and `test_lmove_cross_key` (line 203) for shape.

    **Step 2 — Add these tests** (use the actual exception class the existing tests use for malformed input — check `test_blpop` at ~line 360 or grep `pytest.raises` near other syntax-error sites in the file to confirm whether it's `ResponseError`, `redis.exceptions.ResponseError`, or `Exception, match="syntax"`). The tests below assume a generic `Exception, match="syntax"` pattern — adjust the `match` arg to whatever the project already uses if different:

    ```python
    # P3: bytes-token compatibility (mirrors real Redis + redis-py encoding behavior)
    # Verifies linsert/lmove/blmove + their execute_command dispatch arms accept
    # pre-encoded bytes for option tokens (where, src, dest), not just str.

    async def test_linsert_bytes_where_before(r):
        await r.rpush("k", "a", "c")
        n = await r.linsert("k", b"BEFORE", "c", "b")
        assert n == 3
        assert await r.lrange("k", 0, -1) == [b"a", b"b", b"c"]


    async def test_linsert_bytes_where_after(r):
        await r.rpush("k", "a", "c")
        n = await r.linsert("k", b"AFTER", "a", "b")
        assert n == 3
        assert await r.lrange("k", 0, -1) == [b"a", b"b", b"c"]


    async def test_linsert_bytes_where_lowercase(r):
        # Case-insensitive parity with the str path (parse_linsert_where uppercases internally)
        await r.rpush("k", "a", "c")
        n = await r.linsert("k", b"before", "c", "b")
        assert n == 3
        assert await r.lrange("k", 0, -1) == [b"a", b"b", b"c"]


    async def test_linsert_bytes_where_unknown_token(r):
        await r.rpush("k", "a")
        with pytest.raises(Exception, match="syntax"):
            await r.linsert("k", b"SIDEWAYS", "a", "b")


    async def test_linsert_bytes_where_invalid_utf8(r):
        await r.rpush("k", "a")
        # Invalid UTF-8 bytes must surface as a syntax error (same path as unknown token),
        # NOT a TypeError leak from the helper.
        with pytest.raises(Exception, match="syntax"):
            await r.linsert("k", b"\xff", "a", "b")


    async def test_lmove_bytes_tokens_cross_key(r):
        await r.rpush("src", "a", "b", "c")
        moved = await r.lmove("src", "dst", src=b"LEFT", dest=b"RIGHT")
        assert moved == b"a"
        assert await r.lrange("src", 0, -1) == [b"b", b"c"]
        assert await r.lrange("dst", 0, -1) == [b"a"]


    async def test_lmove_bytes_tokens_same_key_rotation(r):
        await r.rpush("k", "a", "b", "c")
        moved = await r.lmove("k", "k", src=b"RIGHT", dest=b"LEFT")
        assert moved == b"c"
        assert await r.lrange("k", 0, -1) == [b"c", b"a", b"b"]


    async def test_lmove_bytes_tokens_lowercase(r):
        await r.rpush("src", "a", "b", "c")
        moved = await r.lmove("src", "dst", src=b"left", dest=b"right")
        assert moved == b"a"


    async def test_blmove_bytes_tokens(r):
        await r.rpush("src", "a", "b", "c")
        moved = await r.blmove("src", "dst", timeout=1.0, src=b"LEFT", dest=b"RIGHT")
        assert moved == b"a"
        assert await r.lrange("dst", 0, -1) == [b"a"]


    # execute_command path (Pipeline dispatch) — covers the dispatch arms in lib.rs
    async def test_execute_command_linsert_bytes_where(r):
        await r.rpush("k", "a", "c")
        # Some redis-py versions accept positional via execute_command; prefer pipeline
        # if the project's pipeline interface is the real consumer of these arms.
        async with r.pipeline() as p:
            p.rpush("k2", "a", "c")
            p.linsert("k2", b"BEFORE", "c", "b")
            results = await p.execute()
        assert results[-1] == 3
        assert await r.lrange("k2", 0, -1) == [b"a", b"b", b"c"]


    async def test_execute_command_lmove_bytes_tokens(r):
        await r.rpush("src", "a", "b", "c")
        async with r.pipeline() as p:
            p.lmove("src", "dst", src=b"LEFT", dest=b"RIGHT")
            results = await p.execute()
        assert results[-1] == b"a"
        assert await r.lrange("dst", 0, -1) == [b"a"]
    ```

    **Step 3 — Calibrate the `pytest.raises(...)` exception class:**
    Run `grep -n "pytest.raises" tests/test_lists.py | head -10` to see what the file already uses for syntax/value errors. If existing tests use `redis.exceptions.ResponseError` or `burner_redis.ResponseError`, swap that in. The `match="syntax"` substring works against the StoreError::Syntax format string `ERR syntax error: expected ...`, so it should match regardless of the wrapping exception class.

    **Step 4 — If the pipeline tests fail because pipeline().linsert() / .lmove() pre-coerce the where/src/dest tokens at the Python wrapper layer (a Phase 14 D-? coercion):**
    Per Phase 14 decisions, value coercion happens at the Python wrapper for LPUSH/RPUSH/LSET/LINSERT but specifically for *values*, not option tokens. If the Pipeline.linsert() Python method nevertheless coerces the `where` arg to bytes/str before buffering, the test still verifies the right thing because the Rust dispatch arm now accepts both. If a wrapper rejects the bytes form before reaching Rust, drop just those two execute_command tests and add a note in the SUMMARY — the direct pymethod tests already cover the Rust side of the contract.
  </action>
  <verify>
    <automated>cd /Users/alexander/dev/prefectlabs/burner-redis &amp;&amp; maturin develop --release 2>&amp;1 | tail -3 &amp;&amp; pytest tests/test_lists.py -x -k "bytes_where or bytes_tokens or execute_command_linsert or execute_command_lmove" -v 2>&amp;1 | tail -40</automated>
  </verify>
  <done>
    - 11 new tests added to tests/test_lists.py
    - All new tests pass under `pytest tests/test_lists.py`
    - Existing tests in tests/test_lists.py still pass (no regression): `pytest tests/test_lists.py` is green end-to-end
    - The two execute_command tests either pass OR are documented as wrapper-blocked with a SUMMARY note; the direct-pymethod bytes tests must pass unconditionally
  </done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|----------|-------------|
| Python caller → Rust pymethod | Untrusted (in the threat-model sense) Python objects cross into Rust extract_* helpers. |

## STRIDE Threat Register

| Threat ID | Category | Component | Disposition | Mitigation Plan |
|-----------|----------|-----------|-------------|-----------------|
| T-260425-ftl-01 | Tampering | extract_token_str on bytes input | mitigate | Use `String::from_utf8` with `from_utf8_lossy` fallback; route invalid bytes through the existing parse_* syntax-error path (StoreError::Syntax → ResponseError). No panic on malformed input; no unsafe code; no allocation amplification (lossy decode produces output bounded by input length). |
| T-260425-ftl-02 | DoS | extract_token_str with very large bytes | accept | Token bytes flow from a Python caller already inside the same process; no network-side input. PyO3's `extract::<Vec<u8>>()` uses a single allocation bounded by Python-side memory. The downstream parser uppercases the string in a single pass before O(1) match — total cost is linear in input length. Embedded use case (single-process Prefect server) makes this acceptable. |
| T-260425-ftl-03 | Information Disclosure | from_utf8_lossy substituting U+FFFD into the ResponseError message | accept | The error message echoes the (lossy) token back to the caller. The caller already controls the input bytes, so no confidentiality boundary is crossed. Matches real Redis behavior of echoing unknown tokens in error messages. |
</threat_model>

<verification>
- `cargo check` and `cargo build --release` succeed (or `maturin develop --release` succeeds)
- `pytest tests/test_lists.py` passes end-to-end (existing + new tests)
- Spot-check from an interpreter: `python -c "import asyncio, burner_redis as br; r = br.BurnerRedis(); asyncio.run(r.linsert('k', b'BEFORE', 'a', 'b'))"` does not raise TypeError (key may not exist; return value 0 is fine)
</verification>

<success_criteria>
- All 5 call sites (linsert, lmove, blmove pymethods + execute_command linsert/lmove arms) accept bytes for option tokens with no behavior change for str callers
- `extract_token_str` is the single source of truth for str-or-bytes option-token decoding (consistent with `extract_bytes` for keys/values)
- Invalid UTF-8 bytes for tokens raise ResponseError (syntax error), NOT TypeError — proven by an explicit test
- All existing tests/test_lists.py tests still pass
- 11 new tests cover the bytes path including case-insensitivity, unknown-token, invalid-UTF-8, and pipeline dispatch
</success_criteria>

<output>
After completion, create `.planning/quick/260425-ftl-fix-p3-accept-bytes-for-list-option-toke/260425-ftl-SUMMARY.md` per the standard summary template, recording:
- The 5 sites patched (line numbers in lib.rs at the time of commit)
- The helper signature
- Whether the two `execute_command` pipeline tests passed or were wrapper-blocked (per the Task 2 Step 4 fallback)
- Confirmation that the unknown-token + invalid-UTF-8 tests both routed through ResponseError (not TypeError)
</output>
