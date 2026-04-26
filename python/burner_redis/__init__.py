from burner_redis._burner_redis import BurnerRedis
from burner_redis.pipeline import Pipeline
from burner_redis.lock import Lock, LockError
from burner_redis.pubsub import PubSub


class ResponseError(Exception):
    """Redis-compatible WRONGTYPE error.

    Subclasses redis.exceptions.ResponseError if redis package is available.
    """
    pass


# Try to make it a subclass of redis.exceptions.ResponseError if available
try:
    import redis.exceptions

    class ResponseError(redis.exceptions.ResponseError):  # type: ignore[no-redef]
        """Redis-compatible WRONGTYPE error (subclass of redis.exceptions.ResponseError)."""
        pass
except (ImportError, AttributeError):
    pass


class NoScriptError(Exception):
    """Raised when EVALSHA references an unknown script SHA."""
    pass


try:
    import redis.exceptions

    class NoScriptError(redis.exceptions.NoScriptError):  # type: ignore[no-redef]
        """Raised when EVALSHA references an unknown script SHA (subclass of redis.exceptions.NoScriptError)."""
        pass
except (ImportError, AttributeError):
    pass


def _coerce_value(value):
    """Coerce a value to str or bytes, matching redis-py's Encoder.encode() behavior.

    Accepts: bytes, memoryview, int, float, str.
    Rejects: bool (redis-py rejects bools with DataError since bool is subclass of int).
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


_original_set = BurnerRedis.set


async def _coerced_set(self, name, value, ex=None, px=None, nx=False, xx=False):
    """SET with value coercion matching redis-py behavior."""
    return await _original_set(self, name, _coerce_value(value), ex=ex, px=px, nx=nx, xx=xx)


BurnerRedis.set = _coerced_set


async def _setex(self, name, time, value):
    """SETEX: Set key with expiration in seconds. Shorthand for SET with EX."""
    return await self.set(name, _coerce_value(value), ex=time)


BurnerRedis.setex = _setex


# ---- List Commands: value coercion wrappers (redis-py parity) ----

_original_lpush = BurnerRedis.lpush


async def _coerced_lpush(self, name, *values):
    """LPUSH with per-value coercion matching redis-py behavior."""
    coerced = [_coerce_value(v) for v in values]
    return await _original_lpush(self, name, *coerced)


BurnerRedis.lpush = _coerced_lpush


_original_rpush = BurnerRedis.rpush


async def _coerced_rpush(self, name, *values):
    """RPUSH with per-value coercion matching redis-py behavior."""
    coerced = [_coerce_value(v) for v in values]
    return await _original_rpush(self, name, *coerced)


BurnerRedis.rpush = _coerced_rpush


_original_lset = BurnerRedis.lset


async def _coerced_lset(self, name, index, value):
    """LSET with value coercion matching redis-py behavior."""
    return await _original_lset(self, name, index, _coerce_value(value))


BurnerRedis.lset = _coerced_lset


_original_lrem = BurnerRedis.lrem


async def _coerced_lrem(self, name, count, value):
    """LREM with value coercion.

    P2-07: redis-py encodes ints/floats for `value` arguments via
    Encoder.encode() just like LPUSH/LSET. The PyO3 binding only accepts
    str/bytes, so we coerce at the Python boundary to match drop-in
    behavior — `r.lrem('k', 0, 42)` should match the bytes b'42'.
    """
    return await _original_lrem(self, name, count, _coerce_value(value))


BurnerRedis.lrem = _coerced_lrem


_original_linsert = BurnerRedis.linsert


async def _coerced_linsert(self, name, where, refvalue, value):
    """LINSERT with value coercion.

    P2-06: redis-py encodes EVERY command argument including the LINSERT
    `refvalue` pivot, so numeric pivots are legal (they encode to bytes
    via the same Encoder.encode() path as `value`). We coerce both to
    match drop-in behavior.
    """
    return await _original_linsert(
        self, name, where, _coerce_value(refvalue), _coerce_value(value)
    )


BurnerRedis.linsert = _coerced_linsert


# ---- Blocking List Commands: coroutine wrappers (redis-py parity) ----
#
# PyO3's pyo3_async_runtimes::tokio::future_into_py(...) schedules the
# returned future onto the Tokio runtime *immediately at call time* and
# returns an asyncio.Future. redis.asyncio.Redis blocking commands must
# instead be coroutines so that:
#   1. asyncio.create_task(r.blpop(...)) accepts them (create_task requires
#      a coroutine, not a Future).
#   2. The blocking pop only begins when the coroutine is awaited /
#      scheduled — not when r.blpop(...) is called.
# The async-def wrappers below capture the underlying Rust binding and
# defer its invocation until the wrapper coroutine itself is awaited.

_original_blpop = BurnerRedis.blpop


async def _async_blpop(self, keys, timeout=None):
    """BLPOP wrapped as a coroutine for redis.asyncio.Redis compatibility.

    The underlying Rust binding returns an asyncio.Future eagerly; wrapping
    it in `async def` defers the call until the coroutine is awaited and
    makes asyncio.create_task(r.blpop(...)) accept it.
    """
    return await _original_blpop(self, keys, timeout=timeout)


BurnerRedis.blpop = _async_blpop


_original_brpop = BurnerRedis.brpop


async def _async_brpop(self, keys, timeout=None):
    """BRPOP wrapped as a coroutine for redis.asyncio.Redis compatibility.

    See _async_blpop docstring for rationale.
    """
    return await _original_brpop(self, keys, timeout=timeout)


BurnerRedis.brpop = _async_brpop


_original_blmove = BurnerRedis.blmove


async def _async_blmove(self, first_list, second_list, timeout, src="LEFT", dest="RIGHT"):
    """BLMOVE wrapped as a coroutine for redis.asyncio.Redis compatibility.

    Signature mirrors redis.asyncio.Redis.blmove (src/dest default to
    "LEFT"/"RIGHT"). Values are forwarded through to the Rust binding,
    which is the single source of truth for default handling.

    See _async_blpop docstring for rationale.
    """
    return await _original_blmove(self, first_list, second_list, timeout, src=src, dest=dest)


BurnerRedis.blmove = _async_blmove


async def _scan_iter(self, match=None, count=None, _type=None):
    """Async iterator over keys matching a glob pattern.

    Wraps keys() as an async generator for redis-py scan_iter() compatibility.
    count and _type parameters are accepted but ignored (in-process, no cursor needed).
    """
    pattern = match if match is not None else "*"
    keys = await self.keys(pattern)
    for key in keys:
        yield key


BurnerRedis.scan_iter = _scan_iter


def _pipeline(self):
    """Create a Pipeline for batched command execution."""
    return Pipeline(self)


BurnerRedis.pipeline = _pipeline


def _lock(self, name, timeout=None, sleep=0.1, blocking=True, blocking_timeout=None):
    """Create a Lock for distributed locking."""
    return Lock(self, name, timeout=timeout, sleep=sleep, blocking=blocking, blocking_timeout=blocking_timeout)


BurnerRedis.lock = _lock


def _pubsub(self, ignore_subscribe_messages=False):
    """Create a PubSub for channel/pattern message subscription."""
    return PubSub(self, ignore_subscribe_messages=ignore_subscribe_messages)


BurnerRedis.pubsub = _pubsub


class Script:
    """Redis-compatible Script object returned by register_script().

    Stores the Lua script text. On first invocation, loads the script
    via SCRIPT LOAD to get the SHA, then uses EVALSHA for execution.
    """

    def __init__(self, client, script):
        self.client = client
        self.script = script if isinstance(script, str) else script.decode()
        self.sha = None

    @staticmethod
    def _coerce_arg(arg):
        """Coerce a script argument to str or bytes for evalsha compatibility."""
        if isinstance(arg, (str, bytes, memoryview)):
            return arg
        if isinstance(arg, (int, float)):
            return str(arg)
        return str(arg)

    async def __call__(self, keys=[], args=[], client=None):
        """Execute the script with the given keys and args.

        Args:
            keys: List of Redis keys the script accesses.
            args: List of additional arguments passed to the script.
            client: Optional alternative client to use for execution.
        """
        target = client or self.client
        if self.sha is None:
            self.sha = await target.script_load(self.script)
        coerced_keys = [self._coerce_arg(k) for k in keys]
        coerced_args = [self._coerce_arg(a) for a in args]
        return await target.evalsha(self.sha, len(coerced_keys), *coerced_keys, *coerced_args)


def _register_script(self, script):
    """Register a Lua script and return a callable Script object.

    Compatible with redis.asyncio.Redis.register_script().
    """
    return Script(self, script)


BurnerRedis.register_script = _register_script


async def _aclose(self):
    """Graceful shutdown: drain all in-flight Rust futures and stop listeners.

    Matches redis.asyncio.Redis.aclose() interface.
    """
    await self._aclose()


async def _close_alias(self):
    """Alias for aclose(). Matches redis.asyncio.Redis.close() interface."""
    await self._aclose()


async def _aenter(self):
    """Async context manager entry."""
    return self


async def _aexit(self, *args):
    """Async context manager exit: calls aclose()."""
    await self._aclose()


BurnerRedis.aclose = _aclose
BurnerRedis.close = _close_alias
BurnerRedis.__aenter__ = _aenter
BurnerRedis.__aexit__ = _aexit

__all__ = ["BurnerRedis", "Lock", "LockError", "NoScriptError", "Pipeline", "PubSub", "ResponseError", "Script", "_coerce_value"]
