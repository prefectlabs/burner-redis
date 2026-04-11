# Phase 4: Key Expiration - Context

**Gathered:** 2026-04-11
**Status:** Ready for planning

<domain>
## Phase Boundary

Complete the TTL expiration system by adding an active sweep mechanism (Tokio background task) to complement the existing passive expiration on read. Ensure ALL data types respect expiration uniformly.

</domain>

<decisions>
## Implementation Decisions

### Active Sweep Strategy
- Tokio background task runs every 100ms, checks a random sample of up to 20 keys with TTLs per cycle, deletes expired ones — mimics Redis's lazy expiration approach.
- Sweep starts on first `BurnerRedis()` instantiation via Tokio spawn. Stops implicitly when the Store's Arc refcount drops to zero.
- Sweep interval is not configurable (hardcoded 100ms). Configuration can be added later if needed.

### Passive Expiration Scope
- Passive check on ALL read operations (get, hget, hvals, smembers, sismember, zrange, etc.), not just string GET.
- Any access to an expired key treats it as non-existent (key deleted on access).
- This extends Phase 1's existing passive check to cover hash, set, and sorted set operations added in Phases 2-3.

### Implementation Details
- Use `Tokio::time::interval(Duration::from_millis(100))` for the sweep timer.
- The sweep task holds a `Weak<Store>` (or `Arc<Store>`) reference — if using Weak, the task stops when all strong references are dropped.
- Random sampling: collect keys with expiry, pick up to 20 randomly, check each and remove if expired.
- Milliseconds and seconds precision both already supported in Phase 1's SET EX/PX — no new precision work needed, just ensure active sweep honors the exact timestamps.

### Claude's Discretion
No items deferred to Claude's discretion — all questions resolved.

</decisions>

<code_context>
## Existing Code Insights

### Reusable Assets
- `src/store.rs` — Already has `expires_at: Option<Instant>` in `ValueEntry`, passive expiry check in `get()`.
- `src/lib.rs` — BurnerRedis with `Arc<Store>` and Tokio runtime already initialized.
- Phase 1 SET already stores TTL as `Instant::now() + Duration`.

### Established Patterns
- `Arc<Store>` shared between Python-facing methods and background tasks.
- `parking_lot::RwLock` for thread-safe access to the keyspace.
- `future_into_py` for async methods.

### Integration Points
- `src/store.rs` needs a `sweep_expired()` method that does the random sampling and deletion.
- `src/lib.rs` BurnerRedis `__new__` needs to spawn the sweep task.
- All read methods (hget, hvals, smembers, sismember, zrange, zrangebyscore, exists) need passive expiry checks.

</code_context>

<specifics>
## Specific Ideas

No specific requirements — follow established patterns.

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope.

</deferred>
