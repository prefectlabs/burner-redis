from burner_redis._burner_redis import BurnerRedis
from burner_redis.pipeline import Pipeline
from burner_redis.lock import Lock, LockError


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

__all__ = ["BurnerRedis", "Lock", "LockError", "Pipeline", "ResponseError"]
