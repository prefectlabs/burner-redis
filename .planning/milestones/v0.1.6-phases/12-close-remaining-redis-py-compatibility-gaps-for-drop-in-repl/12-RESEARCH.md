# Phase 12: Close remaining redis-py compatibility gaps - Research

**Researched:** 2026-04-14
**Domain:** redis-py API compatibility, Python value coercion, Rust glob matching, exception hierarchy
**Confidence:** HIGH

## Summary

This phase closes the final redis-py compatibility gaps identified during real docket integration testing. The gaps fall into four categories: (1) value type coercion in set() and related commands, (2) missing key enumeration commands (keys, scan_iter), (3) exception hierarchy alignment (LockError), and (4) missing convenience commands (ttl, xpending summary, setex, mget).

All gaps are well-understood from the source gap document and from verified redis-py source code. The codebase already has all the infrastructure needed: TTL tracking via `expires_at: Option<Instant>` on ValueEntry, a reusable `glob_match()` function in `src/commands/pubsub.rs` (needs range support added), the conditional subclassing pattern for exceptions, and the established PyO3 async binding pattern. No new Rust crates are needed.

**Primary recommendation:** Implement in two plans -- Plan 1 handles Python-layer changes (value coercion, exception hierarchy, scan_iter, setex, mget wrappers) and Plan 2 handles Rust-layer commands (keys with glob, ttl, xpending summary) plus pipeline stubs.

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions
- **D-01:** Value coercion happens in the Python layer, not Rust. Add a coercion helper that converts values before passing to Rust's extract_bytes(). Keep the Rust layer strict (str/bytes only).
- **D-02:** Match redis-py's coercion rules exactly: accept int, float, bool, memoryview, and fall back to str(value) for anything else. Apply coercion to set() and any other command that accepts values (setex, mget values if applicable).
- **D-03:** Implement keys(pattern) as a Rust store method that iterates HashMap keys with glob matching, exposed via PyO3. Pattern matching stays close to the data for performance.
- **D-04:** Support full Redis glob syntax: *, ?, [charset], [^charset], [a-z] ranges, and backslash escaping.
- **D-05:** scan_iter(match=pattern) wraps the Rust keys method as an async iterator in Python.
- **D-06:** LockError follows the same conditional import pattern as ResponseError: try importing redis.exceptions.LockError, conditionally subclass it if available, fall back to plain Exception otherwise.
- **D-07:** Audit all redis.exceptions types and align any that burner-redis could raise. Check what docket/Prefect actually catches and ensure those exception types are subclassed correctly.
- **D-08:** Implement ALL items from the gaps document in this phase: set() coercion, LockError hierarchy, keys(pattern), scan_iter(match=), ttl(name), xpending summary form, setex(name, time, value), mget(*keys). No deferrals.
- **D-09:** Every new command needs a corresponding pipeline stub in pipeline.py.
- **D-10:** ttl(name) returns seconds until expiry, -1 for no TTL, -2 for key doesn't exist -- matching Redis behavior exactly.
- **D-11:** xpending(name, groupname) summary form returns dict with pending count, min id, max id, and per-consumer counts -- matching redis-py's return format.
- **D-12:** setex(name, time, value) is a shorthand for set(name, value, ex=time).
- **D-13:** mget(*keys) returns a list of values (or None for missing keys) for multiple keys at once.

### Claude's Discretion
- Implementation details for glob pattern matching in Rust (regex crate vs custom matcher)
- Whether mget is implemented in Rust as a batch operation or as a Python wrapper over individual get() calls
- Internal organization of new store methods

### Deferred Ideas (OUT OF SCOPE)
None -- discussion stayed within phase scope

</user_constraints>

## Standard Stack

### Core (already in project)
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| PyO3 | 0.28.3 | Rust-Python bindings | Already used, all new commands follow same pattern |
| pyo3-async-runtimes | 0.28.0 | Async bridge | Already used for all async methods |
| parking_lot | 0.12.5 | RwLock for store | Already used, keys/ttl/mget need read locks |
| bytes | 1.11 | Byte buffer type | Already used for all key/value handling |

