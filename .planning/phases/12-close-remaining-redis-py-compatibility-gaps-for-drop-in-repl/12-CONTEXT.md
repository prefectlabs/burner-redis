# Phase 12: Close remaining redis-py compatibility gaps for drop-in replacement - Context

**Gathered:** 2026-04-14
**Status:** Ready for planning

<domain>
## Phase Boundary

This phase closes all remaining redis-py compatibility gaps identified during docket integration testing. It implements missing commands (keys, scan_iter, ttl, xpending summary, setex, mget), fixes value type coercion in set() and similar commands, and aligns the exception hierarchy so code catching redis.exceptions types works correctly.

After this phase, burner-redis should be a true drop-in replacement for redis.asyncio.Redis with no wrapper shims needed.

</domain>

<decisions>
## Implementation Decisions

### Value Coercion
- **D-01:** Value coercion happens in the Python layer, not Rust. Add a coercion helper that converts values before passing to Rust's extract_bytes(). Keep the Rust layer strict (str/bytes only).
- **D-02:** Match redis-py's coercion rules exactly: accept int, float, bool, memoryview, and fall back to str(value) for anything else. Apply coercion to set() and any other command that accepts values (setex, mget values if applicable).

### Key Enumeration
- **D-03:** Implement keys(pattern) as a Rust store method that iterates HashMap keys with glob matching, exposed via PyO3. Pattern matching stays close to the data for performance.
- **D-04:** Support full Redis glob syntax: *, ?, [charset], [^charset], [a-z] ranges, and backslash escaping.
- **D-05:** scan_iter(match=pattern) wraps the Rust keys method as an async iterator in Python.

### Exception Hierarchy
- **D-06:** LockError follows the same conditional import pattern as ResponseError: try importing redis.exceptions.LockError, conditionally subclass it if available, fall back to plain Exception otherwise.
- **D-07:** Audit all redis.exceptions types and align any that burner-redis could raise. Check what docket/Prefect actually catches and ensure those exception types are subclassed correctly.

### Command Completeness
- **D-08:** Implement ALL items from the gaps document in this phase: set() coercion, LockError hierarchy, keys(pattern), scan_iter(match=), ttl(name), xpending summary form, setex(name, time, value), mget(*keys). No deferrals.
- **D-09:** Every new command needs a corresponding pipeline stub in pipeline.py.
- **D-10:** ttl(name) returns seconds until expiry, -1 for no TTL, -2 for key doesn't exist — matching Redis behavior exactly.
- **D-11:** xpending(name, groupname) summary form returns dict with pending count, min id, max id, and per-consumer counts — matching redis-py's return format.
- **D-12:** setex(name, time, value) is a shorthand for set(name, value, ex=time).
- **D-13:** mget(*keys) returns a list of values (or None for missing keys) for multiple keys at once.

### Claude's Discretion
- Implementation details for glob pattern matching in Rust (regex crate vs custom matcher)
- Whether mget is implemented in Rust as a batch operation or as a Python wrapper over individual get() calls
- Internal organization of new store methods

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Gap Specification
- `/Users/alexander/dev/chrisguidry/docket/burner-redis-gaps.md` — Complete list of remaining compatibility gaps with priority classification

### Existing Patterns
- `python/burner_redis/__init__.py` — ResponseError conditional subclassing pattern (lines 7-23), to be replicated for LockError and other exceptions
- `python/burner_redis/lock.py` — Current LockError definition (line 10-12)
- `python/burner_redis/pipeline.py` — Pipeline command stubs pattern
- `src/commands/strings.rs` — extract_bytes() function that needs value coercion upstream
- `src/store.rs` — Store struct with HashMap<Bytes, ValueEntry> keyspace and expires_at TTL infrastructure
- `src/lib.rs` — PyO3 method bindings pattern

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `extract_bytes()` in `src/commands/strings.rs` — current str/bytes extraction, coercion layer goes upstream of this
- `ResponseError` conditional subclassing in `__init__.py` — exact pattern to replicate for LockError
- TTL infrastructure in `store.rs` — `ValueEntry.expires_at: Option<Instant>` already tracks expiry, just needs a read-back method
- `xpending_range` in `store.rs` (line 2014) — existing PEL iteration logic, summary form aggregates the same data differently

### Established Patterns
- Python→Rust value flow: Python calls PyO3 method → extract_bytes() → Store method → return value
- Exception pattern: conditional import of redis.exceptions, dynamic class creation with redis base class
- Pipeline pattern: synchronous buffer methods that match BurnerRedis method signatures
- Async bridge: pyo3_async_runtimes::tokio::future_into_py for all async methods

### Integration Points
- `lib.rs` #[pymethods] block — all new commands added here
- `store.rs` Store impl — new store-level methods for keys, ttl, xpending summary, mget
- `pipeline.py` Pipeline class — stubs for all new commands
- `__init__.py` — value coercion helper, exception hierarchy updates

</code_context>

<specifics>
## Specific Ideas

- The gaps document comes from real docket integration testing — every item is a real failure, not theoretical
- docket calls `redis.set(key, 1, nx=True, px=...)` — integer values in set() is the most critical coercion case
- docket catches `redis.exceptions.LockError` in worker code — the exception hierarchy fix unblocks lock usage
- keys() and scan_iter() are used in test verification code — may be less critical for production but needed for test compatibility

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 12-close-remaining-redis-py-compatibility-gaps-for-drop-in-repl*
*Context gathered: 2026-04-14*
