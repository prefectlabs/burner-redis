"""Lock class for distributed locking with token-based ownership.

Provides redis-py compatible Lock API using SET NX PX for atomic
lock acquisition with UUID token-based ownership verification.
"""
import asyncio
import time
import uuid


class LockError(Exception):
    """Raised when a lock operation fails (e.g., release without ownership)."""
    pass


try:
    import redis.exceptions

    class LockError(redis.exceptions.LockError):  # type: ignore[no-redef]
        """Raised when a lock operation fails (subclass of redis.exceptions.LockError)."""
        pass
except (ImportError, AttributeError):
    pass


RELEASE_SCRIPT = """
if redis.call("get", KEYS[1]) == ARGV[1] then
    return redis.call("del", KEYS[1])
else
    return 0
end
"""


class Lock:
    """Distributed lock with token-based ownership.

    Created via client.lock(name, ...). Uses SET NX PX for atomic acquisition
    and token verification for safe release. Release uses a Lua script for
    atomic check-and-delete to prevent TOCTOU race conditions.

    Args:
        client: BurnerRedis instance
        name: Lock key name
        timeout: Lock TTL in seconds (None = no expiry). Converted to milliseconds for PX.
        sleep: Polling interval in seconds for blocking acquire (default 0.1)
        blocking: Whether acquire() blocks until lock is obtained (default True)
        blocking_timeout: Maximum seconds to wait for blocking acquire (None = wait forever)
    """

    def __init__(self, client, name, timeout=None, sleep=0.1, blocking=True, blocking_timeout=None):
        self._client = client
        self.name = name
        self.timeout = timeout
        self.sleep = sleep
        self.blocking = blocking
        self.blocking_timeout = blocking_timeout
        self.token = None

    async def acquire(self, blocking=None, blocking_timeout=None):
        """Acquire the lock.

        Args:
            blocking: Override instance blocking setting
            blocking_timeout: Override instance blocking_timeout setting

        Returns:
            True if lock was acquired, False if non-blocking and lock not available.
        """
        if blocking is None:
            blocking = self.blocking
        if blocking_timeout is None:
            blocking_timeout = self.blocking_timeout

        token = str(uuid.uuid4())

        # Calculate PX (milliseconds) from timeout (seconds)
        px = int(self.timeout * 1000) if self.timeout is not None else None

        if not blocking:
            # Non-blocking: single attempt
            result = await self._client.set(self.name, token, px=px, nx=True)
            if result is True:
                self.token = token
                return True
            return False

        # Blocking: poll until acquired or timeout
        deadline = time.monotonic() + blocking_timeout if blocking_timeout is not None else None
        while True:
            result = await self._client.set(self.name, token, px=px, nx=True)
            if result is True:
                self.token = token
                return True

            if deadline is not None and time.monotonic() >= deadline:
                return False

            await asyncio.sleep(self.sleep)

    async def release(self):
        """Release the lock atomically using a Lua script.

        Uses EVAL with a Lua script to atomically check token ownership
        and delete the key, preventing TOCTOU race conditions where the
        lock could expire and be re-acquired between GET and DELETE.

        Raises LockError if the lock is not owned by this instance.
        """
        if self.token is None:
            raise LockError("Cannot release an unlocked lock")

        result = await self._client.eval(RELEASE_SCRIPT, 1, self.name, self.token)
        if result != 1:
            raise LockError("Cannot release a lock that's no longer owned")
        self.token = None

    async def __aenter__(self):
        acquired = await self.acquire()
        if not acquired:
            raise LockError("Unable to acquire lock")
        return self

    async def __aexit__(self, exc_type, exc_val, exc_tb):
        await self.release()
        return False
