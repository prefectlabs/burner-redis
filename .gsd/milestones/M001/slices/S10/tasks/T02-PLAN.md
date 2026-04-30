# T02: Implement the Python PubSub class mirroring redis-py's async PubSub interface, integrate PUBLISH into Pipeline and Lua scripting, and provide a comprehensive test suite covering all 13 CONTEXT.

**Slice:** S10 — **Milestone:** M001

## Description

Implement the Python PubSub class mirroring redis-py's async PubSub interface, integrate PUBLISH into Pipeline and Lua scripting, and provide a comprehensive test suite covering all 13 CONTEXT.md decisions.

Purpose: Completes the pub/sub feature by providing the user-facing Python API and ensuring PUBLISH works across all execution contexts (direct, pipeline, Lua script).

Output: New python/burner_redis/pubsub.py, updated __init__.py and pipeline.py, updated scripting.rs with PUBLISH dispatch, and tests/test_pubsub.py.

## Legacy Source

---
phase: 10-add-pub-sub-support
plan: 02
type: execute
wave: 2
depends_on: ["10-01"]
files_modified:
  - python/burner_redis/pubsub.py
  - python/burner_redis/__init__.py
  - python/burner_redis/pipeline.py
  - src/scripting.rs
  - tests/test_pubsub.py
autonomous: true
requirements:
  - PUBSUB-07
  - PUBSUB-08
  - PUBSUB-09
  - PUBSUB-10
  - PUBSUB-11
  - PUBSUB-12

must_haves:
  truths:
    - "client.pubsub() returns a PubSub object (monkey-patched onto BurnerRedis)"
    - "PubSub.subscribe() registers channels and delivers confirmation messages"
    - "PubSub.psubscribe() registers patterns and delivers pmessage on match"
    - "PubSub.get_message() returns a dict with type/pattern/channel/data keys or None"
    - "PubSub.listen() is an async generator that yields messages"
    - "Handler callbacks are invoked automatically for subscribed channels"
    - "ignore_subscribe_messages=True filters subscribe/unsubscribe confirmations"
    - "run_in_thread() processes messages in a background daemon thread"
    - "PUBLISH works inside Lua scripts via redis.call('PUBLISH', channel, message)"
    - "Pipeline.publish() queues PUBLISH for batch execution"
    - "Messages are dicts with type/pattern/channel/data keys matching redis-py format"
  artifacts:
    - path: "python/burner_redis/pubsub.py"
      provides: "PubSub class with subscribe, unsubscribe, psubscribe, punsubscribe, listen, get_message, handle_message, run_in_thread, close, aclose, reset, subscribed property"
      contains: "class PubSub"
    - path: "python/burner_redis/__init__.py"
      provides: "pubsub() factory monkey-patched onto BurnerRedis"
      contains: "def _pubsub"
    - path: "python/burner_redis/pipeline.py"
      provides: "publish() method on Pipeline"
      contains: "def publish"
    - path: "src/scripting.rs"
      provides: "PUBLISH command in dispatch_command"
      contains: "\"PUBLISH\""
    - path: "tests/test_pubsub.py"
      provides: "Comprehensive test suite for all pub/sub decisions"
      contains: "test_subscribe_and_publish"
  key_links:
    - from: "python/burner_redis/pubsub.py"
      to: "src/lib.rs"
      via: "PubSub calls client._subscribe_listener, client.subscribe_channels, client.publish, etc."
      pattern: "self\\._client\\.(subscribe_channels|unsubscribe_channels|psubscribe_patterns|punsubscribe_patterns|publish|_new_subscriber|_subscribe_listener)"
    - from: "python/burner_redis/__init__.py"
      to: "python/burner_redis/pubsub.py"
      via: "import PubSub, monkey-patch pubsub() factory"
      pattern: "from burner_redis.pubsub import PubSub"
    - from: "python/burner_redis/pipeline.py"
      to: "src/lib.rs"
      via: "Pipeline.publish queues the publish method for batch execution"
      pattern: "def publish"
    - from: "src/scripting.rs"
      to: "src/store.rs"
      via: "dispatch_command PUBLISH uses broadcast sender to fan out"
      pattern: "\"PUBLISH\""
---

<objective>
Implement the Python PubSub class mirroring redis-py's async PubSub interface, integrate PUBLISH into Pipeline and Lua scripting, and provide a comprehensive test suite covering all 13 CONTEXT.md decisions.

