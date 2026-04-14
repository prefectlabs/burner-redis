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
        return await target.evalsha(self.sha, len(keys), *keys, *args)


def _register_script(self, script):
    """Register a Lua script and return a callable Script object.

    Compatible with redis.asyncio.Redis.register_script().
    """
    return Script(self, script)


BurnerRedis.register_script = _register_script

__all__ = ["BurnerRedis", "Lock", "LockError", "Pipeline", "PubSub", "ResponseError", "Script"]
