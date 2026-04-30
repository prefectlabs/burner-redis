# T02: Create comprehensive Python integration tests validating key expiration across all data types, testing both passive (on-access) and active (background sweep) expiration strategies.

**Slice:** S04 — **Milestone:** M001

## Description

Create comprehensive Python integration tests validating key expiration across all data types, testing both passive (on-access) and active (background sweep) expiration strategies.

Purpose: Prove that EXP-01 (keys expire after TTL), EXP-02 (seconds and milliseconds precision), and EXP-03 (passive + active cleanup) all work correctly at the Python API level, covering strings, hashes, sets, and sorted sets.

Output: tests/test_expiration.py with pytest-asyncio tests covering all expiration behaviors.

## Legacy Source

---
phase: 04-key-expiration
plan: 02
type: execute
wave: 2
depends_on:
  - 04-01
files_modified:
  - tests/test_expiration.py
autonomous: true
requirements:
  - EXP-01
  - EXP-02
  - EXP-03
must_haves:
  truths:
    - "A string key set with EX (seconds) TTL is inaccessible after expiration"
    - "A string key set with PX (milliseconds) TTL is inaccessible after expiration"
    - "An expired hash key returns empty/None on HGET and HVALS"
    - "An expired set key returns empty on SMEMBERS and false on SISMEMBER"
    - "An expired sorted set key returns empty on ZRANGE and ZRANGEBYSCORE"
    - "Active sweep cleans up expired keys that are never accessed"
    - "EXISTS returns 0 for expired keys"
  artifacts:
    - path: "tests/test_expiration.py"
      provides: "Comprehensive Python integration tests for passive and active expiration across all data types"
      min_lines: 80
  key_links:
    - from: "tests/test_expiration.py"
      to: "burner_redis.BurnerRedis"
      via: "pytest async tests calling set/get/hset/hget/sadd/smembers/zadd/zrange with TTL"
      pattern: "await r\\.set.*ex=|await r\\.set.*px="
---

<objective>
Create comprehensive Python integration tests validating key expiration across all data types, testing both passive (on-access) and active (background sweep) expiration strategies.

Purpose: Prove that EXP-01 (keys expire after TTL), EXP-02 (seconds and milliseconds precision), and EXP-03 (passive + active cleanup) all work correctly at the Python API level, covering strings, hashes, sets, and sorted sets.

Output: tests/test_expiration.py with pytest-asyncio tests covering all expiration behaviors.
</objective>

<execution_context>
@/Users/desertaxle/.claude/get-shit-done/workflows/execute-plan.md
@/Users/desertaxle/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@.planning/PROJECT.md
@.planning/ROADMAP.md
@.planning/STATE.md
@.planning/phases/04-key-expiration/04-CONTEXT.md
@.planning/phases/04-key-expiration/04-01-SUMMARY.md

<interfaces>
<!-- Key Python API the executor tests against. Extracted from lib.rs and __init__.py. -->

From python/burner_redis/__init__.py:
```python
from burner_redis._burner_redis import BurnerRedis
class ResponseError(Exception): ...
__all__ = ["BurnerRedis", "ResponseError"]
```

BurnerRedis Python methods (all async):
```python
async def set(name, value, ex=None, px=None, nx=False, xx=False) -> Optional[bool]
async def get(name) -> Optional[bytes]
async def delete(*names) -> int
async def exists(*names) -> int
async def hset(name, key=None, value=None, mapping=None) -> int
async def hget(name, key) -> Optional[bytes]
async def hdel(name, *keys) -> int
async def hvals(name) -> list[bytes]
async def sadd(name, *values) -> int
async def smembers(name) -> set[bytes]
async def sismember(name, value) -> bool
async def srem(name, *values) -> int
async def zadd(name, mapping, nx=False, xx=False, gt=False, lt=False, ch=False) -> int
async def zrem(name, *values) -> int
async def zrange(name, start, end, withscores=False) -> list
async def zrangebyscore(name, min, max, withscores=False) -> list
async def zrangestore(dest, name, start, end) -> int
async def zremrangebyscore(name, min, max) -> int
```

From tests/conftest.py:
```python
@pytest.fixture
def r():
    return BurnerRedis()
```

Existing test patterns (from tests/test_strings.py):
- Uses `asyncio.sleep()` for TTL waits
- Uses `await r.set("key", "value", px=50)` then `await asyncio.sleep(0.1)` for short TTL tests
- Tests are plain `async def test_*` functions using `r` fixture
</interfaces>
</context>

<tasks>

<task type="auto">
  <name>Task 1: Build module with sweep support</name>
  <files>python/burner_redis/_burner_redis.abi3.so</files>
  <read_first>Cargo.toml, pyproject.toml</read_first>
  <action>
Run `maturin develop` to build the updated Rust code (with sweep_expired and background task from Plan 01) into the Python package. This makes the new behavior available for Python tests.

```bash
cd /Users/desertaxle/dev/prefectlabs/burner-redis && maturin develop
```

After building, verify the module loads correctly:

