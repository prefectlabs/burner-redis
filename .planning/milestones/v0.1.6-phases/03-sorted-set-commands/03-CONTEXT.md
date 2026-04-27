# Phase 3: Sorted Set Commands - Context

**Gathered:** 2026-04-10
**Status:** Ready for planning

<domain>
## Phase Boundary

Add sorted set data type support using the dual-index pattern (BTreeMap + HashMap) from CLAUDE.md. Implement ZADD (with NX/XX/GT/LT/CH flags), ZREM, ZRANGE, ZRANGEBYSCORE, ZRANGESTORE, and ZREMRANGEBYSCORE as async Python methods matching `redis.asyncio.Redis` signatures.

</domain>

<decisions>
## Implementation Decisions

### ZADD Semantics
- Full flag support: `zadd(name, mapping, nx=False, xx=False, gt=False, lt=False, ch=False)` matching redis-py.
- Return integer count of *new* elements added (default), or count of *changed* elements if `ch=True`.
- ZREM returns integer count of members actually removed.

### Range Query Semantics
- ZRANGE returns `list[bytes]` by default, `list[tuple[bytes, float]]` when `withscores=True` — matching redis-py.
- ZRANGEBYSCORE accepts `float` and `str` for `-inf`/`+inf`, matching redis-py's `min="-inf", max="+inf"` convention.
- ZRANGESTORE returns integer count of elements stored in destination key.
- ZREMRANGEBYSCORE returns integer count of elements removed.

### Data Structure (from CLAUDE.md)
- Dual-index pattern: `BTreeMap<(OrderedFloat<f64>, Bytes), ()>` for score-ordered range queries + `HashMap<Bytes, f64>` for O(1) member-to-score lookup.
- Use `ordered-float` crate (or manual Ord implementation) for f64 ordering in BTreeMap.

### Claude's Discretion
No items deferred to Claude's discretion — all questions resolved.

</decisions>

<code_context>
## Existing Code Insights

### Reusable Assets
- `src/store.rs` — Store engine with `ValueData` enum (String/Hash/Set). Needs SortedSet variant.
- `src/commands/strings.rs` — `extract_bytes` helper for str/bytes conversion.
- `src/lib.rs` — BurnerRedis pyclass with `Arc<Store>` pattern, `store_err_to_py` for error conversion.
- `python/burner_redis/__init__.py` — ResponseError exception class already defined.
- `tests/conftest.py` — Shared BurnerRedis fixture.

### Established Patterns
- All command methods async via `future_into_py` with `Arc<Store>` clone.
- Accept both str/bytes for keys/members, auto-encode via `extract_bytes`.
- Store methods return `Result<T, StoreError>` with WRONGTYPE handling.
- One pytest file per command group.

### Integration Points
- `src/store.rs` ValueData enum needs SortedSet variant.
- `src/lib.rs` needs new `#[pymethods]` for sorted set commands.
- New `src/commands/sorted_sets.rs` module.
- New `tests/test_sorted_sets.py` file.

</code_context>

<specifics>
## Specific Ideas

No specific requirements — follow established Phase 1/2 patterns and redis-py compatibility.

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope.

</deferred>