### No New Dependencies Required
No new Rust crates are needed. The glob matching already exists in `src/commands/pubsub.rs` and just needs range support added. The Python-layer changes use only stdlib.

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Enhancing existing glob_match | `glob` or `fnmatch-regex` crate | Adding a crate for a ~15-line enhancement is overkill; existing function already handles *, ?, [charset], [^charset], escaping. Only [a-z] ranges missing. |
| Rust-side mget batch | Python wrapper over get() | Rust batch is one lock acquisition vs N; recommend Rust for correctness and performance. |

[VERIFIED: Cargo.toml in project] -- no new crates needed.

## Architecture Patterns

### Value Coercion Pattern (Python Layer)

**What:** A `_coerce_value()` helper in `__init__.py` that converts Python values to str/bytes before they reach Rust's `extract_bytes()`.

**When to use:** Every command that accepts user values (set, setex, hset values, sadd values, etc.). The critical case from docket is `redis.set(key, 1, nx=True, px=...)`.

**redis-py's exact coercion rules (VERIFIED from source):** [VERIFIED: redis-py `Encoder.encode()` at `redis/_parsers/encoders.py`]
```python
# redis-py Encoder.encode() coercion order:
# 1. bytes, memoryview -> pass through
# 2. bool -> DataError (REJECTED, not coerced)
# 3. int, float -> repr(value).encode()
# 4. str -> value.encode(encoding)
# 5. anything else -> DataError
```

**IMPORTANT DISCREPANCY:** D-02 says "accept int, float, bool, memoryview" but redis-py **rejects booleans** with a DataError. The `Encoder.encode()` method explicitly checks for `bool` before `int` (since `bool` is a subclass of `int`) and raises `DataError("Invalid input of type: 'bool'...")`. [VERIFIED: redis-py source code `redis/_parsers/encoders.py`]

**Recommendation:** Follow redis-py's actual behavior and reject booleans. Since D-02 specifically says "Match redis-py's coercion rules exactly," the word "exactly" takes precedence over the list that includes bool. Flag this for the planner to note, but implementing redis-py's actual behavior IS matching the rules exactly.

**Recommended implementation:**
```python
def _coerce_value(value):
    """Coerce a value to str or bytes, matching redis-py's Encoder.encode() behavior."""
    if isinstance(value, (bytes, memoryview)):
        return value
    if isinstance(value, bool):
        # redis-py rejects bools: bool is subclass of int, must check first
        raise TypeError(
            "Invalid input of type: 'bool'. Convert to a bytes, string, int or float first."
        )
    if isinstance(value, (int, float)):
        return repr(value).encode()
    if isinstance(value, str):
        return value
    # Fallback: str() coercion for unknown types (more permissive than redis-py)
    return str(value)
```

**Application points:** Wrap value arguments in set(), setex(), and any other value-accepting commands. Keys don't need coercion (they're already str/bytes in all usage patterns).

### Exception Hierarchy Pattern

**What:** Conditional subclassing so burner-redis exceptions inherit from redis.exceptions when redis-py is installed.

**Existing pattern (VERIFIED from `python/burner_redis/__init__.py` lines 7-23):**
```python
class ResponseError(Exception):
    pass

try:
    import redis.exceptions
    class ResponseError(redis.exceptions.ResponseError):  # type: ignore[no-redef]
        pass
except (ImportError, AttributeError):
    pass
```

**Apply same pattern to LockError.** Current LockError in `lock.py` (line 10-12) is:
```python
class LockError(Exception):
    pass
```

Needs to become:
```python
class LockError(Exception):
    pass

try:
    import redis.exceptions
    class LockError(redis.exceptions.LockError):  # type: ignore[no-redef]
        pass
except (ImportError, AttributeError):
    pass
```

