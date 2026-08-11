"""Type stubs for the compiled `burner_redis._burner_redis` extension.

This stub describes the `BurnerRedis` class as users see it after the
package's `__init__.py` finishes monkey-patching value-coercion wrappers
and helper methods (pipeline, lock, pubsub, register_script, scan_iter,
aclose/close, async-context-manager protocol) onto the Rust class.

Type checkers cannot track runtime reassignments on a PyO3 class, so the
unified surface lives here rather than being split across multiple files.
"""

from collections.abc import AsyncIterator, Awaitable, Mapping, Sequence
from typing import TYPE_CHECKING, Any, Literal, Self, TypeAlias, TypedDict, overload

from burner_redis.lock import Lock
from burner_redis.pipeline import Pipeline
from burner_redis.pubsub import PubSub

if TYPE_CHECKING:
    # `Script` lives in burner_redis/__init__.py — a real circular import at
    # runtime, but type checkers resolve TYPE_CHECKING imports without
    # executing them.
    from burner_redis import Script

# ---------------------------------------------------------------------------
# Input type aliases — chosen to align with the type domain in
# chrisguidry/docket#401, which is the canonical async-only consumer of
# this surface. Keep these in sync if the Rust bindings accept new shapes.
# ---------------------------------------------------------------------------

# Redis keys / channels / patterns — anything that the Rust binding can
# treat as a byte string.
KeyT: TypeAlias = str | bytes | memoryview

# Redis-encodable values. Matches the acceptance set of `_coerce_value` in
# `burner_redis/__init__.py`. Bool is a subclass of int but rejected at
# runtime to mirror redis-py's DataError.
EncodableT: TypeAlias = str | bytes | bytearray | memoryview | int | float

# Stream IDs — `*`, `$`, `0-0`, `<ms>-<seq>`, or numeric.
StreamIDT: TypeAlias = str | bytes | int

# A score for sorted-set ranges — float, or "-inf"/"+inf" string.
ScoreT: TypeAlias = float | str | bytes

# ---------------------------------------------------------------------------
# Stream return-shape aliases. burner-redis is committed to returning
# `bytes` (no decode_responses=True equivalent), so these collapse to the
# bytes-typed shapes.
# ---------------------------------------------------------------------------

RedisStreamID: TypeAlias = bytes
RedisMessageID: TypeAlias = bytes
RedisMessage: TypeAlias = dict[bytes, bytes]
RedisMessages: TypeAlias = list[tuple[RedisMessageID, RedisMessage]]
RedisStream: TypeAlias = tuple[RedisStreamID, RedisMessages]
RedisReadGroupResponse: TypeAlias = list[RedisStream]


class RedisStreamPendingMessage(TypedDict):
    """One entry returned by XPENDING ... RANGE."""

    message_id: bytes
    consumer: bytes
    time_since_delivered: int
    times_delivered: int

class ResolvedFuture:
    """Awaitable wrapper around an already-computed result.

    Returned by every non-blocking command on `BurnerRedis`. From Python's
    perspective these behave as coroutines that resolve immediately on first
    await, with no Tokio scheduling.
    """

    def __await__(self) -> Self: ...
    def __iter__(self) -> Self: ...
    def __next__(self) -> Any: ...