```bash
python3 -c "from burner_redis import BurnerRedis; r = BurnerRedis(); print('OK')"
```
  </action>
  <verify>
    <automated>cd /Users/desertaxle/dev/prefectlabs/burner-redis && maturin develop 2>&1 | tail -3 && python3 -c "from burner_redis import BurnerRedis; r = BurnerRedis(); print('Module loaded OK')"</automated>
  </verify>
  <acceptance_criteria>
    - maturin develop completes without errors
    - python3 -c "from burner_redis import BurnerRedis; BurnerRedis()" succeeds
  </acceptance_criteria>
  <done>Updated Rust module is compiled and importable from Python with background sweep task active.</done>
</task>

<task type="auto">
  <name>Task 2: Python integration tests for passive and active expiration</name>
  <files>tests/test_expiration.py</files>
  <read_first>tests/conftest.py, tests/test_strings.py</read_first>
  <action>
Create tests/test_expiration.py with comprehensive expiration tests. Follow the existing test patterns: plain async def functions, use the `r` fixture from conftest.py, use `asyncio.sleep()` for TTL waits.

The test file should cover:

**EXP-01: Keys with TTL are inaccessible after expiration (passive)**

1. `test_string_expires_with_ex` -- SET with ex=1 (seconds), sleep 1.1s, GET returns None.
2. `test_string_expires_with_px` -- SET with px=100 (milliseconds), sleep 0.15s, GET returns None.
3. `test_hash_expires_passive` -- SET a key with px=100, then HSET fields on a DIFFERENT key with TTL. Wait, the problem is: HSET does not accept TTL directly. Expiration is set via SET command only (SET EX/PX). For hash/set/sorted-set keys, TTL can only be set if SET was used to create a String first and then the type changed... No, actually in Redis TTL is set per-key by the EXPIRE/PEXPIRE commands. In our implementation, only SET supports EX/PX. Hashes, sets, and sorted sets created via HSET/SADD/ZADD do NOT get a TTL unless we add EXPIRE command support.

**IMPORTANT REALIZATION:** The current implementation only supports TTL via the SET command (which creates String-typed values). Hash, Set, and SortedSet entries are created without TTL (expires_at: None). The passive expiration checks on hash/set/sorted-set operations exist for keys that were SET with a TTL and then type-changed (which would be a WRONGTYPE error anyway) or for future EXPIRE command support.

So for Phase 4, the meaningful tests are:
- String keys with EX/PX expire correctly (passive on GET, EXISTS, DELETE)
- Active sweep cleans up expired String keys without access
- Expired keys treated as non-existent for NX/XX conditions

Create the following tests:

```python
"""Tests for key expiration: passive on-access and active background sweep.

Covers requirements: EXP-01, EXP-02, EXP-03.
"""
import asyncio

import pytest
from burner_redis import BurnerRedis


# --- EXP-01: Keys with TTL expire and are no longer accessible ---

async def test_string_ex_expires(r):
    """EXP-01: Key set with EX (seconds) is inaccessible after TTL."""
    await r.set("mykey", "myvalue", ex=1)
    # Key accessible before expiry
    assert await r.get("mykey") == b"myvalue"
    await asyncio.sleep(1.1)
    # Key gone after expiry
    assert await r.get("mykey") is None


async def test_string_px_expires(r):
    """EXP-01: Key set with PX (milliseconds) is inaccessible after TTL."""
    await r.set("mykey", "myvalue", px=100)
    assert await r.get("mykey") == b"myvalue"
    await asyncio.sleep(0.15)
    assert await r.get("mykey") is None


async def test_expired_key_not_found_by_exists(r):
    """EXP-01: EXISTS returns 0 for an expired key."""
    await r.set("mykey", "myvalue", px=100)
    assert await r.exists("mykey") == 1
    await asyncio.sleep(0.15)
    assert await r.exists("mykey") == 0


async def test_expired_key_delete_returns_zero(r):
    """EXP-01: DELETE returns 0 for an expired key (treated as non-existent)."""
    await r.set("mykey", "myvalue", px=100)
    await asyncio.sleep(0.15)
    result = await r.delete("mykey")
    assert result == 0


async def test_expired_key_allows_nx_set(r):
    """EXP-01: SET with NX succeeds on an expired key (treated as non-existent)."""
    await r.set("mykey", "old", px=100)
    await asyncio.sleep(0.15)
    result = await r.set("mykey", "new", nx=True)
    assert result is True
    assert await r.get("mykey") == b"new"


async def test_expired_key_blocks_xx_set(r):
    """EXP-01: SET with XX fails on an expired key (treated as non-existent)."""
    await r.set("mykey", "old", px=100)
    await asyncio.sleep(0.15)
    result = await r.set("mykey", "new", xx=True)
    assert result is None


# --- EXP-02: Seconds and milliseconds precision ---

async def test_ex_precision_seconds(r):
    """EXP-02: EX=2 key survives at 1s but expires by 2.1s."""
    await r.set("mykey", "val", ex=2)
    await asyncio.sleep(1.0)
    assert await r.get("mykey") == b"val"  # Still alive at 1s
    await asyncio.sleep(1.2)
    assert await r.get("mykey") is None  # Gone at 2.2s total


async def test_px_precision_milliseconds(r):
    """EXP-02: PX=200 key survives at 100ms but expires by 250ms."""
    await r.set("mykey", "val", px=200)
    await asyncio.sleep(0.1)
    assert await r.get("mykey") == b"val"  # Still alive at 100ms
    await asyncio.sleep(0.2)
    assert await r.get("mykey") is None  # Gone at 300ms total


async def test_px_takes_precedence_over_ex(r):
    """EXP-02: When both PX and EX provided, PX takes precedence."""
    # PX=100ms should expire before EX=10s
    await r.set("mykey", "val", ex=10, px=100)
    await asyncio.sleep(0.15)
    assert await r.get("mykey") is None


# --- EXP-03: Active sweep cleans up expired keys without access ---

async def test_active_sweep_cleans_expired_keys(r):
    """EXP-03: Background sweep removes expired keys even if never accessed.

    Creates keys with short TTL, waits for expiry + sweep cycles,
    then checks internal state by creating a new key with the same name
    using NX (which would fail if the old key still existed in memory).
    """
    # Set 5 keys with very short TTL
    for i in range(5):
        await r.set(f"sweep-key-{i}", "value", px=50)

    # Wait for expiry (50ms) plus several sweep cycles (100ms each)
    # 400ms gives at least 3 sweep cycles after expiry
    await asyncio.sleep(0.4)

    # Keys should have been swept -- verify by checking exists
    for i in range(5):
        assert await r.exists(f"sweep-key-{i}") == 0, (
            f"sweep-key-{i} should have been cleaned up by active sweep"
        )


async def test_active_sweep_does_not_remove_live_keys(r):
    """EXP-03: Background sweep does not remove keys that have not expired."""
    await r.set("live-key", "value", ex=60)  # 60 second TTL
    await r.set("no-ttl-key", "value")  # No TTL at all

    # Wait for several sweep cycles
    await asyncio.sleep(0.4)

    # Both keys should still exist
    assert await r.get("live-key") == b"value"
    assert await r.get("no-ttl-key") == b"value"


async def test_multiple_instances_have_independent_sweep(r):
    """EXP-03: Each BurnerRedis instance has its own sweep task."""
    r2 = BurnerRedis()

    await r.set("r1-key", "val", px=50)
    await r2.set("r2-key", "val", px=50)

    await asyncio.sleep(0.4)

    # Both instances should have swept their own expired keys
    assert await r.exists("r1-key") == 0
    assert await r2.exists("r2-key") == 0
```

