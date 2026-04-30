# T02: Validate all Phase 11 fixes against pydocket's test suite and close any remaining gaps.

**Slice:** S11 — **Milestone:** M001

## Description

Validate all Phase 11 fixes against pydocket's test suite and close any remaining gaps.

Purpose: D-05 requires a full gap inventory from pydocket's test suite. D-09 requires zero xfails/skips AND regression coverage. This plan runs pydocket tests, fixes any remaining issues discovered, removes xfail markers, and adds regression tests.

Output: All pydocket integration tests green, regression tests added, zero xfails in test suite.

## Legacy Source

---
phase: 11-close-redis-py-compatibility-gaps-for-pydocket-integration
plan: 02
type: execute
wave: 2
depends_on: [11-01]
files_modified:
  - tests/test_pydocket_compat.py
  - tests/test_streams.py
  - src/store.rs
  - src/lib.rs
  - src/scripting.rs
  - python/burner_redis/pipeline.py
  - python/burner_redis/__init__.py
autonomous: true
requirements: [D-01, D-02, D-04, D-05, D-09, D-10]

must_haves:
  truths:
    - "All pydocket integration tests pass with zero xfails and zero skips"
    - "The delayed task test (test_docket_add_delayed_task) passes reliably without xfail marker"
    - "Any additional gaps discovered by running pydocket tests are fixed"
    - "Regression tests in our suite cover every gap fixed in this phase"
  artifacts:
    - path: "tests/test_pydocket_compat.py"
      provides: "Pydocket integration tests with zero xfail markers"
      min_lines: 50
    - path: "tests/test_streams.py"
      provides: "Regression tests for XCLAIM, blocking XREADGROUP, and any new gaps"
      contains: "test_xclaim"
  key_links:
    - from: "tests/test_pydocket_compat.py"
      to: "python/burner_redis/__init__.py"
      via: "BurnerRedis import and monkey-patch fixture"
      pattern: "from burner_redis import BurnerRedis"
    - from: "tests/test_pydocket_compat.py (test_docket_add_delayed_task)"
      to: "src/lib.rs (xreadgroup blocking)"
      via: "pydocket Worker calls xreadgroup with block param"
      pattern: "xreadgroup"
---

<objective>
Validate all Phase 11 fixes against pydocket's test suite and close any remaining gaps.

Purpose: D-05 requires a full gap inventory from pydocket's test suite. D-09 requires zero xfails/skips AND regression coverage. This plan runs pydocket tests, fixes any remaining issues discovered, removes xfail markers, and adds regression tests.

Output: All pydocket integration tests green, regression tests added, zero xfails in test suite.
</objective>

<execution_context>
@$HOME/.claude/get-shit-done/workflows/execute-plan.md
@$HOME/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@.planning/PROJECT.md
@.planning/ROADMAP.md
@.planning/STATE.md
@.planning/phases/11-close-redis-py-compatibility-gaps-for-pydocket-integration/11-CONTEXT.md
@.planning/phases/11-close-redis-py-compatibility-gaps-for-pydocket-integration/11-RESEARCH.md
@.planning/phases/11-close-redis-py-compatibility-gaps-for-pydocket-integration/11-01-SUMMARY.md

<interfaces>
<!-- From Plan 01, the executor should have these available -->

From src/store.rs (after Plan 01):
```rust
// Store now has:
pub fn xclaim(&self, key, group, consumer, min_idle_time_ms, ids, idle, time, retrycount, force, justid) -> Result<Vec<(StreamId, Option<HashMap<Bytes, Bytes>>)>, StoreError>
pub fn stream_notify(&self) -> Arc<Notify>  // for blocking XREADGROUP
```

From src/lib.rs (after Plan 01):
```rust
// XREADGROUP now supports blocking via tokio::select! + stream_notify
// XCLAIM PyO3 binding exists
// XTRIM accepts approximate parameter
```

From tests/test_pydocket_compat.py:
```python
# Current xfail on test_docket_add_delayed_task should now be removable
# because blocking XREADGROUP was implemented in Plan 01
```
</interfaces>
</context>

<tasks>