**D-07 audit -- redis.exceptions types that burner-redis could raise:** [ASSUMED]
- `ResponseError` -- already handled
- `LockError` -- needs this phase
- `DataError` -- could be raised by value coercion, but redis-py raises it from Encoder, not from commands. Since our coercion is Python-layer, we can raise TypeError (matching current pattern) or DataError if redis-py is available.
- `ConnectionError` -- not applicable (no network)
- `TimeoutError` -- not applicable (no network)
- `BusyLoadingError` -- not applicable
- `NoScriptError` -- could arise from EVALSHA with unknown SHA. Currently raises generic exception.

**Recommendation for D-07:** Focus on LockError (the known gap). Optionally align NoScriptError if time permits, but docket doesn't catch it directly.

### Rust Store Method Pattern for keys()

**What:** New `pub fn keys(&self, pattern: &[u8]) -> Vec<Bytes>` on Store that acquires a read lock and filters HashMap keys through glob_match. [VERIFIED: existing Store methods follow read-lock-then-iterate pattern]

**Glob matching enhancement needed:** The existing `glob_match()` in `src/commands/pubsub.rs` supports `*`, `?`, `[charset]`, `[^charset]`, and `\` escaping, but does NOT support `[a-z]` character ranges. D-04 requires range support. [VERIFIED: source code inspection of pubsub.rs lines 40-70]

**Range support addition (~15 lines):** Inside the `[...]` parsing loop, check for `pattern[pi+1] == b'-'` to detect range syntax, then compare `string[si] >= pattern[pi] && string[si] <= pattern[pi+2]`. This matches Redis's own implementation. [CITED: https://redis.io/docs/latest/commands/keys/]

### TTL Command Pattern

**What:** New `pub fn ttl(&self, key: &Bytes) -> i64` on Store.

**Return values (matching Redis exactly):** [CITED: https://redis.io/docs/latest/commands/ttl/]
- `-2` if key does not exist (or is expired)
- `-1` if key exists but has no TTL set
- Positive integer: remaining seconds until expiry

**Implementation:** Read lock on data, check key existence and expiration. If exists and has `expires_at`, compute remaining duration as `(expires_at - Instant::now()).as_secs()`. Need to handle the case where TTL is sub-second (round up or truncate -- Redis truncates). [ASSUMED: Redis truncates fractional seconds]

**Infrastructure already exists:** `ValueEntry.expires_at: Option<Instant>` tracks TTL. Just need to read it back. [VERIFIED: store.rs lines 121-124]

### xpending Summary Pattern

**What:** New `pub fn xpending_summary(&self, key: &Bytes, group: &Bytes) -> Result<...>` on Store.

**redis-py return format (VERIFIED from `redis/_parsers/helpers.py` `parse_xpending` function):**
```python
{
    "pending": int,        # total pending count across all consumers
    "min": str_or_None,    # smallest pending message ID (None if 0 pending)
    "max": str_or_None,    # greatest pending message ID (None if 0 pending)
    "consumers": [         # list of consumer dicts
        {"name": bytes, "pending": int},
        ...
    ]
}
```

**Implementation:** Iterate all consumers in the ConsumerGroup, aggregate their PEL sizes, track min/max IDs. Existing `xpending_range` in store.rs (line 2014) already iterates PEL entries -- summary form is simpler (just counts, not individual entries). [VERIFIED: store.rs xpending_range implementation]

### scan_iter Pattern (Python Async Generator)

**What:** Async generator in Python that wraps the Rust keys() method.

**redis-py signature:** `scan_iter(match=None, count=None, _type=None)` [ASSUMED]

**Implementation:** Since burner-redis is in-process with no cursor semantics needed, scan_iter can simply call keys(match_pattern) and yield each key:
```python
async def scan_iter(self, match=None, count=None, _type=None):
    pattern = match if match is not None else "*"
    keys = await self.keys(pattern)
    for key in keys:
        yield key
