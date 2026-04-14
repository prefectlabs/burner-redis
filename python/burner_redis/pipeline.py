"""Pipeline class for batched command execution.

Provides redis-py compatible Pipeline API that buffers commands
and executes them sequentially against a BurnerRedis instance.
"""


class Pipeline:
    """Buffers commands and executes them as a batch.

    Created via client.pipeline(). Commands are queued as
    (method_name, args, kwargs) tuples and executed sequentially
    on execute(), returning results in command order.
    """

    def __init__(self, client):
        self._client = client
        self._commands = []

    async def execute(self):
        """Execute all queued commands and return results in order."""
        results = []
        for method_name, args, kwargs in self._commands:
            method = getattr(self._client, method_name)
            result = await method(*args, **kwargs)
            results.append(result)
        self._commands = []
        return results

    async def __aenter__(self):
        return self

    async def __aexit__(self, exc_type, exc_val, exc_tb):
        if exc_type is None:
            await self.execute()
        return False

    # ---- String Commands ----

    def set(self, name, value, ex=None, px=None, nx=False, xx=False):
        self._commands.append(("set", (name, value), {"ex": ex, "px": px, "nx": nx, "xx": xx}))
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
