# Phase 10: Add PUB/SUB Support - Research

**Researched:** 2026-04-13
**Domain:** Redis Pub/Sub protocol, async message fan-out, Python redis-py PubSub API
**Confidence:** HIGH

## Summary

Redis Pub/Sub is a fire-and-forget messaging system with six commands (SUBSCRIBE, UNSUBSCRIBE, PUBLISH, PSUBSCRIBE, PUNSUBSCRIBE, PUBSUB) and six message types. The architecture splits cleanly into three layers: (1) a Rust-side subscription registry with Tokio broadcast channels for fan-out, (2) Rust-side `#[pymethods]` for `publish` and `pubsub_*` introspection commands on `BurnerRedis`, and (3) a pure-Python `PubSub` class that mirrors redis-py's async PubSub interface -- consistent with the existing Pipeline/Lock monkey-patch pattern.

The driving use case is `pydocket` (the PyPI package for the `docket` library). Investigation reveals that pydocket is "purpose-built for Redis streams" and does NOT use Redis pub/sub at all. Zero references to SUBSCRIBE, PUBLISH, or PubSub exist in the docket codebase. The user's D-01 decision states pydocket compatibility drives this phase, but pydocket's actual Redis usage is streams-only. This discrepancy should be acknowledged but does not change the implementation plan -- pub/sub is a standard Redis feature worth supporting for general compatibility.

**Primary recommendation:** Implement a Tokio broadcast channel-based fan-out in `Store`, expose `publish`/`subscribe`/`unsubscribe`/`psubscribe`/`punsubscribe` as Rust methods returning subscriber counts and channel lists, then build a pure-Python `PubSub` class using `asyncio.Queue` for per-subscriber message buffering with `listen()`, `get_message()`, `run_in_thread()`, and handler callback support.

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

- **D-01:** Driven by compatibility with the `pydocket` package which Prefect uses -- not a direct Prefect Redis subsystem need
- **D-02:** All pub/sub commands in scope: SUBSCRIBE, UNSUBSCRIBE, PUBLISH, PSUBSCRIBE, PUNSUBSCRIBE, PUBSUB CHANNELS, PUBSUB NUMSUB, PUBSUB NUMPAT
- **D-03:** Researcher should investigate pydocket's specific pub/sub usage patterns to ensure compatibility
- **D-04:** Mirror redis-py PubSub interface -- `client.pubsub()` returns a PubSub object, monkey-patched onto BurnerRedis (consistent with Pipeline/Lock pattern)
- **D-05:** Support handler callbacks -- `subscribe('channel', handler=my_func)` for per-channel message routing
- **D-06:** PubSub class implemented in pure Python, calling into Rust Store methods for subscribe/publish operations (consistent with Pipeline/Lock pattern)
- **D-07:** Support `run_in_thread()` for background message processing in a daemon thread
- **D-08:** Support `ignore_subscribe_messages` option to filter subscription confirmation messages from listen() output
- **D-09:** Support both `listen()` (async generator) and `get_message()` (polling) for message consumption -- full redis-py compatibility
- **D-10:** Internal fan-out mechanism (Rust side) is Claude's Discretion -- choose best fit from Tokio ecosystem (broadcast channel, per-subscriber queue, etc.)
- **D-11:** `redis.call('PUBLISH', ...)` must work from Lua scripts -- enables atomic publish-after-write patterns
- **D-12:** PUBLISH works inside pipelines -- queued and executed with the batch
- **D-13:** Fire-and-forget semantics -- no persistence for pub/sub. Subscriptions and messages lost on restart. Matches Redis behavior exactly.

### Claude's Discretion

- Internal Rust fan-out mechanism for message dispatch (D-10)

### Deferred Ideas (OUT OF SCOPE)

None -- discussion stayed within phase scope

</user_constraints>

## Standard Stack

### Core