class BurnerRedis:
    """Embedded, in-process Redis-compatible client.

    Drop-in async replacement for `redis.asyncio.Redis` that runs inside the
    host process — no external server required.
    """

    def __init__(self, persistence_path: str | None = None) -> None: ...

    @property
    def persistence_path(self) -> str | None: ...

    # ---- Persistence ----
    def save(self, path: str | None = None) -> Awaitable[bool]: ...
    def _save_sync(self) -> bool: ...

    # ---- Strings ----
    def set(
        self,
        name: KeyT,
        value: EncodableT,
        ex: int | None = None,
        px: int | None = None,
        nx: bool = False,
        xx: bool = False,
    ) -> Awaitable[bool | None]: ...
    def setex(self, name: KeyT, time: int, value: EncodableT) -> Awaitable[bool]: ...
    def get(self, name: KeyT) -> Awaitable[bytes | None]: ...
    def mget(self, *keys: KeyT) -> Awaitable[list[bytes | None]]: ...

    # ---- Keys ----
    def delete(self, *names: KeyT) -> Awaitable[int]: ...
    def exists(self, *names: KeyT) -> Awaitable[int]: ...
    def expire(self, name: KeyT, time: int) -> Awaitable[bool]: ...
    def ttl(self, name: KeyT) -> Awaitable[int]: ...
    def keys(self, pattern: KeyT = "*") -> Awaitable[list[bytes]]: ...
    def scan_iter(
        self,
        match: KeyT | None = None,
        count: int | None = None,
        _type: str | None = None,
    ) -> AsyncIterator[bytes]: ...

    # ---- Hashes ----
    def hset(
        self,
        name: KeyT,
        key: KeyT | None = None,
        value: EncodableT | None = None,
        mapping: Mapping[KeyT, EncodableT] | None = None,
    ) -> Awaitable[int]: ...
    def hget(self, name: KeyT, key: KeyT) -> Awaitable[bytes | None]: ...
    def hdel(self, name: KeyT, *keys: KeyT) -> Awaitable[int]: ...
    def hexists(self, name: KeyT, key: KeyT) -> Awaitable[bool]: ...
    def hvals(self, name: KeyT) -> Awaitable[list[bytes]]: ...
    def hgetall(self, name: KeyT) -> Awaitable[dict[bytes, bytes]]: ...
    def hincrby(self, name: KeyT, key: KeyT, amount: int = 1) -> Awaitable[int]: ...

    # ---- Sets ----
    def sadd(self, name: KeyT, *values: EncodableT) -> Awaitable[int]: ...
    def smembers(self, name: KeyT) -> Awaitable[set[bytes]]: ...
    def sismember(self, name: KeyT, value: EncodableT) -> Awaitable[bool]: ...
    def srem(self, name: KeyT, *values: EncodableT) -> Awaitable[int]: ...

    # ---- Sorted sets ----
    def zadd(
        self,
        name: KeyT,
        mapping: Mapping[EncodableT, float],
        nx: bool = False,
        xx: bool = False,
        gt: bool = False,
        lt: bool = False,
        ch: bool = False,
    ) -> Awaitable[int]: ...
    def zrem(self, name: KeyT, *values: EncodableT) -> Awaitable[int]: ...
    @overload
    def zrange(
        self,
        name: KeyT,
        start: int,
        end: int,
        withscores: Literal[False] = False,
    ) -> Awaitable[list[bytes]]: ...
    @overload
    def zrange(
        self,
        name: KeyT,
        start: int,
        end: int,
        *,
        withscores: Literal[True],
    ) -> Awaitable[list[tuple[bytes, float]]]: ...
    @overload
    def zrangebyscore(
        self,
        name: KeyT,
        min: ScoreT,
        max: ScoreT,
        withscores: Literal[False] = False,
    ) -> Awaitable[list[bytes]]: ...
    @overload
    def zrangebyscore(
        self,
        name: KeyT,
        min: ScoreT,
        max: ScoreT,
        *,
        withscores: Literal[True],
    ) -> Awaitable[list[tuple[bytes, float]]]: ...
    def zrangestore(
        self, dest: KeyT, name: KeyT, start: int, end: int
    ) -> Awaitable[int]: ...
    def zremrangebyscore(
        self, name: KeyT, min: ScoreT, max: ScoreT
    ) -> Awaitable[int]: ...
    def zcard(self, name: KeyT) -> Awaitable[int]: ...
    def zscore(self, name: KeyT, value: EncodableT) -> Awaitable[float | None]: ...
    def zcount(self, name: KeyT, min: ScoreT, max: ScoreT) -> Awaitable[int]: ...

    # ---- Lists ----
    def lpush(self, name: KeyT, *values: EncodableT) -> Awaitable[int]: ...
    def rpush(self, name: KeyT, *values: EncodableT) -> Awaitable[int]: ...
    @overload
    def lpop(self, name: KeyT) -> Awaitable[bytes | None]: ...
    @overload
    def lpop(self, name: KeyT, count: int) -> Awaitable[list[bytes] | None]: ...
    @overload
    def rpop(self, name: KeyT) -> Awaitable[bytes | None]: ...
    @overload
    def rpop(self, name: KeyT, count: int) -> Awaitable[list[bytes] | None]: ...
    def lrange(self, name: KeyT, start: int, end: int) -> Awaitable[list[bytes]]: ...
    def llen(self, name: KeyT) -> Awaitable[int]: ...
    def lindex(self, name: KeyT, index: int) -> Awaitable[bytes | None]: ...
    def linsert(
        self,
        name: KeyT,
        where: str,
        refvalue: EncodableT,
        value: EncodableT,
    ) -> Awaitable[int]: ...
    def lrem(self, name: KeyT, count: int, value: EncodableT) -> Awaitable[int]: ...
    def lset(self, name: KeyT, index: int, value: EncodableT) -> Awaitable[bool]: ...
    def ltrim(self, name: KeyT, start: int, end: int) -> Awaitable[bool]: ...
    def lmove(
        self,
        first_list: KeyT,
        second_list: KeyT,
        src: str = "LEFT",
        dest: str = "RIGHT",
    ) -> Awaitable[bytes | None]: ...
    def rpoplpush(self, src: KeyT, dst: KeyT) -> Awaitable[bytes | None]: ...

    # ---- Blocking list commands ----
    # These wrappers in __init__.py defer the underlying Rust future until
    # awaited so they're true coroutines (asyncio.create_task accepts them).
    async def blpop(
        self,
        keys: KeyT | list[KeyT],
        timeout: float | None = None,
    ) -> tuple[bytes, bytes] | None: ...
    async def brpop(
        self,
        keys: KeyT | list[KeyT],
        timeout: float | None = None,
    ) -> tuple[bytes, bytes] | None: ...
    async def blmove(
        self,
        first_list: KeyT,
        second_list: KeyT,
        timeout: float,
        src: str = "LEFT",
        dest: str = "RIGHT",
    ) -> bytes | None: ...

    # ---- Streams ----
    def xadd(
        self,
        name: KeyT,
        fields: Mapping[KeyT, EncodableT],
        id: StreamIDT = "*",
        maxlen: int | None = None,
        minid: StreamIDT | None = None,
    ) -> Awaitable[RedisStreamID]: ...
    def xread(
        self,
        streams: Mapping[KeyT, StreamIDT],
        count: int | None = None,
        block: int | None = None,
    ) -> Awaitable[RedisReadGroupResponse | None]: ...
    def xlen(self, name: KeyT) -> Awaitable[int]: ...
    def xtrim(
        self,
        name: KeyT,
        maxlen: int | None = None,
        minid: StreamIDT | None = None,
        approximate: bool = True,
    ) -> Awaitable[int]: ...
    def xdel(self, name: KeyT, *ids: StreamIDT) -> Awaitable[int]: ...
    def xrange(
        self,
        name: KeyT,
        min: StreamIDT = "-",
        max: StreamIDT = "+",
        count: int | None = None,
    ) -> Awaitable[RedisMessages]: ...

    # ---- Stream consumer groups ----
    def xgroup_create(
        self,
        name: KeyT,
        groupname: KeyT,
        id: StreamIDT = "$",
        mkstream: bool = False,
    ) -> Awaitable[bool]: ...
    def xgroup_destroy(self, name: KeyT, groupname: KeyT) -> Awaitable[int]: ...
    def xreadgroup(
        self,
        groupname: KeyT,
        consumername: KeyT,
        streams: Mapping[KeyT, StreamIDT],
        count: int | None = None,
        block: int | None = None,
        noack: bool = False,
    ) -> Awaitable[RedisReadGroupResponse]: ...
    def xack(self, name: KeyT, groupname: KeyT, *ids: StreamIDT) -> Awaitable[int]: ...
    def xautoclaim(
        self,
        name: KeyT,
        groupname: KeyT,
        consumername: KeyT,
        min_idle_time: int,
        start_id: StreamIDT = "0-0",
        count: int | None = None,
    ) -> Awaitable[tuple[RedisMessageID, RedisMessages, list[RedisMessageID]]]: ...
    def xclaim(
        self,
        name: KeyT,
        groupname: KeyT,
        consumername: KeyT,
        min_idle_time: int,
        message_ids: Sequence[StreamIDT],
        idle: int | None = None,
        time: int | None = None,
        retrycount: int | None = None,
        force: bool = False,
        justid: bool = False,
    ) -> Awaitable[list[tuple[RedisMessageID, RedisMessage] | RedisMessageID]]: ...
    def xinfo_stream(self, name: KeyT) -> Awaitable[dict[str, Any]]: ...
    def xinfo_groups(self, name: KeyT) -> Awaitable[list[dict[str, Any]]]: ...
    def xinfo_consumers(
        self, name: KeyT, groupname: KeyT
    ) -> Awaitable[list[dict[str, Any]]]: ...
    def xpending(self, name: KeyT, groupname: KeyT) -> Awaitable[dict[str, Any]]: ...
    def xpending_range(
        self,
        name: KeyT,
        groupname: KeyT,
        min: StreamIDT = "-",
        max: StreamIDT = "+",
        count: int = 100,
        consumername: KeyT | None = None,
        idle: int | None = None,
    ) -> Awaitable[list[RedisStreamPendingMessage]]: ...

    # ---- Scripting ----
    def eval(
        self, script: str, numkeys: int, *keys_and_args: KeyT
    ) -> Awaitable[Any]: ...
    def evalsha(
        self, sha: str, numkeys: int, *keys_and_args: KeyT
    ) -> Awaitable[Any]: ...
    def script_load(self, script: str) -> Awaitable[str]: ...
    def script_exists(self, *args: str) -> Awaitable[list[bool]]: ...
    def register_script(self, script: str | bytes) -> "Script": ...

    # ---- Pub/Sub ----
    def publish(self, channel: KeyT, message: EncodableT) -> Awaitable[int]: ...
    def subscribe_channels(
        self, subscriber_id: int, channels: list[bytes]
    ) -> Awaitable[list[tuple[bytes, int]]]: ...
    def unsubscribe_channels(
        self, subscriber_id: int, channels: list[bytes]
    ) -> Awaitable[list[tuple[bytes, int]]]: ...
    def psubscribe_patterns(
        self, subscriber_id: int, patterns: list[bytes]
    ) -> Awaitable[list[tuple[bytes, int]]]: ...
    def punsubscribe_patterns(
        self, subscriber_id: int, patterns: list[bytes]
    ) -> Awaitable[list[tuple[bytes, int]]]: ...
    def pubsub_channels(self, pattern: KeyT | None = None) -> Awaitable[list[bytes]]: ...
    def pubsub_numsub(
        self, channels: list[bytes]
    ) -> Awaitable[list[tuple[bytes, int]]]: ...
    def pubsub_numpat(self) -> Awaitable[int]: ...

    # ---- Pipeline (server-side batched execution) ----
    # `execute_pipeline` is the low-level entry point used by Pipeline.execute.
    # End users should call `pipeline()` instead.
    def execute_pipeline(
        self, commands: list[tuple[str, tuple[Any, ...], dict[str, Any]]]
    ) -> Awaitable[list[Any]]: ...

    # ---- Helper factories (added by burner_redis/__init__.py) ----
    def pipeline(self) -> Pipeline: ...
    def lock(
        self,
        name: KeyT,
        timeout: float | None = None,
        sleep: float = 0.1,
        blocking: bool = True,
        blocking_timeout: float | None = None,
    ) -> Lock: ...
    def pubsub(self, ignore_subscribe_messages: bool = False) -> PubSub: ...

    # ---- Lifecycle ----
    async def aclose(self) -> None: ...
    async def close(self) -> None: ...
    async def __aenter__(self) -> Self: ...
    async def __aexit__(self, *args: Any) -> None: ...

    # ---- Pub/Sub internals (consumed by burner_redis.pubsub.PubSub) ----
    def _new_subscriber(self) -> int: ...
    def _stream_last_id(self, key: KeyT) -> bytes | None: ...
    def _subscribe_listener(
        self, subscriber_id: int, queue: Any
    ) -> Awaitable[int]: ...
    def _stop_subscriber_listener(self, subscriber_id: int) -> Awaitable[None]: ...
    def _aclose(self) -> Awaitable[None]: ...
    def _close(self) -> Awaitable[None]: ...