<task type="auto">
  <name>Task 1: Run pydocket test suite, inventory and fix remaining gaps</name>
  <read_first>
    - tests/test_pydocket_compat.py (current state with xfail markers)
    - .planning/phases/11-close-redis-py-compatibility-gaps-for-pydocket-integration/11-01-SUMMARY.md (what Plan 01 accomplished)
    - .planning/phases/11-close-redis-py-compatibility-gaps-for-pydocket-integration/11-RESEARCH.md (lines 86-160 for known gaps and potential additional gaps)
  </read_first>
  <files>tests/test_pydocket_compat.py, src/store.rs, src/lib.rs, src/scripting.rs, python/burner_redis/pipeline.py, python/burner_redis/__init__.py</files>
  <action>
**Step 1: Run existing pydocket integration tests without xfail to discover current status.**

Run: `.venv/bin/python -m pytest tests/test_pydocket_compat.py -m integration --runxfail -v --tb=short`

This runs ALL pydocket tests including the previously-xfailed `test_docket_add_delayed_task`. Check:
- Does `test_docket_add_delayed_task` now pass reliably? Run it 10 times: `.venv/bin/python -m pytest tests/test_pydocket_compat.py::test_docket_add_delayed_task -m integration --runxfail --count=10 -v` (install pytest-repeat if needed: `.venv/bin/pip install pytest-repeat`)
- Do all other tests still pass?

**Step 2: Attempt to run pydocket's own test suite for comprehensive gap discovery (D-04, D-05).**

Clone pydocket's test suite and attempt to run it against BurnerRedis:

```bash
# Check if pydocket source is available
pip show docket | grep Location
# Look at pydocket's test structure
ls .venv/lib/python*/site-packages/docket/
```

Create a temporary conftest override to run pydocket's tests:

```bash
# Clone pydocket repo temporarily
git clone --depth 1 https://github.com/chrisguidry/docket.git /tmp/pydocket-tests
```

Create a conftest.py override in /tmp/pydocket-tests/tests/ that patches RedisConnection to use BurnerRedis (same monkey-patch pattern as our test_pydocket_compat.py). Run pydocket's tests:

```bash
cd /tmp/pydocket-tests && .venv/bin/python -m pytest tests/ -v --tb=short -x 2>&1 | head -200
```

**IMPORTANT:** If pydocket's own test suite cannot be run easily (requires Docker, specific fixtures, etc.), fall back to expanding our own integration tests to cover more pydocket scenarios. The goal is gap discovery, not test framework compatibility.

**Step 3: Fix any remaining gaps discovered.**

For each failure, determine the root cause:
- Missing command: Implement across all 4 layers (Store, PyO3, Pipeline, Lua dispatch if used from Lua)
- Behavioral difference: Fix to match redis-py semantics
- Missing parameter: Add the parameter (accept and ignore if not semantically meaningful for embedded DB)

Known potential gaps from research that may surface:
- Concurrency limit tests may exercise XCLAIM edge cases
- Redelivery tests may exercise XAUTOCLAIM/XCLAIM interaction
- Clear tests exercise XTRIM with `approximate=False` (fixed in Plan 01)
- Any commands that the static analysis missed

For each fix, follow the established 4-layer pattern:
1. Store method in `src/store.rs`
2. PyO3 async binding in `src/lib.rs`
3. Pipeline buffer method in `python/burner_redis/pipeline.py`
4. Lua dispatch entry in `src/scripting.rs` (if the command is used from Lua scripts)

**Step 4: Remove the xfail marker from test_docket_add_delayed_task.**

In tests/test_pydocket_compat.py, remove the `@pytest.mark.xfail(...)` decorator from `test_docket_add_delayed_task` (lines 150-153). The function should be a plain `async def test_docket_add_delayed_task(patch_pydocket):` with no xfail.

Also update the module docstring to remove mention of xfail: change "Tests either pass (proving compatibility) or are marked xfail with specific missing commands documented." to "All tests pass, proving full compatibility with pydocket's usage patterns."
  </action>
  <verify>
    <automated>.venv/bin/python -m pytest tests/test_pydocket_compat.py -m integration --runxfail -v --tb=short</automated>
  </verify>
  <acceptance_criteria>
    - tests/test_pydocket_compat.py does NOT contain `@pytest.mark.xfail` anywhere
    - tests/test_pydocket_compat.py does NOT contain `xfail` in any decorator
    - `.venv/bin/python -m pytest tests/test_pydocket_compat.py -m integration -v` exits 0 with all tests passing
    - test_docket_add_delayed_task passes reliably (no intermittent failures)
    - `.venv/bin/python -m pytest tests/ -q -m "not integration" -x` exits 0 (no regressions)
  </acceptance_criteria>
  <done>
    All pydocket integration tests pass with zero xfails. Any additional gaps discovered during inventory have been fixed. The delayed task race is resolved.
  </done>
