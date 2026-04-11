from burner_redis._burner_redis import BurnerRedis
from burner_redis.pipeline import Pipeline


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

__all__ = ["BurnerRedis", "Pipeline", "ResponseError"]