```

The `count` parameter is a hint in Redis (not a hard limit) and can be safely ignored for an in-process implementation. The `_type` parameter filters by value type and is rarely used -- can be ignored initially. [ASSUMED: docket doesn't use _type parameter]

### setex and mget Patterns

**setex(name, time, value):** Pure Python wrapper. Just calls `self.set(name, value, ex=time)`. No Rust changes needed. D-12 confirms this. [VERIFIED: D-12 in CONTEXT.md]

**mget(*keys):** Two options per Claude's discretion:
1. **Python wrapper:** `[await self.get(k) for k in keys]` -- simple, N lock acquisitions
2. **Rust batch:** Single read lock, iterate keys, return results list -- one lock acquisition

**Recommendation: Rust batch.** A single read lock is correct for atomicity (mget should return a consistent snapshot). With N individual gets, interleaving writes between gets could return inconsistent results. Implement as `pub fn mget(&self, keys: &[Bytes]) -> Vec<Option<Bytes>>` in Store. [ASSUMED: atomicity matters for mget correctness]

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Glob pattern matching | New glob implementation | Enhance existing `glob_match()` in pubsub.rs with range support | Already 90% complete, well-tested, matches Redis semantics |
| Value type coercion | Complex type dispatch | Simple isinstance chain matching redis-py's Encoder.encode() | redis-py's coercion is <15 lines; match it exactly |
| Async iteration | Custom async iterator class | Python async generator function (yield) | `async def` + `yield` is the idiomatic pattern |
| Exception subclassing | metaclass magic | Conditional class redefinition (existing pattern) | Already proven in ResponseError; copy-paste pattern |

**Key insight:** Every gap in this phase has an existing pattern in the codebase to follow. No novel architecture needed.

## Common Pitfalls

### Pitfall 1: Bool Coercion Mismatch
**What goes wrong:** Accepting booleans in set() when redis-py rejects them, causing behavior divergence.
**Why it happens:** `bool` is a subclass of `int` in Python, so `isinstance(True, int)` returns True. Easy to accidentally coerce bools as ints.
**How to avoid:** Check `isinstance(value, bool)` BEFORE `isinstance(value, int)`, exactly as redis-py does.
**Warning signs:** Tests pass with `set(key, True)` but fail when docket code expects DataError/TypeError on bool input.

### Pitfall 2: TTL Rounding
**What goes wrong:** TTL returns 0 when there's sub-second remaining time, or returns negative values.
**Why it happens:** `Instant::now()` might be past `expires_at` by the time we compute duration, or fractional seconds are handled wrong.
**How to avoid:** Use `expires_at.checked_duration_since(Instant::now())` to safely handle the past case. If None (expired), return -2 after removing the key. If Some(duration), return `duration.as_secs() as i64` (truncate, matching Redis).
**Warning signs:** Flaky TTL tests where timing causes off-by-one or negative results.

### Pitfall 3: glob_match Range Edge Cases
**What goes wrong:** `[z-a]` (reversed range), `[-abc]` (leading hyphen), `[abc-]` (trailing hyphen) handled incorrectly.
**Why it happens:** Range parsing assumes first char < second char, or hyphen is always a range operator.
**How to avoid:** Redis treats `[z-a]` as no match (empty range). Leading/trailing hyphens are literal characters, not range operators. Test these edge cases explicitly.
**Warning signs:** Pattern `[a-z]` works but `[-a]` or `[a-]` crashes or gives wrong results.

### Pitfall 4: xpending Summary with No Pending
**What goes wrong:** Returning None or empty dict when there are 0 pending messages.
**Why it happens:** Edge case where consumer group exists but all messages are acknowledged.
**How to avoid:** Return `{"pending": 0, "min": None, "max": None, "consumers": []}` for empty PEL. [ASSUMED: redis-py returns None for min/max when pending is 0]
**Warning signs:** docket code crashes on `result["consumers"]` when result is None.

### Pitfall 5: scan_iter Not Being an Async Generator
**What goes wrong:** Returning a list instead of an async iterable, breaking `async for key in r.scan_iter()`.
**Why it happens:** Using `return` instead of `yield` in the implementation.
**How to avoid:** Use `async def scan_iter(...)` with `yield` -- this creates an async generator that supports `async for`.
**Warning signs:** `async for` raises TypeError about the return value not being an async iterator.

### Pitfall 6: Pipeline Stub Mismatches
**What goes wrong:** New commands work on BurnerRedis but fail in pipeline context.
**Why it happens:** Forgetting to add pipeline stubs for new commands, or getting the signature wrong.
**How to avoid:** D-09 requires stubs for ALL new commands. Each stub must match the BurnerRedis method signature exactly.
**Warning signs:** Pipeline tests pass for existing commands but fail for new ones.

## Code Examples

### Value Coercion Helper
```python
# Source: Modeled on redis-py's Encoder.encode() [VERIFIED: redis/_parsers/encoders.py]
def _coerce_value(value):
    """Coerce a value to str or bytes for Rust extract_bytes() compatibility."""
    if isinstance(value, (bytes, memoryview)):
        return value
    if isinstance(value, bool):
        raise TypeError(
            "Invalid input of type: 'bool'. "
            "Convert to a bytes, string, int or float first."
        )
    if isinstance(value, (int, float)):
        return repr(value).encode()
    if isinstance(value, str):
        return value
    return str(value)