</task>

<task type="auto">
  <name>Task 2: Add regression tests covering every gap fixed in Phase 11</name>
  <read_first>
    - tests/test_pydocket_compat.py (updated state from Task 1)
    - tests/test_streams.py (current state including Plan 01's new tests)
    - .planning/phases/11-close-redis-py-compatibility-gaps-for-pydocket-integration/11-01-SUMMARY.md
    - .planning/phases/11-close-redis-py-compatibility-gaps-for-pydocket-integration/11-RESEARCH.md (lines 136-158 for gap inventory)
  </read_first>
  <files>tests/test_pydocket_compat.py, tests/test_streams.py</files>
  <action>
**Step 1: Ensure regression tests exist for every gap fixed in Phase 11.**

Review what was fixed across Plan 01 and Task 1 of this plan. For each gap, verify tests exist:

| Gap | Expected Test Location | Test Name Pattern |
|-----|----------------------|-------------------|
| XREADGROUP blocking (direct XADD) | test_streams.py | test_xreadgroup_block_returns_new_entries |
| XREADGROUP blocking (Lua XADD) | test_streams.py | test_xreadgroup_block_lua_xadd_wakes_reader |
| XREADGROUP blocking timeout | test_streams.py | test_xreadgroup_block_timeout_returns_empty |
| XCLAIM ownership transfer | test_streams.py | test_xclaim_transfers_ownership |
| XCLAIM idle reset (pydocket lease renewal) | test_streams.py | test_xclaim_resets_idle_time |
| XCLAIM min_idle_time filtering | test_streams.py | test_xclaim_respects_min_idle_time |
| XCLAIM justid mode | test_streams.py | test_xclaim_justid_returns_ids_only |
| XTRIM approximate parameter | test_streams.py | test_xtrim_accepts_approximate_parameter |
| Pydocket delayed task delivery | test_pydocket_compat.py | test_docket_add_delayed_task (no xfail) |

If any test is missing from Plan 01, add it now.

**Step 2: Add integration-level regression tests for pydocket-specific scenarios (D-10).**

Add to tests/test_pydocket_compat.py to cover the specific patterns pydocket uses that were broken before:

```python
async def test_pydocket_lease_renewal_pattern(patch_pydocket, burner):
    """Regression: pydocket uses XCLAIM for lease renewal (same consumer, idle=0).

    This is the pattern from docket/worker.py _renew_leases() method.
    The consumer xclaims its own messages to reset idle time, preventing
    XAUTOCLAIM from reclaiming them during long-running tasks.
    """
    # Setup: create stream with consumer group and deliver a message
    await burner.xadd("test:stream", {"task": "data"})
    await burner.xgroup_create("test:stream", "workers", id="0")
    result = await burner.xreadgroup("workers", "worker-1", {"test:stream": ">"})
    assert len(result) > 0
    msg_id = result[0][1][0][0]  # First stream, first entry, ID

    # Lease renewal: same consumer claims its own message with idle=0
    claimed = await burner.xclaim("test:stream", "workers", "worker-1", 0, [msg_id], idle=0)
    assert len(claimed) == 1

    # After renewal, XAUTOCLAIM should NOT reclaim (idle was just reset)
    autoclaim_result = await burner.xautoclaim("test:stream", "workers", "worker-2", 1000, start_id="0-0")
    next_id, autoclaimed, deleted = autoclaim_result
    assert len(autoclaimed) == 0  # Nothing idle enough to claim


async def test_pydocket_delayed_task_pattern(patch_pydocket, burner):
    """Regression: the scheduler atomically moves tasks from sorted set to stream.

    This test simulates the exact pattern that caused the delayed task race:
    1. Lua script does ZRANGEBYSCORE + ZREM + XADD (scheduler finds due task)
    2. Worker does XREADGROUP with block (waits for new stream entries)
    3. Worker should receive the entry added by Lua
    """
    import asyncio

    # Setup stream and consumer group
    await burner.xadd("test:stream", {"init": "setup"})
    await burner.xgroup_create("test:stream", "workers", id="$")

    # Simulate scheduler: Lua script adds an entry to the stream
    lua_scheduler = burner.register_script("""
    redis.call('XADD', KEYS[1], '*', 'task', ARGV[1])
    return 1
    """)

    async def scheduler():
        await asyncio.sleep(0.05)
        await lua_scheduler(keys=["test:stream"], args=["delayed-payload"])

    # Worker: blocking read should see the Lua-added entry
    task = asyncio.create_task(scheduler())
    result = await burner.xreadgroup("workers", "worker-1", {"test:stream": ">"}, block=2000)
    await task

    assert len(result) > 0
    stream_name, entries = result[0]
    assert entries[0][1][b"task"] == b"delayed-payload"
```

**Step 3: Add any additional regression tests for gaps discovered in Task 1.**

If Task 1 discovered and fixed additional gaps beyond XCLAIM/block/approximate, add targeted regression tests here for each one.

**Step 4: Run the complete test suite to verify everything is green.**

Run:
```bash
# Unit tests
.venv/bin/python -m pytest tests/ -q -m "not integration" -x --tb=short
# Integration tests
.venv/bin/python -m pytest tests/ -q -m integration -x --tb=short
# Verify no xfails remain
grep -r "xfail" tests/ || echo "No xfails found"
```

All must pass. No xfails should exist in the test suite.
  </action>
  <verify>
    <automated>.venv/bin/python -m pytest tests/ -q --tb=short -x && .venv/bin/python -m pytest tests/ -q -m integration --tb=short -x</automated>
  </verify>
  <acceptance_criteria>
    - tests/test_pydocket_compat.py contains `test_pydocket_lease_renewal_pattern`
    - tests/test_pydocket_compat.py contains `test_pydocket_delayed_task_pattern`
    - `grep -r "xfail" tests/` returns no matches
    - `.venv/bin/python -m pytest tests/ -q -m "not integration" -x` exits 0
    - `.venv/bin/python -m pytest tests/ -q -m integration -x` exits 0
    - Every gap fixed in Phase 11 has at least one dedicated test
  </acceptance_criteria>
  <done>
    Complete regression test coverage for all Phase 11 fixes. Zero xfails in the entire test suite. Both unit and integration test suites pass fully.
  </done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|----------|-------------|
| Test fixture -> BurnerRedis | Test monkey-patches inject BurnerRedis into pydocket's Redis connection path |

## STRIDE Threat Register

| Threat ID | Category | Component | Disposition | Mitigation Plan |
|-----------|----------|-----------|-------------|-----------------|
| T-11-06 | T (Tampering) | Test fixture monkey-patch | accept | Test-only code path. Monkey-patching RedisConnection is intentional for testing. No production impact. |
| T-11-07 | D (Denial of Service) | pydocket test suite clone | accept | Temporary clone into /tmp for gap inventory. Cleaned up after use. No persistence. |
</threat_model>

<verification>
1. `.venv/bin/python -m pytest tests/test_pydocket_compat.py -m integration -v` -- all pydocket tests pass, no xfails
2. `.venv/bin/python -m pytest tests/ -q -m "not integration" -x` -- full unit test suite passes
3. `grep -r "xfail" tests/` -- returns empty (no xfails anywhere)
4. Each gap fixed in Phase 11 has a corresponding test that would fail if the fix were reverted
</verification>

<success_criteria>
- Zero xfails in the entire test suite
- All pydocket integration tests pass (test_docket_add_immediate_task, test_docket_add_delayed_task, test_docket_cancel_task, test_docket_snapshot, test_worker_heartbeat)
- Regression tests for lease renewal pattern (XCLAIM) and delayed task pattern (blocking XREADGROUP + Lua XADD)
- Any additional gaps discovered during pydocket test suite inventory have been fixed and tested
- Full unit + integration test suites pass
</success_criteria>

<output>
After completion, create `.planning/phases/11-close-redis-py-compatibility-gaps-for-pydocket-integration/11-02-SUMMARY.md`
</output>
