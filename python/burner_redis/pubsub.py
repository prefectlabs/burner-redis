"""PubSub class for Redis-compatible pub/sub messaging.

Provides redis-py compatible async PubSub API with subscribe/unsubscribe,
pattern matching, message handlers, and background thread processing.
"""
import asyncio
import inspect
import queue
import threading
import time


class PubSub:
    """Async pub/sub message handler mirroring redis-py's PubSub interface.

    Created via client.pubsub(). Manages channel/pattern subscriptions
    and message delivery through an internal thread-safe queue fed by a
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
        self._queue = queue.Queue()
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
            self._queue.put_nowait(msg)

    async def unsubscribe(self, *args):
        """Unsubscribe from one or more channels. If no args, unsubscribe from all."""
        if self._subscriber_id is None:
            self.channels.clear()  # defensive: clear local state even if no backend call needed
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
            self._queue.put_nowait(msg)

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
            self._queue.put_nowait(msg)

    async def punsubscribe(self, *args):
        """Unsubscribe from one or more patterns. If no args, unsubscribe from all."""
        if self._subscriber_id is None:
            self.patterns.clear()  # defensive: clear local state even if no backend call needed
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
            self._queue.put_nowait(msg)

    async def _queue_get(self, timeout):
        """Read one item from the internal thread-safe queue.

        For blocking reads, poll in short slices so external task cancellation
        still propagates promptly and no worker thread is left parked forever.
        """
        if timeout == 0.0:
            try:
                return self._queue.get_nowait()
            except queue.Empty:
                return None

        deadline = None if timeout is None else time.monotonic() + timeout
        poll_timeout = 0.1 if timeout is None else min(timeout, 0.1)

        while True:
            try:
                return await asyncio.to_thread(self._queue.get, True, poll_timeout)
            except queue.Empty:
                if deadline is None:
                    continue
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    return None
                poll_timeout = min(remaining, 0.1)

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
                raw = await self._queue_get(timeout)
                if raw is None:
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
                if inspect.iscoroutinefunction(handler):
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
        """Async close -- unsubscribe from all channels and patterns, and
        signal the Rust-side listener task to exit so its captured references
        to this PubSub's event loop and queue are released.

        This awaits the Rust listener's actual exit before returning. Without
        that join, the background Tokio task can outlive the function-scoped
        asyncio loop it captured and poison later worker-shutdown tests,
        especially on Windows.
        """
        if self.channels:
            await self.unsubscribe()
        if self.patterns:
            await self.punsubscribe()
        if self._listener_started and self._subscriber_id is not None:
            try:
                await self._client._stop_subscriber_listener(self._subscriber_id)
            finally:
                self._listener_started = False

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