```

### LockError Conditional Subclassing
```python
# Source: Existing ResponseError pattern in __init__.py [VERIFIED: codebase]
class LockError(Exception):
    """Raised when a lock operation fails."""
    pass

try:
    import redis.exceptions
    class LockError(redis.exceptions.LockError):  # type: ignore[no-redef]
        """Raised when a lock operation fails (subclass of redis.exceptions.LockError)."""
        pass
except (ImportError, AttributeError):
    pass
```

### Rust keys() Store Method
```rust
// Source: Pattern from existing store.rs methods [VERIFIED: codebase]
pub fn keys(&self, pattern: &[u8]) -> Vec<Bytes> {
    let data = self.data.read();
    let mut result = Vec::new();
    for (key, entry) in data.iter() {
        if !entry.is_expired() && glob_match(pattern, key.as_ref()) {
            result.push(key.clone());
        }
    }
    result
}
```

### Rust ttl() Store Method
```rust
// Source: Redis TTL semantics [CITED: https://redis.io/docs/latest/commands/ttl/]
pub fn ttl(&self, key: &Bytes) -> i64 {
    let mut data = self.data.write(); // write for passive expiration cleanup
    match data.get(key) {
        None => -2,
        Some(entry) if entry.is_expired() => {
            data.remove(key);
            -2
        }
        Some(entry) => match entry.expires_at {
            None => -1,
            Some(exp) => {
                match exp.checked_duration_since(Instant::now()) {
                    Some(remaining) => remaining.as_secs() as i64,
                    None => -2, // already expired between check and computation
                }
            }
        },
    }
}
```

### Rust mget() Store Method
```rust
// Source: Pattern from existing store.rs get() [VERIFIED: codebase]
pub fn mget(&self, keys: &[Bytes]) -> Vec<Option<Bytes>> {
    let data = self.data.read();
    keys.iter().map(|key| {
        match data.get(key) {
            Some(entry) if !entry.is_expired() => {
                if let ValueData::String(ref v) = entry.data {
                    Some(v.clone())
                } else {
                    None // non-string type returns None (matches GET behavior)
                }
            }
            _ => None,
        }
    }).collect()
}
```

### Glob Match with Range Support
```rust
// Source: Enhancement to existing glob_match in pubsub.rs [VERIFIED: codebase]
// Inside the [...] parsing loop, replace simple character check with:
while pi < pattern.len() && pattern[pi] != b']' {
    // Check for range: a-z
    if pi + 2 < pattern.len() && pattern[pi + 1] == b'-' && pattern[pi + 2] != b']' {
        let range_start = pattern[pi];
        let range_end = pattern[pi + 2];
        if string[si] >= range_start && string[si] <= range_end {
            found = true;
        }
        pi += 3; // skip 'a', '-', 'z'
    } else {
        if string[si] == pattern[pi] {
            found = true;
        }
        pi += 1;
    }
}
```

### Python scan_iter Async Generator
```python
# Source: redis-py scan_iter pattern [ASSUMED]
async def scan_iter(self, match=None, count=None, _type=None):
    """Async iterator over keys matching a pattern."""
    pattern = match if match is not None else "*"
    keys = await self.keys(pattern)
    for key in keys:
        yield key