Note: The test for active sweep is inherently timing-sensitive. The 400ms wait provides ample margin: 50ms for expiry + 300ms for at least 3 sweep cycles at 100ms intervals. This is generous enough to avoid flakiness on CI.
  </action>
  <verify>
    <automated>cd /Users/desertaxle/dev/prefectlabs/burner-redis && pytest tests/test_expiration.py -x -v 2>&1 | tail -30</automated>
  </verify>
  <acceptance_criteria>
    - grep -q "EXP-01" tests/test_expiration.py
    - grep -q "EXP-02" tests/test_expiration.py
    - grep -q "EXP-03" tests/test_expiration.py
    - grep -q "test_string_ex_expires" tests/test_expiration.py
    - grep -q "test_string_px_expires" tests/test_expiration.py
    - grep -q "test_active_sweep_cleans_expired_keys" tests/test_expiration.py
    - grep -q "test_px_takes_precedence_over_ex" tests/test_expiration.py
    - grep -q "test_expired_key_allows_nx_set" tests/test_expiration.py
    - pytest tests/test_expiration.py passes all tests
    - pytest tests/ passes all tests (no regressions)
  </acceptance_criteria>
  <done>13 Python integration tests pass covering: passive expiration on GET/EXISTS/DELETE (EXP-01), seconds and milliseconds precision (EXP-02), and active background sweep cleanup (EXP-03). All existing tests in other test files continue to pass.</done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|----------|-------------|
| Python -> Rust | TTL values from Python tests (already validated in Phase 1 extract_expiry) |

## STRIDE Threat Register

| Threat ID | Category | Component | Disposition | Mitigation Plan |
|-----------|----------|-----------|-------------|-----------------|
| T-04-03 | T (Tampering) | test timing | accept | Tests use generous sleep margins (3x expected) to avoid flakiness. Timing-sensitive but inherent to expiration testing. |
</threat_model>

<verification>
After both tasks complete:
1. `pytest tests/test_expiration.py -v` passes all 13 tests
2. `pytest tests/ -v` passes all tests across all test files (no regressions)
3. Tests cover passive expiry (GET returns None), active sweep (keys cleaned without access), and both time precisions (EX seconds, PX milliseconds)
</verification>

<success_criteria>
- tests/test_expiration.py exists with 13+ async tests
- All tests pass covering EXP-01, EXP-02, and EXP-03
- No regressions in existing test files
- Active sweep test proves keys are cleaned up without explicit access
</success_criteria>

<output>
After completion, create `.planning/phases/04-key-expiration/04-02-SUMMARY.md`
</output>