Purpose: Completes the pub/sub feature by providing the user-facing Python API and ensuring PUBLISH works across all execution contexts (direct, pipeline, Lua script).

Output: New python/burner_redis/pubsub.py, updated __init__.py and pipeline.py, updated scripting.rs with PUBLISH dispatch, and tests/test_pubsub.py.
</objective>

<execution_context>
@$HOME/.claude/get-shit-done/workflows/execute-plan.md
@$HOME/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@.planning/PROJECT.md
@.planning/ROADMAP.md
@.planning/STATE.md
@.planning/phases/10-add-pub-sub-support-subscribe-unsubscribe-publish-psubscribe/10-CONTEXT.md
@.planning/phases/10-add-pub-sub-support-subscribe-unsubscribe-publish-psubscribe/10-RESEARCH.md
@.planning/phases/10-add-pub-sub-support-subscribe-unsubscribe-publish-psubscribe/10-01-SUMMARY.md

<interfaces>
<!-- Key types and contracts from Plan 01 that this plan depends on. -->

From src/lib.rs (Plan 01 additions):
```python
# These are the Rust methods exposed on BurnerRedis that PubSub class calls:
await client.publish(channel: bytes, message: bytes) -> int  # subscriber count
client._new_subscriber() -> int  # subscriber_id (sync, no await)
await client._subscribe_listener(subscriber_id: int, queue: asyncio.Queue) -> int
await client.subscribe_channels(subscriber_id: int, channels: list[bytes]) -> list[tuple[bytes, int]]
await client.unsubscribe_channels(subscriber_id: int, channels: list[bytes]) -> list[tuple[bytes, int]]
await client.psubscribe_patterns(subscriber_id: int, patterns: list[bytes]) -> list[tuple[bytes, int]]
await client.punsubscribe_patterns(subscriber_id: int, patterns: list[bytes]) -> list[tuple[bytes, int]]
await client.pubsub_channels(pattern: bytes | None = None) -> list[bytes]
await client.pubsub_numsub(channels: list[bytes]) -> list[tuple[bytes, int]]
await client.pubsub_numpat() -> int
```

From python/burner_redis/__init__.py (monkey-patch pattern):
```python
def _pipeline(self):
    return Pipeline(self)
BurnerRedis.pipeline = _pipeline

def _lock(self, name, timeout=None, sleep=0.1, blocking=True, blocking_timeout=None):
    return Lock(self, name, ...)
BurnerRedis.lock = _lock
```

From python/burner_redis/pipeline.py (command buffer pattern):
```python
class Pipeline:
    def __init__(self, client):
        self._client = client
        self._commands = []
    
    def some_command(self, *args, **kwargs):
        self._commands.append(("method_name", args, kwargs))
        return self
```

From src/scripting.rs (dispatch_command signature):
```rust
fn dispatch_command(
    cmd: &str,
    args: &[Bytes],
    data: &mut HashMap<Bytes, ValueEntry>,
) -> Result<RedisValue, String> {
    match cmd {
        "GET" => { ... }
        // ... other commands ...
        _ => Ok(RedisValue::Error(format!("ERR unknown command '{}'", cmd))),
    }
}
```

Note: dispatch_command needs a broadcast::Sender parameter added for PUBLISH support.
The LuaEngine::execute signature also needs updating, and Store::eval/evalsha must pass the sender.
</interfaces>
</context>

<tasks>

