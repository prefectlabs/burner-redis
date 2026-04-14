"""Lock class for distributed locking with token-based ownership.

Provides redis-py compatible Lock API using SET NX PX for atomic
lock acquisition with UUID token-based ownership verification.
"""
import asyncio
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


class Lock:
    """Distributed lock with token-based ownership.

    Created via client.lock(name, ...). Uses SET NX PX for atomic acquisition
    and token verification for safe release.

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
        elapsed = 0.0
        while True:
            result = await self._client.set(self.name, token, px=px, nx=True)
            if result is True:
                self.token = token
                return True

            if blocking_timeout is not None:
                elapsed += self.sleep
                if elapsed >= blocking_timeout:
                    return False

            await asyncio.sleep(self.sleep)

    async def release(self):
        """Release the lock.

        Verifies token ownership before deleting. Raises LockError if
        the lock is not owned by this instance (token mismatch or expired).
        """
        if self.token is None:
            raise LockError("Cannot release an unlocked lock")

        stored = await self._client.get(self.name)
        if stored is None:
            raise LockError("Cannot release an unlocked lock")

        if stored != self.token.encode():
            raise LockError("Cannot release a lock that's no longer owned")

        await self._client.delete(self.name)
        self.token = None

    async def __aenter__(self):
        acquired = await self.acquire()
        if not acquired:
            raise LockError("Unable to acquire lock")
        return self

    async def __aexit__(self, exc_type, exc_val, exc_tb):
        await self.release()
        return False
