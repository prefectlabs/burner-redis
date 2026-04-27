# Phase 10: Add PUB/SUB Support — Context

**Gathered:** 2026-04-13
**Status:** Ready for planning

<domain>
## Phase Boundary

Implement Redis Pub/Sub: SUBSCRIBE, UNSUBSCRIBE, PUBLISH, PSUBSCRIBE, PUNSUBSCRIBE, and PUBSUB introspection subcommands (CHANNELS, NUMSUB, NUMPAT). Includes a Python PubSub class with message dispatch via listen() and get_message(), handler callbacks, and run_in_thread() support. Messages are fire-and-forget (not persisted).

</domain>

<decisions>
## Implementation Decisions

### Motivation & Scope
- **D-01:** Driven by compatibility with the `pydocket` package which Prefect uses — not a direct Prefect Redis subsystem need
- **D-02:** All pub/sub commands in scope: SUBSCRIBE, UNSUBSCRIBE, PUBLISH, PSUBSCRIBE, PUNSUBSCRIBE, PUBSUB CHANNELS, PUBSUB NUMSUB, PUBSUB NUMPAT
- **D-03:** Researcher should investigate pydocket's specific pub/sub usage patterns to ensure compatibility

### Python API Design
- **D-04:** Mirror redis-py PubSub interface — `client.pubsub()` returns a PubSub object, monkey-patched onto BurnerRedis (consistent with Pipeline/Lock pattern)
- **D-05:** Support handler callbacks — `subscribe('channel', handler=my_func)` for per-channel message routing
- **D-06:** PubSub class implemented in pure Python, calling into Rust Store methods for subscribe/publish operations (consistent with Pipeline/Lock pattern)
- **D-07:** Support `run_in_thread()` for background message processing in a daemon thread
- **D-08:** Support `ignore_subscribe_messages` option to filter subscription confirmation messages from listen() output

### Message Delivery
- **D-09:** Support both `listen()` (async generator) and `get_message()` (polling) for message consumption — full redis-py compatibility
- **D-10:** Internal fan-out mechanism (Rust side) is Claude's Discretion — choose best fit from Tokio ecosystem (broadcast channel, per-subscriber queue, etc.)

### Cross-Cutting
- **D-11:** `redis.call('PUBLISH', ...)` must work from Lua scripts — enables atomic publish-after-write patterns
- **D-12:** PUBLISH works inside pipelines — queued and executed with the batch
- **D-13:** Fire-and-forget semantics — no persistence for pub/sub. Subscriptions and messages lost on restart. Matches Redis behavior exactly.

### Claude's Discretion
- Internal Rust fan-out mechanism for message dispatch (D-10)

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Existing Codebase Patterns
- `python/burner_redis/__init__.py` — Monkey-patch pattern for Pipeline/Lock; PubSub should follow the same approach
- `python/burner_redis/pipeline.py` — Reference for pure-Python wrapper class pattern
- `python/burner_redis/lock.py` — Reference for pure-Python wrapper class pattern
- `src/store.rs` — Store struct with Arc<Store> + RwLock pattern; pub/sub state will need to integrate here
- `src/commands/` — Command module organization (one file per data type)
- `src/scripting.rs` — Lua dispatch_command for adding PUBLISH support in redis.call()
- `src/lib.rs` — PyO3 BurnerRedis class with #[pymethods] for Rust-side command bindings

### External
- Researcher should investigate `pydocket` package pub/sub usage patterns

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- Monkey-patch pattern (`__init__.py`) for attaching PubSub factory method to BurnerRedis
- Pure-Python class pattern (Pipeline, Lock) for PubSub implementation
- `future_into_py()` bridge for async Rust methods exposed to Python
- `dispatch_command()` in scripting.rs for Lua redis.call() integration

### Established Patterns
- Command modules in `src/commands/` — one file per data type (strings.rs, hashes.rs, etc.)
- Store methods return Result types, converted to Python exceptions via `store_err_to_py()`
- ValueData enum for typed storage — pub/sub doesn't need a new variant (no stored data)
- Pipeline.execute() calls individual BurnerRedis methods — PUBLISH needs to be a BurnerRedis method

### Integration Points
- `BurnerRedis.__init__.py` — monkey-patch `pubsub()` method
- `src/store.rs` — add subscription registry and publish fan-out logic
- `src/commands/mod.rs` — add new pubsub command module
- `src/scripting.rs` — extend dispatch_command to handle PUBLISH
- `python/burner_redis/pipeline.py` — add publish() to pipeline command buffer

</code_context>

<specifics>
## Specific Ideas

- pydocket package compatibility is the driving use case — researcher should check what specific pub/sub patterns pydocket uses
- Full redis-py PubSub API surface: subscribe(), unsubscribe(), psubscribe(), punsubscribe(), listen(), get_message(), run_in_thread(), close(), ignore_subscribe_messages
- Message format should match redis-py: dict with 'type', 'pattern', 'channel', 'data' keys

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 10-add-pub-sub-support-subscribe-unsubscribe-publish-psubscribe*
*Context gathered: 2026-04-13*