<task type="auto">
  <name>Task 1: Python PubSub class, monkey-patch, Pipeline.publish, and Lua PUBLISH dispatch</name>
  <files>python/burner_redis/pubsub.py, python/burner_redis/__init__.py, python/burner_redis/pipeline.py, src/scripting.rs, src/store.rs</files>
  <read_first>
    - python/burner_redis/__init__.py (monkey-patch pattern for pipeline/lock)
    - python/burner_redis/pipeline.py (command buffer pattern)
    - python/burner_redis/lock.py (pure-Python wrapper class reference)
    - src/scripting.rs (full file -- dispatch_command, LuaEngine::execute, redis.call setup)
    - src/store.rs (eval and evalsha methods -- lines around 1578-1604)
    - src/lib.rs (new pub/sub #[pymethods] from Plan 01 -- understand exact signatures)
  </read_first>
  <action>
**1. Create `python/burner_redis/pubsub.py`:**

```python
"""PubSub class for Redis-compatible pub/sub messaging.

Provides redis-py compatible async PubSub API with subscribe/unsubscribe,
pattern matching, message handlers, and background thread processing.
"""
import asyncio
import threading


class PubSub:
    """Async pub/sub message handler mirroring redis-py's PubSub interface.
    
    Created via client.pubsub(). Manages channel/pattern subscriptions
    and message delivery through an internal asyncio.Queue fed by a
    Rust background task.
    """

    PUBLISH_MESSAGE_TYPES = ("message", "pmessage")
    UNSUBSCRIBE_MESSAGE_TYPES = ("unsubscribe", "punsubscribe")
    HEALTH_CHECK_MESSAGE = "burner-redis-pubsub-health-check"

    def __init__(self, client, ignore_subscribe_messages=False):
        self._client = client
        self.ignore_subscribe_messages = ignore_subscribe_messages
        self.channels = {}      # channel_bytes -> handler or None
        self.patterns = {}      # pattern_bytes -> handler or None
        self._queue = asyncio.Queue()
        self._subscriber_id = None
        self._listener_started = False

    @property
    def subscribed(self):
        """True if this PubSub has any active subscriptions."""
        return bool(self.channels or self.patterns)

    async def _ensure_listener(self):
        """Start the Rust background listener if not already running."""
        if not self._listener_started:
            self._subscriber_id = self._client._new_subscriber()
            await self._client._subscribe_listener(self._subscriber_id, self._queue)
            self._listener_started = True

    async def subscribe(self, *args, **kwargs):
        """Subscribe to one or more channels.
        
        Positional args are channel names (no handler).
        Keyword args map channel names to handler callables.
        
        Example:
            await pubsub.subscribe('channel1', 'channel2')
            await pubsub.subscribe(channel1=handler_func)
        """
        await self._ensure_listener()
        
        new_channels = {}
        for arg in args:
            new_channels[self._encode(arg)] = None
        for channel, handler in kwargs.items():
            new_channels[self._encode(channel)] = handler
        
        if not new_channels:
            return
        
        channel_list = list(new_channels.keys())
        results = await self._client.subscribe_channels(
            self._subscriber_id, channel_list
        )
        
        self.channels.update(new_channels)
        
        # Generate subscribe confirmation messages
        for channel_bytes, count in results:
            msg = {
                "type": "subscribe",
                "pattern": None,
                "channel": channel_bytes,
                "data": count,
            }
            await self._queue.put(msg)

    async def unsubscribe(self, *args):
        """Unsubscribe from one or more channels. If no args, unsubscribe from all."""
        if self._subscriber_id is None:
            return
        
        if args:
            channel_list = [self._encode(a) for a in args]
        else:
            channel_list = list(self.channels.keys())
        
        if not channel_list:
            return
        
        results = await self._client.unsubscribe_channels(
            self._subscriber_id, channel_list
        )
        
        for channel_bytes, count in results:
            self.channels.pop(channel_bytes, None)
            msg = {
                "type": "unsubscribe",
                "pattern": None,
                "channel": channel_bytes,
                "data": count,
            }
            await self._queue.put(msg)

    async def psubscribe(self, *args, **kwargs):
        """Subscribe to one or more glob patterns.
        
        Positional args are patterns (no handler).
        Keyword args map patterns to handler callables.
        """
        await self._ensure_listener()
        
        new_patterns = {}
        for arg in args:
            new_patterns[self._encode(arg)] = None
        for pattern, handler in kwargs.items():
            new_patterns[self._encode(pattern)] = handler
        
        if not new_patterns:
            return
        
        pattern_list = list(new_patterns.keys())
        results = await self._client.psubscribe_patterns(
            self._subscriber_id, pattern_list
        )
        
        self.patterns.update(new_patterns)
        
        for pattern_bytes, count in results:
            msg = {
                "type": "psubscribe",
                "pattern": None,
                "channel": pattern_bytes,
                "data": count,
            }
            await self._queue.put(msg)

    async def punsubscribe(self, *args):
        """Unsubscribe from one or more patterns. If no args, unsubscribe from all."""
        if self._subscriber_id is None:
            return
        
        if args:
            pattern_list = [self._encode(a) for a in args]
        else:
            pattern_list = list(self.patterns.keys())
        
        if not pattern_list:
            return
        
        results = await self._client.punsubscribe_patterns(
            self._subscriber_id, pattern_list
        )
        
        for pattern_bytes, count in results:
            self.patterns.pop(pattern_bytes, None)
            msg = {
                "type": "punsubscribe",
                "pattern": None,
                "channel": pattern_bytes,
                "data": count,
            }
            await self._queue.put(msg)

    async def get_message(self, ignore_subscribe_messages=False, timeout=0.0):
        """Get the next message or None.
        
        Args:
            ignore_subscribe_messages: Filter subscribe/unsubscribe confirmations
            timeout: Seconds to wait. 0.0 = non-blocking. None = block forever.
        
        Returns:
            Message dict or None if no message available within timeout.
        """
        while True:
            try:
                if timeout is None:
                    # Block forever
                    raw = await self._queue.get()
                elif timeout == 0.0:
                    # Non-blocking
                    try:
                        raw = self._queue.get_nowait()
                    except asyncio.QueueEmpty:
                        return None
                else:
                    # Wait with timeout
                    try:
                        raw = await asyncio.wait_for(self._queue.get(), timeout=timeout)
                    except asyncio.TimeoutError:
                        return None
            except Exception:
                return None
            
            # Filter messages based on subscriptions
            message = self._filter_message(raw)
            if message is None:
                continue
            
            result = await self.handle_message(message, ignore_subscribe_messages)
            if result is not None:
                return result
            
            # If handler consumed the message or it was filtered, try again
            # (but only if blocking)
            if timeout == 0.0:
                return None

    def _filter_message(self, raw):
        """Filter broadcast messages to only those matching this subscriber's channels/patterns."""
        msg_type = raw.get("type")
        
        # Subscribe/unsubscribe confirmations are always for this subscriber
        if msg_type in ("subscribe", "unsubscribe", "psubscribe", "punsubscribe"):
            return raw
        
        if msg_type == "message":
            # Only deliver if this subscriber is subscribed to this channel
            channel = raw.get("channel")
            if channel in self.channels:
                return raw
            return None
        
        if msg_type == "pmessage":
            # Only deliver if this subscriber has the matching pattern
            pattern = raw.get("pattern")
            if pattern in self.patterns:
                return raw
            return None
        
        return raw

    async def handle_message(self, message, ignore_subscribe_messages=False):
        """Process a message dict. Dispatch to handler if registered.
        
        Returns the message if no handler consumed it, None if handled.
        """
        message_type = message["type"]
        
        if message_type in self.PUBLISH_MESSAGE_TYPES:
            # Check for registered handler
            if message_type == "pmessage":
                handler = self.patterns.get(message.get("pattern"))
            else:
                handler = self.channels.get(message.get("channel"))
            
            if handler is not None:
                if asyncio.iscoroutinefunction(handler):
                    await handler(message)
                else:
                    handler(message)
                return None  # Handler consumed it
        
        elif message_type in self.UNSUBSCRIBE_MESSAGE_TYPES:
            if ignore_subscribe_messages or self.ignore_subscribe_messages:
                return None
        
        elif message_type in ("subscribe", "psubscribe"):
            if ignore_subscribe_messages or self.ignore_subscribe_messages:
                return None
        
        return message

    async def listen(self):
        """Async generator that yields messages until unsubscribed."""
        while self.subscribed:
            response = await self.get_message(timeout=None)
            if response is not None:
                yield response

    def run_in_thread(self, sleep_time=0.0, daemon=True):
        """Start a background thread that processes messages.
        
        Returns a PubSubWorkerThread with a stop() method.
        """
        thread = PubSubWorkerThread(self, sleep_time=sleep_time, daemon=daemon)
        thread.start()
        return thread

    async def close(self):
        """Close the PubSub, unsubscribing from all channels and patterns."""
        await self.aclose()

    async def aclose(self):
        """Async close -- unsubscribe from all channels and patterns."""
        if self.channels:
            await self.unsubscribe()
        if self.patterns:
            await self.punsubscribe()

    async def reset(self):
        """Reset the PubSub state (deprecated alias for aclose)."""
        await self.aclose()

    def _encode(self, value):
        """Encode a value to bytes if it's a string."""
        if isinstance(value, bytes):
            return value
        if isinstance(value, str):
            return value.encode("utf-8")
        if isinstance(value, memoryview):
            return bytes(value)
        return bytes(str(value), "utf-8")


class PubSubWorkerThread(threading.Thread):
    """Background thread that processes pub/sub messages.
    
    Runs its own asyncio event loop in a daemon thread.
    Call stop() to signal the thread to exit.
    """

    def __init__(self, pubsub, sleep_time=0.0, daemon=True):
        super().__init__(daemon=daemon)
        self._pubsub = pubsub
        self._sleep_time = sleep_time
        self._stop_event = threading.Event()

    def run(self):
        """Run the message processing loop in a new asyncio event loop."""
        loop = asyncio.new_event_loop()
        try:
            loop.run_until_complete(self._process_messages())
        finally:
            loop.close()

    async def _process_messages(self):
        """Process messages until stopped."""
        while not self._stop_event.is_set():
            try:
                msg = await self._pubsub.get_message(
                    ignore_subscribe_messages=True,
                    timeout=0.5,
                )
                if msg is not None and self._sleep_time > 0:
                    await asyncio.sleep(self._sleep_time)
            except Exception:
                break

    def stop(self):
        """Signal the thread to stop processing."""
        self._stop_event.set()
```

**2. Update `python/burner_redis/__init__.py`:**

Add import at top:
```python
from burner_redis.pubsub import PubSub
```

Add monkey-patch after the existing `BurnerRedis.lock = _lock` line:
```python
def _pubsub(self, ignore_subscribe_messages=False):
    """Create a PubSub for channel/pattern message subscription."""
    return PubSub(self, ignore_subscribe_messages=ignore_subscribe_messages)

BurnerRedis.pubsub = _pubsub
```

Update `__all__` to include `"PubSub"`:
```python
__all__ = ["BurnerRedis", "Lock", "LockError", "Pipeline", "PubSub", "ResponseError"]
```

**3. Update `python/burner_redis/pipeline.py`:**

Add a `publish` method in a new `# ---- Pub/Sub Commands ----` section after the Scripting section:
```python
# ---- Pub/Sub Commands ----

def publish(self, channel, message):
    self._commands.append(("publish", (channel, message), {}))
    return self
```

**4. Update `src/scripting.rs` to handle PUBLISH in dispatch_command:**

This requires changing the `dispatch_command` signature to accept an optional broadcast sender for PUBLISH fan-out. The changes are:

**4a. Change `dispatch_command` signature:**
```rust
fn dispatch_command(
    cmd: &str,
    args: &[Bytes],
    data: &mut HashMap<Bytes, ValueEntry>,
    pubsub_tx: Option<&broadcast::Sender<PubSubMessage>>,
) -> Result<RedisValue, String> {
```

Add import at top of scripting.rs:
```rust
use tokio::sync::broadcast;
use crate::store::PubSubMessage;
use crate::commands::pubsub::glob_match;
```

**4b. Add PUBLISH match arm** before the `_ =>` catch-all in `dispatch_command`:
```rust
"PUBLISH" => {
    if args.len() != 2 {
        return Ok(RedisValue::Error(
            "ERR wrong number of arguments for 'publish' command".to_string(),
        ));
    }
    let channel = &args[0];
    let message = &args[1];
    
    match pubsub_tx {
        Some(tx) => {
            // Count subscribers by checking if anyone is listening
            // (in Lua context we don't have access to the registry for counting,
            // so we send the message and return the number of receivers)
            let _ = tx.send(PubSubMessage {
                kind: "message".to_string(),
                pattern: None,
                channel: channel.clone(),
                data: message.clone(),
            });
            // Return the number of receivers (approximate)
            Ok(RedisValue::Integer(tx.receiver_count() as i64))
        }
        None => {
            // No pubsub sender available (shouldn't happen in normal operation)
            Ok(RedisValue::Integer(0))
        }
    }
}
```

**4c. Update all calls to `dispatch_command` within scripting.rs** to pass the new `pubsub_tx` parameter. There are two call sites: inside the `redis.call` closure and inside the `redis.pcall` closure. Both currently call:
```rust
let result = dispatch_command(&cmd_name, &cmd_args, *data_ref);
```
Change to:
```rust
let result = dispatch_command(&cmd_name, &cmd_args, *data_ref, pubsub_tx_ref);
```

To make `pubsub_tx` available in the Lua closures, the `LuaEngine::execute` signature must also change:
```rust
pub fn execute(
    script: &str,
    keys: Vec<Bytes>,
    args: Vec<Bytes>,
    data: &mut HashMap<Bytes, ValueEntry>,
    pubsub_tx: Option<&broadcast::Sender<PubSubMessage>>,
) -> Result<RedisValue, String> {
```

Store `pubsub_tx` in a way accessible to the Lua scope closures. Since `pubsub_tx` is `Option<&broadcast::Sender<...>>`, clone the sender into the closure:
```rust
let pubsub_tx_clone = pubsub_tx.cloned();
// In the closure:
let pubsub_tx_ref = pubsub_tx_clone.as_ref();
let result = dispatch_command(&cmd_name, &cmd_args, *data_ref, pubsub_tx_ref);
```

**4d. Update `src/store.rs` eval and evalsha methods** to pass the pubsub sender:

In `Store::eval`:
```rust
pub fn eval(&self, script: &str, keys: Vec<Bytes>, args: Vec<Bytes>) -> Result<RedisValue, String> {
    let sha1 = LuaEngine::sha1_hex(script);
    self.scripts.write().insert(sha1, script.to_string());
    let pubsub_tx = self.pubsub_sender();
    let mut data = self.data.write();
    LuaEngine::execute(script, keys, args, &mut *data, Some(&pubsub_tx))
}
```

In `Store::evalsha`:
```rust
pub fn evalsha(&self, sha: &str, keys: Vec<Bytes>, args: Vec<Bytes>) -> Result<RedisValue, String> {
    let script = {
        let scripts = self.scripts.read();
        match scripts.get(sha) {
            Some(s) => s.clone(),
            None => return Err("NOSCRIPT No matching script. Use EVAL.".to_string()),
        }
    };
    let pubsub_tx = self.pubsub_sender();
    let mut data = self.data.write();
    LuaEngine::execute(&script, keys, args, &mut *data, Some(&pubsub_tx))
}
```

**Key implementation notes:**
- The broadcast sender is cloned BEFORE acquiring the data write lock. This avoids accessing the pubsub RwLock while holding the data write lock (deadlock prevention per research Pitfall 1).
- `pubsub_tx.receiver_count()` gives an approximate subscriber count for PUBLISH in Lua -- this is acceptable for embedded use.
- The PubSub class _filter_message() method filters broadcast messages by the subscriber's own channel/pattern sets. This is critical because the single broadcast channel delivers ALL messages to ALL receivers.
- run_in_thread() creates its own asyncio event loop via `asyncio.new_event_loop()` (not `asyncio.run()`) for Python 3.9 compatibility.
- PubSubWorkerThread.stop() uses threading.Event for clean shutdown signaling.
  </action>
  <verify>
    <automated>cd /Users/alexander/dev/prefectlabs/burner-redis && cargo check 2>&1 | tail -5</automated>
  </verify>
  <acceptance_criteria>
    - python/burner_redis/pubsub.py contains `class PubSub`
    - python/burner_redis/pubsub.py contains `class PubSubWorkerThread`
    - python/burner_redis/pubsub.py contains `async def subscribe(self, *args, **kwargs)`
    - python/burner_redis/pubsub.py contains `async def unsubscribe(self, *args)`
    - python/burner_redis/pubsub.py contains `async def psubscribe(self, *args, **kwargs)`
    - python/burner_redis/pubsub.py contains `async def punsubscribe(self, *args)`
    - python/burner_redis/pubsub.py contains `async def get_message(`
    - python/burner_redis/pubsub.py contains `async def listen(self)`
    - python/burner_redis/pubsub.py contains `def run_in_thread(`
    - python/burner_redis/pubsub.py contains `async def close(self)`
    - python/burner_redis/pubsub.py contains `async def aclose(self)`
    - python/burner_redis/pubsub.py contains `def _filter_message(`
    - python/burner_redis/__init__.py contains `from burner_redis.pubsub import PubSub`
    - python/burner_redis/__init__.py contains `BurnerRedis.pubsub = _pubsub`
    - python/burner_redis/__init__.py contains `"PubSub"` in __all__
    - python/burner_redis/pipeline.py contains `def publish(self, channel, message)`
    - src/scripting.rs contains `"PUBLISH" =>`
    - src/scripting.rs dispatch_command has `pubsub_tx: Option<&broadcast::Sender<PubSubMessage>>` parameter
    - `cargo check` succeeds with no errors
  </acceptance_criteria>
  <done>PubSub class exists with full redis-py-compatible API, monkey-patched onto BurnerRedis, Pipeline has publish(), Lua PUBLISH dispatches through broadcast sender, cargo check passes</done>
</task>

<task type="auto" tdd="true">
  <name>Task 2: Comprehensive pub/sub test suite</name>
  <files>tests/test_pubsub.py</files>
  <read_first>
    - tests/test_strings.py (first 40 lines -- test pattern, conftest usage, pytest-asyncio markers)
    - tests/test_pipeline.py (first 60 lines -- pipeline test patterns)
    - tests/test_scripting.py (first 60 lines -- Lua script test patterns)
    - tests/conftest.py (fixtures)
    - python/burner_redis/pubsub.py (the PubSub class being tested)
  </read_first>
  <behavior>
    - test_pubsub_factory: client.pubsub() returns PubSub instance with client reference (D-04)
    - test_subscribe_and_publish: subscribe to channel, publish message, get_message returns dict with type="message", channel=b"...", data=b"..." (D-02, D-09)
    - test_publish_returns_subscriber_count: publish() returns number of subscribers that will receive (D-02)
    - test_unsubscribe: subscribe then unsubscribe, verify no more messages received (D-02)
    - test_psubscribe_pattern: psubscribe to "foo.*", publish to "foo.bar", get pmessage with pattern field (D-02)
    - test_punsubscribe: psubscribe then punsubscribe, verify no more messages (D-02)
    - test_pubsub_channels: PUBSUB CHANNELS returns active channels (D-02)
    - test_pubsub_numsub: PUBSUB NUMSUB returns subscriber counts per channel (D-02)
    - test_pubsub_numpat: PUBSUB NUMPAT returns active pattern count (D-02)
    - test_handler_callback: subscribe with handler, verify handler called with message dict (D-05)
    - test_async_handler_callback: subscribe with async handler, verify await works (D-05)
    - test_ignore_subscribe_messages: PubSub(ignore_subscribe_messages=True) filters confirmations (D-08)
    - test_listen_generator: async for msg in pubsub.listen() yields messages (D-09)
    - test_get_message_polling: get_message(timeout=0.0) returns None when no message (D-09)
    - test_get_message_with_timeout: get_message(timeout=0.5) waits then returns None or message (D-09)
    - test_pipeline_publish: pipeline.publish(channel, message) executes in batch (D-12)
    - test_lua_publish: redis.call('PUBLISH', channel, msg) works in Lua scripts (D-11)
    - test_message_format: message dicts have exact keys type/pattern/channel/data (D-09)
    - test_multiple_subscribers: two PubSub instances both receive published message (D-10)
    - test_subscribe_confirmation_message: subscribe produces type="subscribe" confirmation (D-08)
    - test_close_unsubscribes: close()/aclose() removes all subscriptions (D-04)
  </behavior>
  <action>
Create `tests/test_pubsub.py` with all tests listed in the behavior block.

Test file structure:
```python
"""Tests for pub/sub support (Phase 10)."""
import asyncio
import pytest
from burner_redis import BurnerRedis

# Use pytest.fixture for client instance (matching conftest.py pattern)
# Each test gets a fresh BurnerRedis instance

@pytest.fixture
async def client():
    return BurnerRedis()
```

For each test:
1. Create a BurnerRedis client
2. Create PubSub via `client.pubsub()` (or `client.pubsub(ignore_subscribe_messages=True)`)
3. Subscribe to channels/patterns
4. Publish messages
5. Use `get_message(timeout=1.0)` to receive (with timeout to prevent hanging)
6. Assert message format and content

**Key testing patterns:**
- Use `ignore_subscribe_messages=True` in most tests to skip confirmation messages, except for tests specifically verifying confirmation behavior.
- Use `await asyncio.sleep(0.1)` between publish and get_message to allow the Tokio background task to forward messages through the broadcast channel.
- For `test_listen_generator`, use `asyncio.wait_for` with a timeout to prevent infinite blocking.
- For `test_lua_publish`, use `await client.eval("return redis.call('PUBLISH', KEYS[1], ARGV[1])", 1, b'channel', b'message')`.
- For `test_pipeline_publish`, create pipeline, add publish, execute, verify subscriber received message.
- For `test_multiple_subscribers`, create two separate PubSub instances, subscribe both, publish once, verify both receive.
- For `test_run_in_thread`, use `pubsub.run_in_thread()`, publish a message, sleep briefly, stop the thread, verify it exits cleanly. Use a handler callback to capture the received message.

**All tests must use `@pytest.mark.asyncio` decorator** consistent with the existing test suite.
  </action>
  <verify>
    <automated>cd /Users/alexander/dev/prefectlabs/burner-redis && uv run maturin develop 2>&1 | tail -3 && uv run pytest tests/test_pubsub.py -x -v 2>&1 | tail -30</automated>
  </verify>
  <acceptance_criteria>
    - tests/test_pubsub.py exists and contains at least 20 test functions
    - tests/test_pubsub.py contains `test_pubsub_factory`
    - tests/test_pubsub.py contains `test_subscribe_and_publish`
    - tests/test_pubsub.py contains `test_psubscribe_pattern`
    - tests/test_pubsub.py contains `test_handler_callback`
    - tests/test_pubsub.py contains `test_ignore_subscribe_messages`
    - tests/test_pubsub.py contains `test_listen_generator`
    - tests/test_pubsub.py contains `test_pipeline_publish`
    - tests/test_pubsub.py contains `test_lua_publish`
    - tests/test_pubsub.py contains `test_multiple_subscribers`
    - tests/test_pubsub.py contains `test_run_in_thread`
    - `uv run pytest tests/test_pubsub.py -x` exits with code 0 (all tests pass)
  </acceptance_criteria>
  <done>All pub/sub tests pass, covering all 13 CONTEXT.md decisions with at least 20 test functions. Full suite green.</done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|----------|-------------|
| Python PubSub -> Rust Store | Channel names and message data flow from Python PubSub class to Rust Store pub/sub methods |
| Lua script -> broadcast sender | PUBLISH in Lua scripts writes to broadcast channel while data write lock is held |
| Broadcast -> asyncio.Queue | Rust background task pushes message dicts into Python asyncio.Queue via GIL acquisition |

## STRIDE Threat Register

| Threat ID | Category | Component | Disposition | Mitigation Plan |
|-----------|----------|-----------|-------------|-----------------|
| T-10-05 | Denial of Service | PubSub._queue unbounded growth | accept | asyncio.Queue is unbounded by default. In embedded single-process use, the publisher and consumer are co-located. If the consumer stalls, the publisher also stalls (same process). No external attacker can flood messages without code execution. |
| T-10-06 | Denial of Service | run_in_thread() event loop leak | mitigate | PubSubWorkerThread wraps asyncio loop in try/finally to ensure loop.close(). Thread is daemon=True so process exit is not blocked. stop() method uses threading.Event for clean shutdown. |
| T-10-07 | Elevation of Privilege | Lua PUBLISH bypasses subscription checks | accept | PUBLISH in Lua dispatches to broadcast sender which fans out to ALL receivers. Each Python PubSub filters locally. This is correct Redis behavior -- any client can PUBLISH to any channel. No privilege boundary exists in embedded use. |
| T-10-08 | Tampering | Broadcast message format manipulation | mitigate | PubSubMessage struct is created only by Store.publish() and dispatch_command PUBLISH. Python PubSub class only reads message dicts -- never creates fake broadcast messages. The _filter_message method validates message type before processing. |
</threat_model>

<verification>
- `cargo check` passes
- `uv run maturin develop` succeeds
- `uv run pytest tests/test_pubsub.py -x` -- all tests pass
- `uv run pytest tests/ -x` -- full suite still passes (no regressions)
- PubSub class supports all redis-py methods: subscribe, unsubscribe, psubscribe, punsubscribe, listen, get_message, handle_message, run_in_thread, close, aclose, reset
- PUBLISH works from direct call, pipeline, and Lua script
</verification>

<success_criteria>
- Python PubSub class fully implements redis-py async PubSub interface
- client.pubsub() factory works via monkey-patch
- Pipeline.publish() queues PUBLISH for batch execution
- redis.call('PUBLISH', ...) works in Lua scripts
- 20+ test functions cover all 13 CONTEXT.md decisions
- Full test suite passes with no regressions
</success_criteria>

<output>
After completion, create `.planning/phases/10-add-pub-sub-support-subscribe-unsubscribe-publish-psubscribe/10-02-SUMMARY.md`
</output>
