"""Pipeline class for batched command execution.

Provides redis-py compatible Pipeline API that buffers commands
and executes them sequentially against a BurnerRedis instance.
"""
from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, NoReturn

if TYPE_CHECKING:
    from types import TracebackType

    from burner_redis._burner_redis import (
        BurnerRedis,
        EncodableT,
        KeyT,
        ScoreT,
    )


def _coerce_value(value: EncodableT) -> str | bytes | memoryview:
    """Coerce a value to str or bytes, matching redis-py's Encoder.encode() behavior.

    Mirror of burner_redis._coerce_value — duplicated here to avoid a circular
    import (burner_redis.__init__ imports Pipeline). Pipeline list/string stubs
    must apply the same coercion as the monkey-patched client methods so that
    `pipe.lpush("k", 42).execute()` matches `r.lpush("k", 42)` (H-01).

    Accepts: bytes, memoryview, int, float, str.
    Rejects: bool (redis-py rejects bools with TypeError since bool is subclass of int).
    """
    if isinstance(value, (bytes, memoryview)):
        return value
    if isinstance(value, bool):
        raise TypeError(
            "Invalid input of type: 'bool'. "
            "Convert to a bytes, string, int or float first."
        )
    if isinstance(value, (int, float)):
        return repr(value).encode()
    if isinstance(value, str):
        return value
    raise TypeError(
        f"Invalid input of type: '{type(value).__name__}'. "
        "Convert to a bytes, string, int or float first."
    )


