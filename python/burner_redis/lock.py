"""Lock class for distributed locking with token-based ownership.

Provides redis-py compatible Lock API using SET NX PX for atomic
lock acquisition with UUID token-based ownership verification.
"""
from __future__ import annotations

import asyncio
import time
import uuid
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from types import TracebackType

    from burner_redis._burner_redis import BurnerRedis, KeyT


# `redis` is an optional runtime dependency. See the matching comment in
# `burner_redis/__init__.py` for the TYPE_CHECKING rationale.
if TYPE_CHECKING:
    _BaseLockError = Exception
else:
    try:
        from redis.exceptions import LockError as _BaseLockError
    except (ImportError, AttributeError):
        _BaseLockError = Exception


class LockError(_BaseLockError):
    """Raised when a lock operation fails (e.g., release without ownership).

    Subclasses `redis.exceptions.LockError` when the `redis` package is
    installed so existing redis-py error handlers catch us; otherwise falls
    back to `Exception`.
    """
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

    def __init__(
        self,
        client: BurnerRedis,
        name: KeyT,
        timeout: float | None = None,
        sleep: float = 0.1,
        blocking: bool = True,
        blocking_timeout: float | None = None,
    ) -> None:
        self._client = client
        self.name = name
        self.timeout = timeout
        self.sleep = sleep
        self.blocking = blocking
        self.blocking_timeout = blocking_timeout
        self.token: str | None = None

    async def acquire(
        self,
        blocking: bool | None = None,
        blocking_timeout: float | None = None,
    ) -> bool:
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

    async def release(self) -> None:
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

    async def __aenter__(self) -> Lock:
        acquired = await self.acquire()
        if not acquired:
            raise LockError("Unable to acquire lock")
        return self

    async def __aexit__(
        self,
        exc_type: type[BaseException] | None,
        exc_val: BaseException | None,
        exc_tb: TracebackType | None,
    ) -> bool:
        await self.release()
        return False
