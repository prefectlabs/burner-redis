# Phase 11: Close redis-py compatibility gaps for pydocket integration - Context

**Gathered:** 2026-04-14
**Status:** Ready for planning

<domain>
## Phase Boundary

Make burner-redis pass pydocket's full test suite with zero xfails/skips. This means: fix the XREADGROUP `>` delivery ID timing race, implement any missing Redis commands pydocket's tests reveal, and add regression tests to our own suite covering every gap fixed. Scope is strictly pydocket-driven — no speculative redis-py surface expansion.

</domain>

<decisions>
## Implementation Decisions

### Scope Definition
- **D-01:** Scope is pydocket-only — only fix what pydocket's test suite and usage patterns require
- **D-02:** Implement everything pydocket needs — no partial passes, no deferring edge cases
- **D-03:** Each new command must be full redis-py compatible (all flags, edge cases, return types), not minimal stubs

### Gap Discovery
- **D-04:** Run pydocket's own test suite against BurnerRedis as the primary source of truth for gaps
- **D-05:** Inventory all gaps from pydocket's test suite first, before fixing anything — avoids rework
- **D-06:** After inventory, fix everything including the XREADGROUP race in priority order

### XREADGROUP Delivery Race
- **D-07:** Fix the root cause at the Store level — `last_delivered_id` must advance correctly when XADD is called from Lua scripts so XREADGROUP `>` always sees new entries
- **D-08:** No Python-layer workarounds — the semantics must be correct in Rust

### Validation
- **D-09:** Phase is done when: (1) pydocket's full test suite passes against BurnerRedis with zero xfails/skips, AND (2) our own regression test suite covers every gap that was fixed
- **D-10:** Use pydocket's test suite to discover gaps, then add key scenarios to our integration tests as regression coverage

### Claude's Discretion
- Implementation order of individual missing commands (after inventory)
- How to run pydocket's test suite (vendored, subprocess, conftest fixture, etc.)
- Rust-side architecture for new commands (follow existing patterns)

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Existing Pydocket Integration
- `tests/test_pydocket_compat.py` — Current 5 integration tests (4 pass, 1 xfail on delayed task race)
- `.planning/quick/260414-9ub-update-pydocket-to-use-burner-redis-and-/260414-9ub-SUMMARY.md` — Commands added for initial pydocket compat
- `.planning/quick/260414-ap2-implement-xpending-range/260414-ap2-SUMMARY.md` — xpending_range implementation

### Codebase Integration Points
- `src/store.rs` — Store struct with all Redis command implementations; XREADGROUP and stream consumer group logic lives here
- `src/lib.rs` — PyO3 BurnerRedis class with all async Python bindings
- `src/scripting.rs` — Lua dispatch_command; XADD from Lua is where the delivery race originates
- `python/burner_redis/__init__.py` — Monkey-patch pattern for Pipeline/Lock/PubSub/register_script
- `python/burner_redis/pipeline.py` — Pipeline command buffer; new commands need pipeline methods
- `tests/test_prefect_integration.py` — Prefect integration tests (30 passing, must not regress)

### External
- pydocket package source code and test suite — researcher must investigate what commands/behaviors pydocket tests exercise

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- Command implementation pattern: Rust method on Store → PyO3 async binding in lib.rs → Pipeline buffer method in pipeline.py
- `dispatch_command()` in scripting.rs handles Lua redis.call() routing — any new commands need dispatch entries
- `store_err_to_py()` error conversion for consistent Python exception handling
- `redis_value_to_py()` recursive type converter for complex return types

### Established Patterns
- Store methods return `Result<RedisValue, StoreError>` — new commands follow same pattern
- ValueData enum: String, Hash, Set, SortedSet, Stream — add new variants only if needed
- Pipeline methods are synchronous buffer-only, execute() is async
- Quick tasks 260414-9ub and 260414-ap2 established the pattern for adding commands across all layers

### Integration Points
- `src/commands/` — One file per data type for command implementations
- `src/store.rs` — Consumer group `last_delivered_id` tracking (the race condition lives here)
- `python/burner_redis/pipeline.py` — Every new command needs a pipeline method
- `src/scripting.rs` — Every new command needs a Lua dispatch entry

### Known Bug
- XREADGROUP `>` doesn't see entries added by Lua XADD — `last_delivered_id` not advancing when XADD is called through dispatch_command within a Lua script context. This causes the pydocket delayed task delivery race (test_docket_add_delayed_task xfail).

</code_context>

<specifics>
## Specific Ideas

- The delayed task race is the most impactful single fix — it's the only xfail remaining
- pydocket's test suite is the authoritative gap inventory — run it first, then plan fixes
- Commands already added via quick tasks (hgetall, hexists, hincrby, zcard, zscore, zcount, expire, xdel, xrange, xpending_range, register_script) should not need re-implementation unless pydocket tests reveal behavioral differences

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 11-close-redis-py-compatibility-gaps-for-pydocket-integration*
*Context gathered: 2026-04-14*