No new crate dependencies needed. All required functionality is available through existing dependencies.

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `tokio::sync::broadcast` | 1.51.x (already in Cargo.toml) | Fan-out message delivery to multiple subscribers | Built into Tokio `sync` feature (already enabled). Multi-producer multi-consumer channel where every receiver sees every message. Perfect match for Redis pub/sub semantics. | [VERIFIED: Cargo.toml already has `tokio = { version = "1.51", features = ["rt", "time", "sync"] }`]
| `tokio::sync::mpsc` | 1.51.x (already in Cargo.toml) | Per-subscriber message queue bridging Rust to Python | Already available via `sync` feature. Used to create per-subscriber channels that the Python PubSub class polls via `get_message()`. | [VERIFIED: included in tokio sync feature]
| `parking_lot::RwLock` | 0.12.5 (already in Cargo.toml) | Protecting subscription registry | Consistent with existing Store pattern for keyspace locking. | [VERIFIED: Cargo.toml]

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `asyncio.Queue` | stdlib | Python-side message buffering | Per-subscriber queue that PubSub.listen()/get_message() consumes from | [VERIFIED: Python stdlib]

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `tokio::sync::broadcast` for fan-out | `HashMap<channel, Vec<mpsc::Sender>>` | Broadcast is purpose-built for this. Manual Vec<Sender> requires managing subscriber lifecycle manually. Broadcast handles lagged receivers and cleanup automatically. |
| Custom glob matcher | `glob-match` crate | Redis glob patterns are simple (only `*`, `?`, `[...]`, `\` escape). ~30 lines of Rust. Adding a dependency is overkill. |
| Python `asyncio.Queue` | Tokio mpsc bridged to Python | Queue is simpler -- PubSub class is pure Python, so staying in Python-land for buffering avoids complex Rust-Python async bridging for the receive path. |

**Installation:** No new dependencies needed. All libraries already in Cargo.toml and Python stdlib.

## Architecture Patterns

### Recommended Project Structure

```
src/
  commands/
    pubsub.rs           # NEW: PUBSUB CHANNELS/NUMSUB/NUMPAT introspection
  store.rs              # MODIFIED: add PubSubRegistry, subscribe/unsubscribe/publish methods
  scripting.rs          # MODIFIED: add PUBLISH to dispatch_command
  lib.rs                # MODIFIED: add publish, pubsub_channels, pubsub_numsub, pubsub_numpat #[pymethods]
python/
  burner_redis/
    __init__.py         # MODIFIED: monkey-patch pubsub() method onto BurnerRedis
    pubsub.py           # NEW: PubSub class with listen(), get_message(), run_in_thread()
    pipeline.py         # MODIFIED: add publish() to Pipeline command buffer
tests/
  test_pubsub.py        # NEW: comprehensive pub/sub tests
```

### Pattern 1: Subscription Registry in Store

**What:** A dedicated `PubSubRegistry` struct inside `Store` that manages channel subscriptions and pattern subscriptions separately from the keyspace data. Pub/sub state is NOT stored in the `data: RwLock<HashMap<Bytes, ValueEntry>>` -- it's a separate concern.

**When to use:** Always -- pub/sub is orthogonal to key-value storage.

**Example:**
```rust
// Source: Architecture design based on Redis behavior [ASSUMED]
use tokio::sync::broadcast;

/// A subscriber handle returned to Python for message consumption.
pub struct SubscriberHandle {
    pub id: u64,  // unique subscriber ID
    pub rx: broadcast::Receiver<PubSubMessage>,
}

/// A message delivered through pub/sub.
#[derive(Clone, Debug)]
pub struct PubSubMessage {
    pub kind: String,        // "message" or "pmessage"
    pub pattern: Option<Bytes>,  // pattern that matched (for pmessage)
    pub channel: Bytes,      // channel name
    pub data: Bytes,         // message payload
}

/// Registry tracking all active subscriptions.
pub struct PubSubRegistry {
    /// Global broadcast sender -- all published messages go through here
    tx: broadcast::Sender<PubSubMessage>,
    /// Channel -> set of subscriber IDs (for NUMSUB counting)
    channel_subscribers: HashMap<Bytes, HashSet<u64>>,
    /// Pattern -> set of subscriber IDs (for NUMPAT counting)
    pattern_subscribers: HashMap<Bytes, HashSet<u64>>,
    /// Next subscriber ID
    next_id: u64,
}
```

### Pattern 2: Python PubSub Class (Pure Python, Monkey-patched)

**What:** A pure-Python class following the exact redis-py async PubSub interface. Uses `asyncio.Queue` internally for message buffering. Calls Rust Store methods for subscribe/unsubscribe/publish operations.

**When to use:** Always -- consistent with Pipeline and Lock patterns.

**Example:**
```python
# Source: redis-py PubSub API [CITED: github.com/redis/redis-py]
class PubSub:
    PUBLISH_MESSAGE_TYPES = ("message", "pmessage")
    UNSUBSCRIBE_MESSAGE_TYPES = ("unsubscribe", "punsubscribe")

    def __init__(self, client, ignore_subscribe_messages=False):
        self._client = client
        self.ignore_subscribe_messages = ignore_subscribe_messages
        self.channels = {}      # channel -> handler or None
        self.patterns = {}      # pattern -> handler or None
        self._queue = asyncio.Queue()
        self._subscriber_id = None  # assigned by Rust when first subscribe

    async def subscribe(self, *args, **kwargs):
        """Subscribe to channels. kwargs map channel names to handler callables."""
        # ... register channels, call Rust store.subscribe()
        pass

    async def get_message(self, ignore_subscribe_messages=False, timeout=0.0):
        """Get next message or None."""
        pass

    async def listen(self):
        """Async generator yielding messages."""
        while self.subscribed:
            msg = await self.get_message(timeout=None)
            if msg is not None:
                yield msg
```

### Pattern 3: Dual-path Message Delivery (Broadcast + Filter)

**What:** Use a single Tokio broadcast channel for ALL pub/sub messages. Each Python subscriber receives the broadcast and filters locally based on their subscribed channels/patterns. This avoids complex per-channel routing in Rust.

**When to use:** For simplicity in an embedded single-process database. The alternative (per-channel broadcast channels) adds complexity for subscription management with minimal benefit at the scale this project targets.

**Design rationale:**
- One `broadcast::Sender<PubSubMessage>` in the Store
- Each subscriber gets a `broadcast::Receiver` via `tx.subscribe()`
- A Tokio task per subscriber filters messages and pushes matches into that subscriber's `asyncio.Queue`
- Python PubSub polls the Queue

**Alternative considered:** Per-channel broadcast channels. This would give exact delivery (no filtering needed) but requires creating/destroying channels dynamically and managing the lifecycle. For an embedded DB with likely few subscribers, the filtering overhead is negligible.

### Pattern 4: Glob Pattern Matching (Hand-rolled)

**What:** A simple ~30 line function implementing Redis-style glob matching: `*` (any string), `?` (one char), `[abc]` (char class), `\` (escape).

**When to use:** For PSUBSCRIBE pattern matching against channel names during publish.

**Example:**
```rust
// Source: Redis stringmatchlen algorithm [CITED: github.com/redis/redis]
fn glob_match(pattern: &[u8], string: &[u8]) -> bool {
    let mut pi = 0;
    let mut si = 0;
    while pi < pattern.len() && si < string.len() {
        match pattern[pi] {
            b'*' => {
                // Skip consecutive stars
                while pi < pattern.len() && pattern[pi] == b'*' { pi += 1; }
                if pi == pattern.len() { return true; }
                // Try matching rest of pattern at each position
                while si < string.len() {
                    if glob_match(&pattern[pi..], &string[si..]) { return true; }
                    si += 1;
                }
                return false;
            }
            b'?' => { pi += 1; si += 1; }
            b'[' => {
                // Character class matching
                pi += 1;
                let negate = pi < pattern.len() && pattern[pi] == b'^';
                if negate { pi += 1; }
                let mut matched = false;
                while pi < pattern.len() && pattern[pi] != b']' {
                    if pattern[pi] == string[si] { matched = true; }
                    pi += 1;
                }
                if pi < pattern.len() { pi += 1; } // skip ']'
                if negate { matched = !matched; }
                if !matched { return false; }
                si += 1;
            }
            b'\\' => {
                pi += 1;
                if pi < pattern.len() && pattern[pi] == string[si] {
                    pi += 1; si += 1;
                } else { return false; }
            }
            c => {
                if c != string[si] { return false; }
                pi += 1; si += 1;
            }
        }
    }
    // Skip trailing stars
    while pi < pattern.len() && pattern[pi] == b'*' { pi += 1; }
    pi == pattern.len() && si == string.len()
}
```

### Anti-Patterns to Avoid

- **Storing pub/sub messages in keyspace:** Pub/sub is fire-and-forget. Never add a `ValueData::PubSub` variant. Messages are transient.
- **Persisting subscription state:** Subscriptions are per-process, per-connection. They must NOT be serialized to disk. On restart, all subscriptions are lost (matches Redis behavior exactly).
- **Blocking the keyspace RwLock during publish:** Publish should acquire the pub/sub registry lock, NOT the keyspace data lock. These are independent concerns. Exception: PUBLISH inside Lua scripts already holds the data write lock -- the pub/sub fan-out should be deferred until after the Lua script completes to avoid deadlock.
- **Using per-channel broadcast channels:** Creates lifecycle management complexity. A single global broadcast with client-side filtering is simpler and sufficient for embedded use.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Multi-consumer message fan-out | Custom Vec<Sender> with manual cleanup | `tokio::sync::broadcast` | Handles lagged receivers, automatic cleanup on drop, zero-copy cloning |
| Async Python message queue | Rust-to-Python async channel | `asyncio.Queue` in pure Python | PubSub class is pure Python; using Python's own async primitive avoids complex cross-runtime bridging |
| Thread-based message loop | Custom thread management | `threading.Thread(daemon=True)` in run_in_thread() | Standard Python pattern, matches redis-py exactly |

**Key insight:** The Rust layer handles publish fan-out and subscription bookkeeping. The Python layer handles message formatting, handler dispatch, and the redis-py-compatible API surface. Keeping the boundary clean (Rust = data, Python = API) avoids complex cross-language async coordination.

## Common Pitfalls

### Pitfall 1: Deadlock between keyspace lock and pub/sub registry lock

**What goes wrong:** PUBLISH inside a Lua script holds the keyspace write lock. If publish() tries to acquire a separate pub/sub lock, and some other path acquires pub/sub lock then keyspace lock, deadlock occurs.
**Why it happens:** Lock ordering violations when pub/sub state is protected by a separate lock from keyspace data.
**How to avoid:** For Lua dispatch_command PUBLISH: collect the message but defer actual fan-out until after the Lua script completes and the data lock is released. The Store.publish() method itself should only need the pub/sub registry lock, never the keyspace lock. For Lua: return the message to fan-out, do it after the script scope closes.
**Warning signs:** Tests hang when PUBLISH is called from Lua scripts.

### Pitfall 2: Subscription confirmation messages confusing tests

**What goes wrong:** After subscribing, redis-py sends confirmation messages (type="subscribe", data=subscription_count). Tests that call `listen()` right after `subscribe()` get these confirmations instead of published messages.
**Why it happens:** Redis protocol sends subscribe confirmations as messages in the pub/sub stream.
**How to avoid:** (1) Support `ignore_subscribe_messages=True` on PubSub constructor and get_message(). (2) In tests, either consume confirmations explicitly or use `ignore_subscribe_messages=True`.
**Warning signs:** Tests receiving `{'type': 'subscribe', ...}` when expecting `{'type': 'message', ...}`.

### Pitfall 3: Pattern subscriptions delivering duplicate messages

**What goes wrong:** If a client subscribes to both channel "foo" AND pattern "f*", publishing to "foo" should deliver TWO messages: one as "message" type and one as "pmessage" type.
**Why it happens:** Redis deliberately delivers to both matching subscriptions independently.
**How to avoid:** When publishing, iterate BOTH channel subscribers and pattern subscribers. A single client can receive multiple messages for one publish if they have overlapping subscriptions. The PubSub class must handle this correctly.
**Warning signs:** Deduplication logic accidentally removing valid duplicate deliveries.

### Pitfall 4: Broadcast channel capacity and lagged receivers

**What goes wrong:** `tokio::sync::broadcast` has a fixed capacity. If a Python subscriber is slow to consume, the receiver becomes lagged and gets a `RecvError::Lagged` error, losing messages.
**Why it happens:** Broadcast channel drops oldest messages when capacity is exceeded.
**How to avoid:** Use a sufficiently large capacity (e.g., 1024 or 4096). For an embedded in-process database, the publisher and subscriber are in the same process, so lag should be minimal. Handle `RecvError::Lagged` by logging a warning and continuing (fire-and-forget semantics mean message loss is acceptable).
**Warning signs:** `RecvError::Lagged` in logs during high-throughput pub/sub.

### Pitfall 5: run_in_thread() lifecycle management

**What goes wrong:** The background thread created by `run_in_thread()` doesn't stop cleanly, or it holds references that prevent garbage collection.
**Why it happens:** Thread doesn't have a stop mechanism, or the event loop in the thread conflicts with the main asyncio loop.
**How to avoid:** Return a thread object with a `stop()` method (matching redis-py). Use a `threading.Event` to signal the thread to stop. The thread runs its own `asyncio.run()` loop. Mark as daemon thread so it doesn't prevent process exit.
**Warning signs:** Process hangs on exit, or thread continues running after PubSub.close().

### Pitfall 6: PUBLISH in Lua scripts needing special handling

**What goes wrong:** `dispatch_command("PUBLISH", ...)` inside Lua is called while holding the data write lock. The publish implementation tries to broadcast to subscribers, but the broadcast itself doesn't need the data lock.
**Why it happens:** The Lua execution model holds the data write lock for atomicity.
**How to avoid:** `dispatch_command` for PUBLISH should collect the message (channel + data) and return the subscriber count, but the actual broadcast can happen through the broadcast channel sender which doesn't need the data lock. The `broadcast::Sender::send()` is lock-free. Key insight: Store the `broadcast::Sender` separately (not behind the data RwLock), so `dispatch_command` can access it.
**Warning signs:** Deadlocks or panics when running Lua scripts that call `redis.call('PUBLISH', ...)`.

## Code Examples

### Rust: Store publish method
```rust
// Source: Design based on tokio::sync::broadcast docs [CITED: docs.rs/tokio/latest/tokio/sync/broadcast]
impl Store {
    /// PUBLISH: Send message to channel, return number of subscribers that received it.
    pub fn publish(&self, channel: Bytes, message: Bytes) -> i64 {
        let registry = self.pubsub.read();

        // Count direct channel subscribers
        let channel_count = registry.channel_subscribers
            .get(&channel)
            .map(|s| s.len() as i64)
            .unwrap_or(0);

        // Count pattern subscribers that match
        let pattern_count: i64 = registry.pattern_subscribers
            .iter()
            .filter(|(pattern, _)| glob_match(pattern, &channel))
            .map(|(_, subs)| subs.len() as i64)
            .sum();

        // Broadcast the message (fire-and-forget)
        let msg = PubSubMessage {
            kind: "message".to_string(),
            pattern: None,
            channel: channel.clone(),
            data: message,
        };
        let _ = registry.tx.send(msg);
        // Ignore send error (no receivers = 0 delivered, which is fine)

        channel_count + pattern_count
    }
}
```

### Rust: Store subscribe method
```rust
// Source: Design pattern [ASSUMED]
impl Store {
    /// SUBSCRIBE: Register a subscriber for specific channels.
    /// Returns a receiver handle and the subscription count.
    pub fn subscribe(&self, subscriber_id: u64, channels: &[Bytes]) -> Vec<(Bytes, i64)> {
        let mut registry = self.pubsub.write();
        let mut results = Vec::new();

        for channel in channels {
            registry.channel_subscribers
                .entry(channel.clone())
                .or_default()
                .insert(subscriber_id);

            // Count total subscriptions for this subscriber
            let total = registry.count_subscriptions(subscriber_id);
            results.push((channel.clone(), total as i64));
        }

        results
    }
}
```

### Python: PubSub class message handling
```python
# Source: redis-py PubSub.handle_message [CITED: github.com/redis/redis-py]
async def handle_message(self, message, ignore_subscribe_messages=False):
    """Process a raw message dict. Dispatch to handler if registered."""
    message_type = message["type"]

    if message_type in self.PUBLISH_MESSAGE_TYPES:
        # Check for registered handler
        if message_type == "pmessage":
            handler = self.patterns.get(message["pattern"])
        else:
            handler = self.channels.get(message["channel"])

        if handler is not None:
            if asyncio.iscoroutinefunction(handler):
                await handler(message)
            else:
                handler(message)
            return None  # Handler consumed it

    elif message_type in self.UNSUBSCRIBE_MESSAGE_TYPES:
        # Update internal subscription tracking
        if message_type == "punsubscribe":
            self.patterns.pop(message["channel"], None)
        else:
            self.channels.pop(message["channel"], None)

        if ignore_subscribe_messages or self.ignore_subscribe_messages:
            return None

    elif message_type in ("subscribe", "psubscribe"):
        if ignore_subscribe_messages or self.ignore_subscribe_messages:
            return None

    return message
```

### Python: PubSub.listen() async generator
```python
# Source: redis-py async PubSub.listen [CITED: github.com/redis/redis-py]
async def listen(self):
    """Async generator that yields messages until unsubscribed."""
    while self.subscribed:
        response = await self.get_message(timeout=None)
        if response is not None:
            yield response
```

### Python: Message format
```python
# Source: redis-py documentation [CITED: redis.readthedocs.io/en/stable/advanced_features.html]
# Regular message:
{"type": "message", "pattern": None, "channel": b"mychannel", "data": b"hello"}

# Pattern message:
{"type": "pmessage", "pattern": b"my*", "channel": b"mychannel", "data": b"hello"}

# Subscribe confirmation:
{"type": "subscribe", "pattern": None, "channel": b"mychannel", "data": 1}

# Unsubscribe confirmation:
{"type": "unsubscribe", "pattern": None, "channel": b"mychannel", "data": 0}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `redis-py` PubSub.close() | PubSub.aclose() | redis-py 5.0.1 | close() and reset() are deprecated aliases for aclose() |
| Sync PubSub.run_in_thread() | Async PubSub.run() with asyncio.create_task() | redis-py 5.x | Async version uses `run()` coroutine instead of `run_in_thread()`. Both should be supported for compat. |
| aioredis separate library | Merged into redis-py | redis-py 4.2+ | Async PubSub is now in `redis.asyncio`, not a separate package |

**Deprecated/outdated:**
- `PubSub.close()` and `PubSub.reset()` -- deprecated in redis-py 5.0.1, replaced by `aclose()`. We should support all three for maximum compatibility.

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | pydocket does not use Redis pub/sub (verified: zero search results for subscribe/publish in docket repo) | Summary | LOW -- verified via GitHub code search. If wrong, need to check which pub/sub patterns pydocket uses. |
| A2 | Single global broadcast channel with client-side filtering is sufficient for embedded use | Architecture Patterns | LOW -- for an embedded single-process DB, the performance difference vs per-channel routing is negligible. Could be refactored later if needed. |
| A3 | Broadcast channel capacity of 1024-4096 is sufficient | Pitfalls | LOW -- embedded DB has publisher and subscriber in same process, so backpressure is self-regulating. |
| A4 | `run_in_thread()` running its own `asyncio.run()` is the correct approach for background processing | Pitfalls | MEDIUM -- depends on Python version and asyncio event loop policy. May need `asyncio.new_event_loop()` instead. Should verify in testing. |

## Open Questions

1. **pydocket motivation mismatch**
   - What we know: D-01 says pydocket drives this phase. Investigation confirms pydocket uses ONLY Redis Streams, never pub/sub.
   - What's unclear: Whether there's another package or use case driving the pub/sub need, or if this is general Redis compatibility.
   - Recommendation: Proceed with implementation as planned. Pub/sub is a standard Redis feature. The user has explicitly scoped it in D-02.

2. **Broadcast capacity sizing**
   - What we know: `tokio::sync::broadcast` requires a fixed capacity at creation.
   - What's unclear: What's the expected message throughput for this embedded use case.
   - Recommendation: Default to 4096. Sufficient for any reasonable embedded use. Document as a tunable constant.

3. **Shard pub/sub (SSUBSCRIBE/SUNSUBSCRIBE/SPUBLISH)**
   - What we know: Redis 7.0+ added sharded pub/sub for cluster mode.
   - What's unclear: Whether any compatibility is needed.
   - Recommendation: Out of scope. This is a single-process embedded DB with no cluster support.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | pytest + pytest-asyncio |
| Config file | pyproject.toml `[tool.pytest.ini_options]` |
| Quick run command | `pytest tests/test_pubsub.py -x` |
| Full suite command | `pytest tests/ -x` |

### Phase Requirements to Test Map

Since requirement IDs are TBD, mapping against CONTEXT.md decisions:

| Decision | Behavior | Test Type | Automated Command | File Exists? |
|----------|----------|-----------|-------------------|-------------|
| D-02 SUBSCRIBE | Client can subscribe to channels and receive messages | unit | `pytest tests/test_pubsub.py::test_subscribe_and_publish -x` | No -- Wave 0 |
| D-02 UNSUBSCRIBE | Client can unsubscribe from channels | unit | `pytest tests/test_pubsub.py::test_unsubscribe -x` | No -- Wave 0 |
| D-02 PUBLISH | Client can publish messages, returns subscriber count | unit | `pytest tests/test_pubsub.py::test_publish_returns_count -x` | No -- Wave 0 |
| D-02 PSUBSCRIBE | Client can subscribe to glob patterns | unit | `pytest tests/test_pubsub.py::test_psubscribe_pattern -x` | No -- Wave 0 |
| D-02 PUNSUBSCRIBE | Client can unsubscribe from patterns | unit | `pytest tests/test_pubsub.py::test_punsubscribe -x` | No -- Wave 0 |
| D-02 PUBSUB CHANNELS | Introspect active channels | unit | `pytest tests/test_pubsub.py::test_pubsub_channels -x` | No -- Wave 0 |
| D-02 PUBSUB NUMSUB | Count subscribers per channel | unit | `pytest tests/test_pubsub.py::test_pubsub_numsub -x` | No -- Wave 0 |
| D-02 PUBSUB NUMPAT | Count active patterns | unit | `pytest tests/test_pubsub.py::test_pubsub_numpat -x` | No -- Wave 0 |
| D-04 pubsub() factory | client.pubsub() returns PubSub instance | unit | `pytest tests/test_pubsub.py::test_pubsub_factory -x` | No -- Wave 0 |
| D-05 handler callbacks | Per-channel handlers invoked automatically | unit | `pytest tests/test_pubsub.py::test_handler_callback -x` | No -- Wave 0 |
| D-07 run_in_thread | Background thread processes messages | unit | `pytest tests/test_pubsub.py::test_run_in_thread -x` | No -- Wave 0 |
| D-08 ignore_subscribe | Confirmation messages filtered | unit | `pytest tests/test_pubsub.py::test_ignore_subscribe_messages -x` | No -- Wave 0 |
| D-09 listen() | Async generator yields messages | unit | `pytest tests/test_pubsub.py::test_listen_generator -x` | No -- Wave 0 |
| D-09 get_message() | Polling returns message or None | unit | `pytest tests/test_pubsub.py::test_get_message_polling -x` | No -- Wave 0 |
| D-11 Lua PUBLISH | redis.call('PUBLISH') works in Lua scripts | unit | `pytest tests/test_pubsub.py::test_lua_publish -x` | No -- Wave 0 |
| D-12 Pipeline PUBLISH | PUBLISH in pipeline executes with batch | unit | `pytest tests/test_pubsub.py::test_pipeline_publish -x` | No -- Wave 0 |
| D-13 fire-and-forget | Messages not persisted across restart | unit | `pytest tests/test_pubsub.py::test_no_persistence -x` | No -- Wave 0 |

### Sampling Rate
- **Per task commit:** `pytest tests/test_pubsub.py -x`
- **Per wave merge:** `pytest tests/ -x`
- **Phase gate:** Full suite green before `/gsd-verify-work`

### Wave 0 Gaps
- [ ] `tests/test_pubsub.py` -- covers all pub/sub decisions
- No framework install needed (pytest + pytest-asyncio already configured)

## Security Domain

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | N/A -- in-process embedded, no auth boundary |
| V3 Session Management | no | N/A |
| V4 Access Control | no | N/A -- single-process |
| V5 Input Validation | yes | Validate channel names are valid bytes, pattern syntax is well-formed |
| V6 Cryptography | no | N/A |

### Known Threat Patterns

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Glob pattern ReDoS | Denial of Service | Implement iterative (not recursive) pattern matching with bounded complexity; Redis's own stringmatchlen had exponential complexity bugs fixed in 2023 |
| Broadcast channel exhaustion | Denial of Service | Cap broadcast capacity, handle Lagged errors gracefully |

## Sources

### Primary (HIGH confidence)
- [redis-py async PubSub source (github.com/redis/redis-py)](https://github.com/redis/redis-py/blob/master/redis/asyncio/client.py) -- Full API surface extracted
- [Redis Pub/Sub documentation (redis.io)](https://redis.io/docs/latest/develop/pubsub/) -- Protocol semantics, message format
- [Tokio broadcast channel docs (docs.rs)](https://docs.rs/tokio/latest/tokio/sync/broadcast/index.html) -- API, capacity, error handling
- [Tokio sync feature flag](https://docs.rs/tokio/latest/tokio/sync/index.html) -- Verified broadcast is in `sync` feature
- Cargo.toml verified: tokio sync feature already enabled
- [PUBSUB CHANNELS (redis.io)](https://redis.io/docs/latest/commands/pubsub-channels/) -- return format
- [PUBSUB NUMSUB (redis.io)](https://redis.io/docs/latest/commands/pubsub-numsub/) -- return format
- [pydocket PyPI page](https://pypi.org/project/pydocket/) -- confirmed streams-only, no pub/sub
- [docket GitHub repo search](https://github.com/chrisguidry/docket) -- zero pub/sub references confirmed

### Secondary (MEDIUM confidence)
- [Redis stringmatchlen glob matching](https://github.com/redis/redis/blob/master/src/util.c) -- pattern matching reference implementation
- [redis-py advanced features docs](https://redis.readthedocs.io/en/stable/advanced_features.html) -- PubSub usage patterns

### Tertiary (LOW confidence)
- None

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- no new dependencies, all from Tokio/Python stdlib
- Architecture: HIGH -- follows established project patterns (monkey-patch, pure Python wrapper, Store methods)
- Pitfalls: HIGH -- well-documented Redis pub/sub semantics, clear deadlock scenarios identified
- Glob matching: MEDIUM -- hand-rolled implementation needs careful testing, but algorithm is well-known

**Research date:** 2026-04-13
**Valid until:** 2026-05-13 (stable -- pub/sub protocol is mature and unchanging)
