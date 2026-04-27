# Phase 12: Close remaining redis-py compatibility gaps - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-04-14
**Phase:** 12-close-remaining-redis-py-compatibility-gaps-for-drop-in-repl
**Areas discussed:** Value coercion strategy, Key enumeration scope, Exception hierarchy approach, Command completeness priority

---

## Value Coercion Strategy

### Where should coercion happen?

| Option | Description | Selected |
|--------|-------------|----------|
| Python layer (Recommended) | Coerce in __init__.py or a helper before calling Rust. Matches redis-py behavior exactly. Easy to test and extend. Keeps Rust layer strict. | ✓ |
| Rust layer | Expand extract_bytes() in Rust to handle PyInt, PyFloat, etc. More performant but harder to match redis-py's exact coercion rules. | |
| You decide | Let Claude pick based on codebase patterns. | |

**User's choice:** Python layer
**Notes:** Keeps Rust strict, coercion logic is Python-visible and testable.

### Which value types to coerce?

| Option | Description | Selected |
|--------|-------------|----------|
| Match redis-py exactly | Accept int, float, bool, memoryview, and fall back to str(value) for anything else. Full compatibility. | ✓ |
| Common types only | Accept int, float, bool. Raise TypeError for memoryview and exotic types. Covers 99% of usage. | |
| You decide | Let Claude determine scope based on actual usage. | |

**User's choice:** Match redis-py exactly
**Notes:** Full compatibility is the goal.

---

## Key Enumeration Scope

### Implementation layer?

| Option | Description | Selected |
|--------|-------------|----------|
| Rust store method (Recommended) | Add keys_matching(pattern) to Store. Expose via PyO3. scan_iter wraps as async iterator in Python. Best performance. | ✓ |
| Python wrapper over raw key list | Expose raw keys() from Rust, glob filter in Python with fnmatch. Simpler Rust code. | |
| You decide | Let Claude pick based on expected key volume. | |

**User's choice:** Rust store method
**Notes:** Pattern matching stays close to data for performance.

### Glob syntax completeness?

| Option | Description | Selected |
|--------|-------------|----------|
| Full Redis glob syntax | Support *, ?, [ae], [^e], [a-z], and backslash escaping. Matches Redis exactly. | ✓ |
| Star-only (Recommended) | Support * wildcard only. Covers 95%+ of real usage. Simpler. | |
| You decide | Let Claude determine based on actual usage. | |

**User's choice:** Full Redis glob syntax
**Notes:** User wants full compatibility over simplicity.

---

## Exception Hierarchy Approach

### LockError pattern?

| Option | Description | Selected |
|--------|-------------|----------|
| Same pattern as ResponseError (Recommended) | Conditional import, subclass if available. Consistent with existing pattern. | ✓ |
| Always subclass redis.exceptions.LockError | Make redis a required dependency for lock functionality. | |
| You decide | Let Claude pick the best fit. | |

**User's choice:** Same pattern as ResponseError
**Notes:** Maintains consistency with existing __init__.py pattern.

### Other exception types?

| Option | Description | Selected |
|--------|-------------|----------|
| Just LockError | Only what's in the gaps doc. | |
| Audit all redis.exceptions | Check all types and align any that burner-redis could raise. | ✓ |
| You decide | Let Claude check what docket/Prefect catches. | |

**User's choice:** Audit all redis.exceptions
**Notes:** Comprehensive alignment for true drop-in compatibility.

---

## Command Completeness Priority

### Scope?

| Option | Description | Selected |
|--------|-------------|----------|
| Everything (Recommended) | All 8 items. Small commands, closes gap completely. | ✓ |
| Must + should only | Defer setex and mget. They have workarounds. | |
| Must-have only | Only set coercion and LockError. Minimal change. | |

**User's choice:** Everything
**Notes:** All items are relatively small and doing them together closes the compatibility gap completely.

---

## Claude's Discretion

- Glob pattern matching implementation choice in Rust
- Whether mget is Rust batch or Python wrapper
- Internal organization of new store methods

## Deferred Ideas

None — discussion stayed within phase scope
