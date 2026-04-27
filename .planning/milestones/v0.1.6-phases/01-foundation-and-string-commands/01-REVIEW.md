---
phase: 01-foundation-and-string-commands
reviewed: 2026-04-10T00:00:00Z
depth: standard
files_reviewed: 9
files_reviewed_list:
  - Cargo.toml
  - pyproject.toml
  - python/burner_redis/__init__.py
  - src/commands/mod.rs
  - src/commands/strings.rs
  - src/lib.rs
  - src/store.rs
  - tests/conftest.py
  - tests/test_strings.py
findings:
  critical: 0
  warning: 3
  info: 2
  total: 5
status: issues_found
---

# Phase 01: Code Review Report

**Reviewed:** 2026-04-10T00:00:00Z
**Depth:** standard
**Files Reviewed:** 9
**Status:** issues_found

## Summary

The foundation layer is well-structured. The `Store` type is sound: the two-phase read-then-write expiry eviction in `get` is correct (re-checks expiry under the write lock to avoid a TOCTOU race). The `extract_bytes` helper correctly mirrors `redis-py` encoding behavior. The PyO3 async bridge is wired up properly, and the test suite has good coverage of the happy path and NX/XX flags.

Three warnings were found that create divergence from `redis.asyncio.Redis` behavior — the primary compatibility requirement. Two info items cover minor quality issues.

## Warnings

### WR-01: Negative integer expiry silently produces a misleading error

**File:** `src/commands/strings.rs:26`
**Issue:** `extract_expiry` attempts to extract an integer expiry as `u64`. A negative value (e.g., `ex=-1`) will fail the `u64` extraction silently, fall through to the `timedelta` branch, and produce a `PyTypeError: expected int or timedelta for expiration`. Real `redis.asyncio.Redis` raises a `DataError: Invalid expire time in 'set' command` when a negative expiry is given. The diverging error type and message will break callers that catch `redis.exceptions.DataError`.

Additionally, the `u64` extraction means there is no way to produce a descriptive "expiry must be positive" error for negative integers — they are misclassified as the wrong type entirely.

**Fix:**
```rust
// In extract_expiry, extract as i64 first, validate sign, then cast.
pub fn extract_expiry(obj: &Bound<'_, PyAny>, unit_millis: bool) -> PyResult<Duration> {
    if let Ok(val) = obj.extract::<i64>() {
        if val <= 0 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "expiry must be a positive integer",
            ));
        }
        return Ok(if unit_millis {
            Duration::from_millis(val as u64)
        } else {
            Duration::from_secs(val as u64)
        });
    }
    // timedelta branch unchanged ...
}
```

---

### WR-02: Zero expiry is silently accepted, diverging from Redis behavior

**File:** `src/commands/strings.rs:26-31` / `src/store.rs:14`
**Issue:** `extract_expiry` accepts `0` as a valid `u64` and produces a `Duration::ZERO`. `ValueEntry::new` then sets `expires_at = Instant::now() + Duration::ZERO`, which means the key is immediately expired. Real Redis rejects `SET key value EX 0` with `ERR invalid expire time in 'set' command`. Accepting zero silently creates a key that is immediately inaccessible, masking caller bugs and making `SET` appear to succeed when Redis would have raised an error.

**Fix:** Validate that the extracted integer is strictly positive (handled by the fix in WR-01 using `val <= 0` check). If fixing WR-01 first, this issue is resolved as a side effect.

---

### WR-03: Both `nx=True` and `xx=True` simultaneously always returns `None` without a clear error

**File:** `src/store.rs:76-81`
**Issue:** When both `nx` and `xx` are `True`, the `set` method returns `false` regardless of key state (if key exists, `nx` fails; if key does not exist, `xx` fails). This means the Python caller receives `None` as if it were a normal conditional-set failure, not an invalid-argument error. Real `redis.asyncio.Redis` raises `DataError: Invalid expire time ...` or in newer client versions raises `DataError: ``ex`` and ``px`` can't be specified together`. Providing invalid flag combinations should raise an exception, not silently return `None`.

**Fix:**
```rust
// In store.set(), add guard before the NX/XX checks:
if nx && xx {
    // Callers should validate this; Store treats it as a no-op but
    // lib.rs should reject it before calling store.set().
    return false;
}
```

Better, validate in `lib.rs` before dispatching to the store, matching redis-py's behavior:
```rust
// In lib.rs set() method, before computing ttl:
if nx && xx {
    return Err(pyo3::exceptions::PyValueError::new_err(
        "nx and xx options are mutually exclusive",
    ));
}
```

---

## Info

### IN-01: `Store` is missing a `Default` implementation

**File:** `src/store.rs:29-34`
**Issue:** `Store` provides `Store::new()` but does not implement `std::default::Default`. In Rust, types with a no-argument constructor conventionally implement `Default` so they can be used with `..Default::default()` struct update syntax, `unwrap_or_default()`, and similar ergonomic patterns. This will matter when `Store` gains fields with defaults as the implementation grows.

**Fix:**
```rust
impl Default for Store {
    fn default() -> Self {
        Self::new()
    }
}
```

---

### IN-02: CLAUDE.md documents current-thread Tokio runtime but code uses multi-thread

**File:** `src/lib.rs:119-125`
**Issue:** The CLAUDE.md technology stack table states "Use the current-thread runtime (not multi-threaded) since we run inside a Python process and must respect the GIL." The actual implementation uses `Builder::new_multi_thread()`. The inline comment in `lib.rs` explains the rationale for multi-thread (current-thread has no background thread to drive spawned futures). The code choice is likely correct for the current `future_into_py` usage, but CLAUDE.md is out of date. This will confuse future contributors who follow the documented guidance.

**Fix:** Update the CLAUDE.md technology stack entry to document the actual decision and rationale. No code change needed — the code is correct.

---

_Reviewed: 2026-04-10T00:00:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