class Pipeline:
    """Buffers commands and executes them as a batch.

    Created via client.pipeline(). Commands are queued as
    (method_name, args, kwargs) tuples and executed sequentially
    on execute(), returning results in command order.
    """

    def __init__(self, client: BurnerRedis) -> None:
        self._client = client
        self._commands: list[tuple[str, tuple[Any, ...], dict[str, Any]]] = []

    async def execute(self, raise_on_error: bool = True) -> list[Any]:
        """Execute all queued commands and return results in order.

        Fast path (no blocking commands in the queue): uses native Rust
        pipeline execution — a single Python-to-Rust boundary crossing
        executes all commands synchronously in a tight loop, eliminating
        per-command async overhead (quick task 260415-an2).

        Slow path (at least one of brpop/blpop/blmove in the queue): iterate
        commands in Python and await each one individually on self._client.
        Blocking commands respect their per-command timeouts; subsequent
        commands execute after the block resolves. Pipeline semantics in
        redis-py are sequential — blocking one command really does block
        the rest (D-15/D-16).

        Matches redis-py behavior: all commands execute regardless of
        individual failures. When raise_on_error is True (default, matching
        redis-py), the first Exception in the results list is raised after
        execution completes. When False, Exception objects are returned
        inline at the position of the failed command, preserving per-command
        error inspection.
        """
        if not self._commands:
            return []

        blocking_cmds = {"brpop", "blpop", "blmove"}
        has_blocking = any(c[0] in blocking_cmds for c in self._commands)

        if not has_blocking:
            # FAST PATH: single-boundary Rust dispatch (preserves 260415-an2 perf).
            results = await self._client.execute_pipeline(self._commands)
            self._commands = []
            results = list(results)
            if raise_on_error:
                for r in results:
                    if isinstance(r, Exception):
                        raise r
            return results

        # SLOW PATH: iterate + await individual awaitables on the client.
        # Keeps Rust execute_pipeline purely synchronous; no Python-coroutine
        # awaiting from inside a single Rust future.
        #
        # P2-01: mirror fast-path semantics — capture per-command exceptions
        # into the results list, then raise the first one AFTER all commands
        # have been attempted (when raise_on_error=True). Previously the slow
        # path raised on the first failure and skipped subsequent commands,
        # which diverged from redis-py / fast-path behavior.
        results: list[Any] = []
        commands = self._commands
        self._commands = []
        for (method_name, args, kwargs) in commands:
            try:
                method = getattr(self._client, method_name)
                result = await method(*args, **kwargs)
                results.append(result)
            except Exception as e:
                results.append(e)
        if raise_on_error:
            for r in results:
                if isinstance(r, Exception):
                    raise r
        return results

    async def __aenter__(self) -> Pipeline:
        return self

    async def __aexit__(
        self,
        exc_type: type[BaseException] | None,
        exc_val: BaseException | None,
        exc_tb: TracebackType | None,
    ) -> bool:
        if exc_type is None:
            await self.execute()
        return False

    # ---- String Commands ----

    def set(
        self,
        name: KeyT,
        value: EncodableT,
        ex: int | None = None,
        px: int | None = None,
        nx: bool = False,
        xx: bool = False,
    ) -> Pipeline:
        # H-01: apply value coercion at buffer time so the pipeline matches
        # the monkey-patched client (`r.set` runs `_coerced_set` first).
        coerced = _coerce_value(value)
        self._commands.append(("set", (name, coerced), {"ex": ex, "px": px, "nx": nx, "xx": xx}))
        return self

    def get(self, name: KeyT) -> Pipeline:
        self._commands.append(("get", (name,), {}))
        return self

    def delete(self, *names: KeyT) -> Pipeline:
        self._commands.append(("delete", names, {}))
        return self

    def exists(self, *names: KeyT) -> Pipeline:
        self._commands.append(("exists", names, {}))
        return self

    # ---- Hash Commands ----

    def hset(
        self,
        name: KeyT,
        key: KeyT | None = None,
        value: EncodableT | None = None,
        mapping: Mapping[KeyT, EncodableT] | None = None,
    ) -> Pipeline:
        self._commands.append(("hset", (name,), {"key": key, "value": value, "mapping": mapping}))
        return self

    def hget(self, name: KeyT, key: KeyT) -> Pipeline:
        self._commands.append(("hget", (name, key), {}))
        return self

    def hdel(self, name: KeyT, *keys: KeyT) -> Pipeline:
        self._commands.append(("hdel", (name, *keys), {}))
        return self

    def hvals(self, name: KeyT) -> Pipeline:
        self._commands.append(("hvals", (name,), {}))
        return self

    # ---- Set Commands ----

    def sadd(self, name: KeyT, *values: EncodableT) -> Pipeline:
        self._commands.append(("sadd", (name, *values), {}))
        return self

    def smembers(self, name: KeyT) -> Pipeline:
        self._commands.append(("smembers", (name,), {}))
        return self

    def sismember(self, name: KeyT, value: EncodableT) -> Pipeline:
        self._commands.append(("sismember", (name, value), {}))
        return self

    def srem(self, name: KeyT, *values: EncodableT) -> Pipeline:
        self._commands.append(("srem", (name, *values), {}))
        return self

    # ---- Sorted Set Commands ----

    def zadd(
        self,
        name: KeyT,
        mapping: Mapping[KeyT, float],
        nx: bool = False,
        xx: bool = False,
        gt: bool = False,
        lt: bool = False,
        ch: bool = False,
    ) -> Pipeline:
        self._commands.append(("zadd", (name, mapping), {"nx": nx, "xx": xx, "gt": gt, "lt": lt, "ch": ch}))
        return self

    def zrem(self, name: KeyT, *values: EncodableT) -> Pipeline:
        self._commands.append(("zrem", (name, *values), {}))
        return self

    def zrange(
        self, name: KeyT, start: int, end: int, withscores: bool = False
    ) -> Pipeline:
        self._commands.append(("zrange", (name, start, end), {"withscores": withscores}))
        return self

    def zrangebyscore(
        self,
        name: KeyT,
        min: ScoreT,
        max: ScoreT,
        withscores: bool = False,
    ) -> Pipeline:
        self._commands.append(("zrangebyscore", (name, min, max), {"withscores": withscores}))
        return self

    def zrangestore(
        self, dest: KeyT, name: KeyT, start: int, end: int
    ) -> Pipeline:
        self._commands.append(("zrangestore", (dest, name, start, end), {}))
        return self

    def zremrangebyscore(
        self, name: KeyT, min: ScoreT, max: ScoreT
    ) -> Pipeline:
        self._commands.append(("zremrangebyscore", (name, min, max), {}))
        return self

    # ---- List Commands ----

    def lpush(self, name: KeyT, *values: EncodableT) -> Pipeline:
        # H-01: per-value coercion mirrors the monkey-patched `_coerced_lpush`.
        coerced = tuple(_coerce_value(v) for v in values)
        self._commands.append(("lpush", (name, *coerced), {}))
        return self

    def rpush(self, name: KeyT, *values: EncodableT) -> Pipeline:
        # H-01: per-value coercion mirrors the monkey-patched `_coerced_rpush`.
        coerced = tuple(_coerce_value(v) for v in values)
        self._commands.append(("rpush", (name, *coerced), {}))
        return self

    def lpop(self, name: KeyT, count: int | None = None) -> Pipeline:
        self._commands.append(("lpop", (name,), {"count": count}))
        return self

    def rpop(self, name: KeyT, count: int | None = None) -> Pipeline:
        self._commands.append(("rpop", (name,), {"count": count}))
        return self

    def lrange(self, name: KeyT, start: int, end: int) -> Pipeline:
        self._commands.append(("lrange", (name, start, end), {}))
        return self

    def llen(self, name: KeyT) -> Pipeline:
        self._commands.append(("llen", (name,), {}))
        return self

    def lindex(self, name: KeyT, index: int) -> Pipeline:
        self._commands.append(("lindex", (name, index), {}))
        return self

    def linsert(
        self,
        name: KeyT,
        where: str,
        refvalue: EncodableT,
        value: EncodableT,
    ) -> Pipeline:
        # H-01: coerce inserted `value`.
        # P2-06: also coerce `refvalue` — redis-py encodes every command
        # argument including the pivot, so numeric pivots are legal.
        self._commands.append(
            ("linsert", (name, where, _coerce_value(refvalue), _coerce_value(value)), {})
        )
        return self

    def lrem(self, name: KeyT, count: int, value: EncodableT) -> Pipeline:
        # P2-07: coerce `value` — redis-py encodes ints/floats for LREM
        # values just like LPUSH/LSET. Mirror of `_coerced_lrem`.
        self._commands.append(("lrem", (name, count, _coerce_value(value)), {}))
        return self

    def lset(self, name: KeyT, index: int, value: EncodableT) -> Pipeline:
        # H-01: coerce inserted value (mirror of `_coerced_lset`).
        self._commands.append(("lset", (name, index, _coerce_value(value)), {}))
        return self

    def ltrim(self, name: KeyT, start: int, end: int) -> Pipeline:
        self._commands.append(("ltrim", (name, start, end), {}))
        return self

    def lmove(
        self,
        first_list: KeyT,
        second_list: KeyT,
        src: str = "LEFT",
        dest: str = "RIGHT",
    ) -> Pipeline:
        self._commands.append(("lmove", (first_list, second_list), {"src": src, "dest": dest}))
        return self

    def rpoplpush(self, src: KeyT, dst: KeyT) -> Pipeline:
        self._commands.append(("rpoplpush", (src, dst), {}))
        return self

    def blpop(
        self, keys: KeyT | list[KeyT], timeout: float = 0
    ) -> Pipeline:
        self._commands.append(("blpop", (keys,), {"timeout": timeout}))
        return self

    def brpop(
        self, keys: KeyT | list[KeyT], timeout: float = 0
    ) -> Pipeline:
        self._commands.append(("brpop", (keys,), {"timeout": timeout}))
        return self

    def blmove(
        self,
        first_list: KeyT,
        second_list: KeyT,
        timeout: float,
        src: str = "LEFT",
        dest: str = "RIGHT",
    ) -> Pipeline:
        self._commands.append(("blmove", (first_list, second_list, timeout), {"src": src, "dest": dest}))
        return self

    # ---- Stream Commands ----

    def xadd(
        self,
        name: KeyT,
        fields: Mapping[KeyT, EncodableT],
        id: KeyT = "*",
        maxlen: int | None = None,
        minid: KeyT | None = None,
    ) -> Pipeline:
        self._commands.append(("xadd", (name, fields), {"id": id, "maxlen": maxlen, "minid": minid}))
        return self

    def xread(
        self,
        streams: Mapping[KeyT, KeyT],
        count: int | None = None,
        block: int | None = None,
    ) -> Pipeline:
        self._commands.append(("xread", (streams,), {"count": count, "block": block}))
        return self

    def xlen(self, name: KeyT) -> Pipeline:
        self._commands.append(("xlen", (name,), {}))
        return self

    def xtrim(
        self,
        name: KeyT,
        maxlen: int | None = None,
        minid: KeyT | None = None,
        approximate: bool = True,
    ) -> Pipeline:
        self._commands.append(("xtrim", (name,), {"maxlen": maxlen, "minid": minid, "approximate": approximate}))
        return self

    # ---- Consumer Group Commands ----

    def xgroup_create(
        self,
        name: KeyT,
        groupname: KeyT,
        id: KeyT = "$",
        mkstream: bool = False,
    ) -> Pipeline:
        self._commands.append(("xgroup_create", (name, groupname), {"id": id, "mkstream": mkstream}))
        return self

    def xgroup_destroy(self, name: KeyT, groupname: KeyT) -> Pipeline:
        self._commands.append(("xgroup_destroy", (name, groupname), {}))
        return self

    def xreadgroup(
        self,
        groupname: KeyT,
        consumername: KeyT,
        streams: Mapping[KeyT, KeyT],
        count: int | None = None,
        block: int | None = None,
        noack: bool = False,
    ) -> Pipeline:
        self._commands.append(("xreadgroup", (groupname, consumername, streams), {"count": count, "block": block, "noack": noack}))
        return self

    def xack(self, name: KeyT, groupname: KeyT, *ids: KeyT) -> Pipeline:
        self._commands.append(("xack", (name, groupname, *ids), {}))
        return self

    def xautoclaim(
        self,
        name: KeyT,
        groupname: KeyT,
        consumername: KeyT,
        min_idle_time: int,
        start_id: KeyT = "0-0",
        count: int | None = None,
    ) -> Pipeline:
        self._commands.append(("xautoclaim", (name, groupname, consumername, min_idle_time), {"start_id": start_id, "count": count}))
        return self

    def xclaim(
        self,
        name: KeyT,
        groupname: KeyT,
        consumername: KeyT,
        min_idle_time: int,
        message_ids: list[KeyT],
        idle: int | None = None,
        time: int | None = None,
        retrycount: int | None = None,
        force: bool = False,
        justid: bool = False,
    ) -> Pipeline:
        self._commands.append(("xclaim", (name, groupname, consumername, min_idle_time, message_ids),
                              {"idle": idle, "time": time, "retrycount": retrycount,
                               "force": force, "justid": justid}))
        return self

    def xinfo_groups(self, name: KeyT) -> Pipeline:
        self._commands.append(("xinfo_groups", (name,), {}))
        return self

    def xinfo_consumers(self, name: KeyT, groupname: KeyT) -> Pipeline:
        self._commands.append(("xinfo_consumers", (name, groupname), {}))
        return self

    def xpending_range(
        self,
        name: KeyT,
        groupname: KeyT,
        min: KeyT = "-",
        max: KeyT = "+",
        count: int = 100,
        consumername: KeyT | None = None,
        idle: int | None = None,
    ) -> Pipeline:
        self._commands.append(("xpending_range", (name, groupname, min, max, count), {"consumername": consumername, "idle": idle}))
        return self

    # ---- Scripting Commands ----

    def eval(self, script: str, numkeys: int, *keys_and_args: KeyT) -> Pipeline:
        self._commands.append(("eval", (script, numkeys, *keys_and_args), {}))
        return self

    def evalsha(self, sha: str, numkeys: int, *keys_and_args: KeyT) -> Pipeline:
        self._commands.append(("evalsha", (sha, numkeys, *keys_and_args), {}))
        return self

    def script_load(self, script: str) -> Pipeline:
        self._commands.append(("script_load", (script,), {}))
        return self

    def script_exists(self, *args: str) -> Pipeline:
        self._commands.append(("script_exists", args, {}))
        return self

    # ---- Additional Hash Commands ----

    def hgetall(self, name: KeyT) -> Pipeline:
        self._commands.append(("hgetall", (name,), {}))
        return self

    def hexists(self, name: KeyT, key: KeyT) -> Pipeline:
        self._commands.append(("hexists", (name, key), {}))
        return self

    def hincrby(self, name: KeyT, key: KeyT, amount: int = 1) -> Pipeline:
        self._commands.append(("hincrby", (name, key), {"amount": amount}))
        return self

    # ---- Additional Sorted Set Commands ----

    def zcard(self, name: KeyT) -> Pipeline:
        self._commands.append(("zcard", (name,), {}))
        return self

    def zscore(self, name: KeyT, value: EncodableT) -> Pipeline:
        self._commands.append(("zscore", (name, value), {}))
        return self

    def zcount(self, name: KeyT, min: ScoreT, max: ScoreT) -> Pipeline:
        self._commands.append(("zcount", (name, min, max), {}))
        return self

    # ---- Key Commands ----

    def expire(self, name: KeyT, time: int) -> Pipeline:
        self._commands.append(("expire", (name, time), {}))
        return self

    # ---- Additional Stream Commands ----

    def xdel(self, name: KeyT, *ids: KeyT) -> Pipeline:
        self._commands.append(("xdel", (name, *ids), {}))
        return self

    def xrange(
        self,
        name: KeyT,
        min: KeyT = "-",
        max: KeyT = "+",
        count: int | None = None,
    ) -> Pipeline:
        self._commands.append(("xrange", (name,), {"min": min, "max": max, "count": count}))
        return self

    # ---- Pub/Sub Commands ----

    def publish(self, channel: KeyT, message: EncodableT) -> Pipeline:
        self._commands.append(("publish", (channel, message), {}))
        return self

    # ---- Key Enumeration Commands ----

    def keys(self, pattern: KeyT = "*") -> Pipeline:
        self._commands.append(("keys", (pattern,), {}))
        return self

    def ttl(self, name: KeyT) -> Pipeline:
        self._commands.append(("ttl", (name,), {}))
        return self

    def setex(self, name: KeyT, time: int, value: EncodableT) -> Pipeline:
        self._commands.append(("setex", (name, time, value), {}))
        return self

    def mget(self, *keys: KeyT) -> Pipeline:
        self._commands.append(("mget", keys, {}))
        return self

    def xpending(self, name: KeyT, groupname: KeyT) -> Pipeline:
        self._commands.append(("xpending", (name, groupname), {}))
        return self

    def scan_iter(
        self,
        match: KeyT | None = None,
        count: int | None = None,
        _type: str | None = None,
    ) -> NoReturn:
        raise NotImplementedError(
            "scan_iter is an async generator and cannot be used in a pipeline. "
            "Use scan_iter() directly on the client instead."
        )