```

### Pipeline Stubs
```python
# Source: Existing pipeline.py pattern [VERIFIED: codebase]
def keys(self, pattern="*"):
    self._commands.append(("keys", (pattern,), {}))
    return self

def ttl(self, name):
    self._commands.append(("ttl", (name,), {}))
    return self

def setex(self, name, time, value):
    self._commands.append(("setex", (name, time, value), {}))
    return self

def mget(self, *keys):
    self._commands.append(("mget", keys, {}))
    return self

def xpending(self, name, groupname):
    self._commands.append(("xpending", (name, groupname), {}))
    return self
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| redis-py accepted bools (v2.x) | redis-py rejects bools with DataError (v3.0+) | redis-py 3.0 (2018) | Must reject bools, not coerce them |
| xpending was one method | Split into xpending (summary) and xpending_range (detail) | redis-py 4.x | Need both methods with different return formats |

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | Redis truncates (not rounds) fractional seconds for TTL | TTL Command Pattern | Could return off-by-one TTL values |
| A2 | redis-py returns None for min/max in xpending when pending=0 | Pitfall 4 | xpending return format mismatch |
| A3 | redis-py scan_iter signature is `(match=None, count=None, _type=None)` | scan_iter Pattern | Wrong signature could break callers |
| A4 | docket doesn't use the `_type` parameter of scan_iter | scan_iter Pattern | Missing filter could cause test failures |
| A5 | Atomicity matters for mget (single lock acquisition) | mget Pattern | Over-engineering if atomicity not needed |
| A6 | NoScriptError alignment is not needed for docket | Exception audit | Missed exception catch in docket |

## Open Questions

1. **Bool coercion vs D-02**
   - What we know: D-02 says "accept bool" but redis-py rejects booleans with DataError
   - What's unclear: Whether the user intentionally wants more permissive behavior than redis-py, or whether the "exactly" qualifier means follow redis-py's actual behavior
   - Recommendation: Follow redis-py's actual behavior (reject bools). The word "exactly" in D-02 suggests matching redis-py, and the gap doc says `set()` rejects non-string values -- the fix is to accept int/float (which redis-py does), not to add bool support (which redis-py doesn't)

2. **mget with expired keys -- passive cleanup or not?**
   - What we know: Individual get() does passive expiration cleanup (write lock to remove expired key)
   - What's unclear: Should mget also do passive cleanup for each expired key, or just return None?
   - Recommendation: Use read lock for mget and return None for expired keys without cleaning them up. The active sweep will handle cleanup. This avoids upgrading to a write lock inside mget.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | pytest (latest) |
| Config file | none (uses conftest.py fixture) |
| Quick run command | `uv run pytest tests/ -x --ignore=tests/test_pydocket_compat.py --ignore=tests/test_prefect_integration.py -q` |
| Full suite command | `uv run pytest tests/ -q` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| D-01/D-02 | set() accepts int, float values; rejects bool | unit | `uv run pytest tests/test_strings.py -x -q -k coercion` | No -- Wave 0 |
| D-03/D-04 | keys(pattern) with glob syntax | unit | `uv run pytest tests/test_strings.py -x -q -k keys` | No -- Wave 0 |
| D-05 | scan_iter(match=) async iteration | unit | `uv run pytest tests/test_strings.py -x -q -k scan_iter` | No -- Wave 0 |
| D-06 | LockError inherits from redis.exceptions.LockError | unit | `uv run pytest tests/test_locking.py -x -q -k LockError` | No -- Wave 0 |
| D-10 | ttl() returns correct values | unit | `uv run pytest tests/test_expiration.py -x -q -k ttl` | No -- Wave 0 |
| D-11 | xpending summary form returns dict | unit | `uv run pytest tests/test_streams.py -x -q -k xpending` | No -- Wave 0 |
| D-12 | setex(name, time, value) works | unit | `uv run pytest tests/test_strings.py -x -q -k setex` | No -- Wave 0 |
| D-13 | mget(*keys) returns list of values | unit | `uv run pytest tests/test_strings.py -x -q -k mget` | No -- Wave 0 |
| D-09 | Pipeline stubs for all new commands | unit | `uv run pytest tests/test_pipeline.py -x -q -k "keys or ttl or setex or mget or xpending"` | No -- Wave 0 |

