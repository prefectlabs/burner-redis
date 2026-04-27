"""Pipeline class for batched command execution.

Provides redis-py compatible Pipeline API that buffers commands
and executes them sequentially against a BurnerRedis instance.
"""


def _coerce_value(value):
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

    def __init__(self, client):
        self._client = client
        self._commands = []

    async def execute(self, raise_on_error: bool = True) -> list:
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
        results: list = []
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

    async def __aenter__(self):
        return self

    async def __aexit__(self, exc_type, exc_val, exc_tb):
        if exc_type is None:
            await self.execute()
        return False

    # ---- String Commands ----

    def set(self, name, value, ex=None, px=None, nx=False, xx=False):
        # H-01: apply value coercion at buffer time so the pipeline matches
        # the monkey-patched client (`r.set` runs `_coerced_set` first).
        coerced = _coerce_value(value)
        self._commands.append(("set", (name, coerced), {"ex": ex, "px": px, "nx": nx, "xx": xx}))
        return self

    def get(self, name):
        self._commands.append(("get", (name,), {}))
        return self

    def delete(self, *names):
        self._commands.append(("delete", names, {}))
        return self

    def exists(self, *names):
        self._commands.append(("exists", names, {}))
        return self

    # ---- Hash Commands ----

    def hset(self, name, key=None, value=None, mapping=None):
        self._commands.append(("hset", (name,), {"key": key, "value": value, "mapping": mapping}))
        return self

    def hget(self, name, key):
        self._commands.append(("hget", (name, key), {}))
        return self

    def hdel(self, name, *keys):
        self._commands.append(("hdel", (name, *keys), {}))
        return self

    def hvals(self, name):
        self._commands.append(("hvals", (name,), {}))
        return self

    # ---- Set Commands ----

    def sadd(self, name, *values):
        self._commands.append(("sadd", (name, *values), {}))
        return self

    def smembers(self, name):
        self._commands.append(("smembers", (name,), {}))
        return self

    def sismember(self, name, value):
        self._commands.append(("sismember", (name, value), {}))
        return self

    def srem(self, name, *values):
        self._commands.append(("srem", (name, *values), {}))
        return self

    # ---- Sorted Set Commands ----

    def zadd(self, name, mapping, nx=False, xx=False, gt=False, lt=False, ch=False):
        self._commands.append(("zadd", (name, mapping), {"nx": nx, "xx": xx, "gt": gt, "lt": lt, "ch": ch}))
        return self

    def zrem(self, name, *values):
        self._commands.append(("zrem", (name, *values), {}))
        return self

    def zrange(self, name, start, end, withscores=False):
        self._commands.append(("zrange", (name, start, end), {"withscores": withscores}))
        return self

    def zrangebyscore(self, name, min, max, withscores=False):
        self._commands.append(("zrangebyscore", (name, min, max), {"withscores": withscores}))
        return self

    def zrangestore(self, dest, name, start, end):
        self._commands.append(("zrangestore", (dest, name, start, end), {}))
        return self

    def zremrangebyscore(self, name, min, max):
        self._commands.append(("zremrangebyscore", (name, min, max), {}))
        return self

    # ---- List Commands ----

    def lpush(self, name, *values):
        # H-01: per-value coercion mirrors the monkey-patched `_coerced_lpush`.
        coerced = tuple(_coerce_value(v) for v in values)
        self._commands.append(("lpush", (name, *coerced), {}))
        return self

    def rpush(self, name, *values):
        # H-01: per-value coercion mirrors the monkey-patched `_coerced_rpush`.
        coerced = tuple(_coerce_value(v) for v in values)
        self._commands.append(("rpush", (name, *coerced), {}))
        return self

    def lpop(self, name, count=None):
        self._commands.append(("lpop", (name,), {"count": count}))
        return self

    def rpop(self, name, count=None):
        self._commands.append(("rpop", (name,), {"count": count}))
        return self

    def lrange(self, name, start, end):
        self._commands.append(("lrange", (name, start, end), {}))
        return self

    def llen(self, name):
        self._commands.append(("llen", (name,), {}))
        return self

    def lindex(self, name, index):
        self._commands.append(("lindex", (name, index), {}))
        return self

    def linsert(self, name, where, refvalue, value):
        # H-01: coerce inserted `value`.
        # P2-06: also coerce `refvalue` — redis-py encodes every command
        # argument including the pivot, so numeric pivots are legal.
        self._commands.append(
            ("linsert", (name, where, _coerce_value(refvalue), _coerce_value(value)), {})
        )
        return self

    def lrem(self, name, count, value):
        # P2-07: coerce `value` — redis-py encodes ints/floats for LREM
        # values just like LPUSH/LSET. Mirror of `_coerced_lrem`.
        self._commands.append(("lrem", (name, count, _coerce_value(value)), {}))
        return self

    def lset(self, name, index, value):
        # H-01: coerce inserted value (mirror of `_coerced_lset`).
        self._commands.append(("lset", (name, index, _coerce_value(value)), {}))
        return self

    def ltrim(self, name, start, end):
        self._commands.append(("ltrim", (name, start, end), {}))
        return self

    def lmove(self, first_list, second_list, src="LEFT", dest="RIGHT"):
        self._commands.append(("lmove", (first_list, second_list), {"src": src, "dest": dest}))
        return self

    def rpoplpush(self, src, dst):
        self._commands.append(("rpoplpush", (src, dst), {}))
        return self

    def blpop(self, keys, timeout=0):
        self._commands.append(("blpop", (keys,), {"timeout": timeout}))
        return self

    def brpop(self, keys, timeout=0):
        self._commands.append(("brpop", (keys,), {"timeout": timeout}))
        return self

    def blmove(self, first_list, second_list, timeout, src="LEFT", dest="RIGHT"):
        self._commands.append(("blmove", (first_list, second_list, timeout), {"src": src, "dest": dest}))
        return self

    # ---- Stream Commands ----

    def xadd(self, name, fields, id="*", maxlen=None, minid=None):
        self._commands.append(("xadd", (name, fields), {"id": id, "maxlen": maxlen, "minid": minid}))
        return self

    def xread(self, streams, count=None, block=None):
        self._commands.append(("xread", (streams,), {"count": count, "block": block}))
        return self

    def xlen(self, name):
        self._commands.append(("xlen", (name,), {}))
        return self

    def xtrim(self, name, maxlen=None, minid=None, approximate=True):
        self._commands.append(("xtrim", (name,), {"maxlen": maxlen, "minid": minid, "approximate": approximate}))
        return self

    # ---- Consumer Group Commands ----

    def xgroup_create(self, name, groupname, id="$", mkstream=False):
        self._commands.append(("xgroup_create", (name, groupname), {"id": id, "mkstream": mkstream}))
        return self

    def xgroup_destroy(self, name, groupname):
        self._commands.append(("xgroup_destroy", (name, groupname), {}))
        return self

    def xreadgroup(self, groupname, consumername, streams, count=None, block=None, noack=False):
        self._commands.append(("xreadgroup", (groupname, consumername, streams), {"count": count, "block": block, "noack": noack}))
        return self

    def xack(self, name, groupname, *ids):
        self._commands.append(("xack", (name, groupname, *ids), {}))
        return self

    def xautoclaim(self, name, groupname, consumername, min_idle_time, start_id="0-0", count=None):
        self._commands.append(("xautoclaim", (name, groupname, consumername, min_idle_time), {"start_id": start_id, "count": count}))
        return self

    def xclaim(self, name, groupname, consumername, min_idle_time, message_ids,
               idle=None, time=None, retrycount=None, force=False, justid=False):
        self._commands.append(("xclaim", (name, groupname, consumername, min_idle_time, message_ids),
                              {"idle": idle, "time": time, "retrycount": retrycount,
                               "force": force, "justid": justid}))
        return self

    def xinfo_groups(self, name):
        self._commands.append(("xinfo_groups", (name,), {}))
        return self

    def xinfo_consumers(self, name, groupname):
        self._commands.append(("xinfo_consumers", (name, groupname), {}))
        return self

    def xpending_range(self, name, groupname, min="-", max="+", count=100, consumername=None, idle=None):
        self._commands.append(("xpending_range", (name, groupname, min, max, count), {"consumername": consumername, "idle": idle}))
        return self

    # ---- Scripting Commands ----

    def eval(self, script, numkeys, *keys_and_args):
        self._commands.append(("eval", (script, numkeys, *keys_and_args), {}))
        return self

    def evalsha(self, sha, numkeys, *keys_and_args):
        self._commands.append(("evalsha", (sha, numkeys, *keys_and_args), {}))
        return self

    def script_load(self, script):
        self._commands.append(("script_load", (script,), {}))
        return self

    def script_exists(self, *args):
        self._commands.append(("script_exists", args, {}))
        return self

    # ---- Additional Hash Commands ----

    def hgetall(self, name):
        self._commands.append(("hgetall", (name,), {}))
        return self

    def hexists(self, name, key):
        self._commands.append(("hexists", (name, key), {}))
        return self

    def hincrby(self, name, key, amount=1):
        self._commands.append(("hincrby", (name, key), {"amount": amount}))
        return self

    # ---- Additional Sorted Set Commands ----

    def zcard(self, name):
        self._commands.append(("zcard", (name,), {}))
        return self

    def zscore(self, name, value):
        self._commands.append(("zscore", (name, value), {}))
        return self

    def zcount(self, name, min, max):
        self._commands.append(("zcount", (name, min, max), {}))
        return self

    # ---- Key Commands ----

    def expire(self, name, time):
        self._commands.append(("expire", (name, time), {}))
        return self

    # ---- Additional Stream Commands ----

    def xdel(self, name, *ids):
        self._commands.append(("xdel", (name, *ids), {}))
        return self

    def xrange(self, name, min="-", max="+", count=None):
        self._commands.append(("xrange", (name,), {"min": min, "max": max, "count": count}))
        return self

    # ---- Pub/Sub Commands ----

    def publish(self, channel, message):
        self._commands.append(("publish", (channel, message), {}))
        return self

    # ---- Key Enumeration Commands ----

    def keys(self, pattern="*"):
        self._commands.append(("keys", (pattern,), {}))
        return self

    def ttl(self, name):
        self._commands.append(("ttl", (name,), {}))
        return self

    def setex(self, name, time, value):
        self._commands.append(("setex", (name, time, value), {}))
        return self

    def mget(self, *keys):
        self._commands.append(("mget", keys, {}))
        return self

    def xpending(self, name, groupname):
        self._commands.append(("xpending", (name, groupname), {}))
        return self

    def scan_iter(self, match=None, count=None, _type=None):
        raise NotImplementedError(
            "scan_iter is an async generator and cannot be used in a pipeline. "
            "Use scan_iter() directly on the client instead."
        )