### Sampling Rate
- **Per task commit:** `uv run pytest tests/ -x --ignore=tests/test_pydocket_compat.py --ignore=tests/test_prefect_integration.py -q`
- **Per wave merge:** `uv run pytest tests/ -q`
- **Phase gate:** Full suite green before `/gsd-verify-work`

### Wave 0 Gaps
- [ ] Tests for value coercion in `test_strings.py` -- covers D-01/D-02
- [ ] Tests for keys() glob matching in `test_strings.py` -- covers D-03/D-04
- [ ] Tests for scan_iter() in `test_strings.py` -- covers D-05
- [ ] Tests for LockError hierarchy in `test_locking.py` -- covers D-06
- [ ] Tests for ttl() in `test_expiration.py` -- covers D-10
- [ ] Tests for xpending summary in `test_streams.py` -- covers D-11
- [ ] Tests for setex() in `test_strings.py` -- covers D-12
- [ ] Tests for mget() in `test_strings.py` -- covers D-13
- [ ] Tests for pipeline stubs in `test_pipeline.py` -- covers D-09

## Sources

### Primary (HIGH confidence)
- `python/burner_redis/__init__.py` -- ResponseError conditional subclassing pattern [VERIFIED]
- `python/burner_redis/lock.py` -- Current LockError definition [VERIFIED]
- `python/burner_redis/pipeline.py` -- Pipeline command stubs pattern [VERIFIED]
- `src/commands/strings.rs` -- extract_bytes() function [VERIFIED]
- `src/commands/pubsub.rs` -- glob_match() function [VERIFIED]
- `src/store.rs` -- Store struct with HashMap keyspace and TTL infrastructure [VERIFIED]
- `src/lib.rs` -- PyO3 method bindings pattern [VERIFIED]
- `redis/_parsers/encoders.py` -- redis-py Encoder.encode() value coercion rules [VERIFIED: WebFetch]
- `redis/_parsers/helpers.py` -- redis-py parse_xpending() return format [VERIFIED: WebFetch]
- [Redis KEYS command docs](https://redis.io/docs/latest/commands/keys/) -- Glob pattern syntax
- [Redis TTL command docs](https://redis.io/docs/latest/commands/ttl/) -- Return value semantics
- [Redis XPENDING command docs](https://redis.io/docs/latest/commands/xpending/) -- Summary form format
- `/Users/alexander/dev/chrisguidry/docket/burner-redis-gaps.md` -- Complete gap specification [VERIFIED]

### Secondary (MEDIUM confidence)
- [redis.io scan_iter docs](https://redis.io/docs/latest/develop/clients/redis-py/scaniter/) -- scan_iter pattern

### Tertiary (LOW confidence)
- A3: redis-py scan_iter exact signature -- based on training knowledge, not verified from source

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- no new dependencies, all existing infrastructure
- Architecture: HIGH -- all patterns directly follow existing codebase conventions
- Pitfalls: HIGH -- well-understood from redis-py source verification
- Value coercion: HIGH -- verified from redis-py source code
- xpending format: HIGH -- verified from redis-py parse_xpending source
- glob range support: HIGH -- straightforward enhancement to existing function

**Research date:** 2026-04-14
**Valid until:** 2026-05-14 (stable -- all patterns are established)
