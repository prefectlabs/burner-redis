# T03: Implement message recovery (XAUTOCLAIM) and introspection (XINFO GROUPS, XINFO CONSUMERS) commands for streams.

**Slice:** S05 — **Milestone:** M001

## Description

Implement message recovery (XAUTOCLAIM) and introspection (XINFO GROUPS, XINFO CONSUMERS) commands for streams.

Purpose: XAUTOCLAIM enables Prefect to recover messages from failed/stalled consumers without manual intervention. XINFO commands enable monitoring and debugging of consumer group state. Together they complete the consumer group subsystem.

Output: Working XAUTOCLAIM and XINFO commands with full Python test coverage, completing all 11 stream requirements.

## Legacy Source

---
phase: 05-stream-commands-and-consumer-groups
plan: 03
type: execute
wave: 3
depends_on: ["05-02"]
files_modified:
  - src/store.rs
  - src/lib.rs
  - tests/test_streams.py
autonomous: true
requirements:
  - STRM-09
  - STRM-10
  - STRM-11

must_haves:
  truths:
    - "User can XAUTOCLAIM to reclaim idle pending messages from other consumers"
    - "User can XINFO GROUPS to see all consumer groups on a stream with their state"
    - "User can XINFO CONSUMERS to see all consumers in a group with pending count and idle time"
  artifacts:
    - path: "src/store.rs"
      provides: "xautoclaim, xinfo_groups, xinfo_consumers store methods"
      contains: "pub fn xautoclaim"
    - path: "src/lib.rs"
      provides: "Python async bindings for xautoclaim, xinfo_groups, xinfo_consumers"
      contains: "fn xautoclaim"
    - path: "tests/test_streams.py"
      provides: "Pytest tests covering STRM-09 through STRM-11"
      contains: "test_xautoclaim"
  key_links:
    - from: "src/lib.rs"
      to: "src/store.rs"
      via: "store.xautoclaim(), store.xinfo_groups(), store.xinfo_consumers()"
      pattern: "store\\.x(autoclaim|info)"
    - from: "tests/test_streams.py"
      to: "src/lib.rs"
      via: "await r.xautoclaim(), await r.xinfo_groups(), await r.xinfo_consumers()"
      pattern: "await r\\.x(autoclaim|info)"
---

<objective>
Implement message recovery (XAUTOCLAIM) and introspection (XINFO GROUPS, XINFO CONSUMERS) commands for streams.

Purpose: XAUTOCLAIM enables Prefect to recover messages from failed/stalled consumers without manual intervention. XINFO commands enable monitoring and debugging of consumer group state. Together they complete the consumer group subsystem.

Output: Working XAUTOCLAIM and XINFO commands with full Python test coverage, completing all 11 stream requirements.
</objective>

<execution_context>
@$HOME/.claude/get-shit-done/workflows/execute-plan.md
@$HOME/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@.planning/PROJECT.md
@.planning/ROADMAP.md
@.planning/STATE.md
@.planning/phases/05-stream-commands-and-consumer-groups/05-02-SUMMARY.md
@src/store.rs
@src/lib.rs
@src/commands/streams.rs
@tests/test_streams.py

<interfaces>
<!-- From Plans 01 and 02 (will exist when this executes) -->

From src/store.rs (Stream + ConsumerGroup structures):
```rust
pub struct Stream {
    pub entries: BTreeMap<StreamId, HashMap<Bytes, Bytes>>,
    pub last_id: StreamId,
    pub groups: HashMap<Bytes, ConsumerGroup>,
}

pub struct ConsumerGroup {
    pub last_delivered_id: StreamId,
    pub consumers: HashMap<Bytes, Consumer>,
}

pub struct Consumer {
    pub pending: HashMap<StreamId, PendingEntry>,
}

pub struct PendingEntry {
    pub delivery_time: Instant,
    pub delivery_count: u64,
}

pub enum StoreError {
    WrongType,
    NoGroup(String, String),
    BusyGroup,
    KeyNotFound,
}
```

From src/commands/streams.rs:
```rust
pub type StreamId = (u64, u64);
pub fn format_stream_id(id: StreamId) -> String;
pub fn parse_stream_id(s: &str) -> Option<StreamId>;
```
</interfaces>
</context>

<tasks>

<task type="auto">
  <name>Task 1: Implement XAUTOCLAIM and XINFO Rust store methods</name>
  <files>src/store.rs</files>
  <read_first>src/store.rs, src/commands/streams.rs</read_first>
  <action>
Add these methods to the Store impl in `src/store.rs`:

1. `pub fn xautoclaim(&self, key: &Bytes, group: &Bytes, consumer: Bytes, min_idle_time_ms: u64, start: StreamId, count: Option<usize>) -> Result<(StreamId, Vec<(StreamId, HashMap<Bytes, Bytes>)>, Vec<StreamId>), StoreError>`:
   - Acquire write lock. Passive expiration check.
   - If key doesn't exist or expired, return NoGroup error. WrongType if not Stream.
   - Get ConsumerGroup or return NoGroup.
   - Scan ALL consumers' PELs for entries with delivery_time elapsed >= min_idle_time_ms AND entry_id >= start.
   - Collect qualifying entries up to count (default: all if None). Sort by StreamId.
   - For each claimed entry:
     - Remove from original consumer's PEL.
     - Add to claiming consumer's PEL with delivery_time = Instant::now(), delivery_count = old_count + 1.
     - If entry still exists in stream.entries, include in claimed results.
     - If entry does NOT exist in stream.entries (was trimmed), add to deleted_ids list.
   - Auto-create the claiming consumer in group.consumers if not present.
   - Determine next_start_id: if all qualifying entries processed, use (0, 0) to signal completion. Otherwise use the next unprocessed entry's ID.
   - Return (next_start_id, claimed_entries_with_data, deleted_ids).

2. `pub fn xinfo_groups(&self, key: &Bytes) -> Result<Vec<HashMap<String, String>>, StoreError>`:
   - Acquire write lock. Passive expiration check.
   - If key doesn't exist, return Ok(empty vec). WrongType if not Stream.
   - For each group in stream.groups, build a HashMap with:
     - "name" -> group name (String from Bytes via UTF-8)
     - "consumers" -> count of consumers (as string)
     - "pending" -> total pending entries across all consumers (as string)
     - "last-delivered-id" -> formatted StreamId string
   - Return vec of group info maps.

3. `pub fn xinfo_consumers(&self, key: &Bytes, group: &Bytes) -> Result<Vec<HashMap<String, String>>, StoreError>`:
   - Acquire write lock. Passive expiration check.
   - If key doesn't exist or expired, return NoGroup error. WrongType if not Stream.
   - Get ConsumerGroup or return NoGroup.
   - For each consumer in group.consumers, build a HashMap with:
     - "name" -> consumer name (String from Bytes via UTF-8)
     - "pending" -> count of entries in consumer's PEL (as string)
     - "idle" -> milliseconds since last delivery_time in their PEL (most recent entry). If PEL empty, use 0. (as string)
   - Return vec of consumer info maps.
  </action>
  <verify>
    <automated>cd /Users/desertaxle/dev/prefectlabs/burner-redis && cargo build 2>&1 | tail -5</automated>
  </verify>
  <acceptance_criteria>
    - grep -q "pub fn xautoclaim" src/store.rs
    - grep -q "pub fn xinfo_groups" src/store.rs
    - grep -q "pub fn xinfo_consumers" src/store.rs
    - grep -q "min_idle_time_ms" src/store.rs
    - grep -q "next_start_id\|next_id" src/store.rs
    - cargo build succeeds with no errors
  </acceptance_criteria>
  <done>XAUTOCLAIM scans PELs for idle messages, transfers ownership to claiming consumer, increments delivery count, and separates deleted entries. XINFO GROUPS returns group metadata. XINFO CONSUMERS returns per-consumer pending count and idle time. All compile without errors.</done>
</task>

<task type="auto">
  <name>Task 2: Python bindings and tests for XAUTOCLAIM and XINFO</name>
  <files>src/lib.rs, tests/test_streams.py</files>
  <read_first>src/lib.rs, src/store.rs, src/commands/streams.rs, tests/test_streams.py</read_first>
  <action>
1. Add Python methods to BurnerRedis in `src/lib.rs`:

   `#[pyo3(signature = (name, groupname, consumername, min_idle_time, start_id="0-0", count=None))]`
   `fn xautoclaim<'py>(&self, py: Python<'py>, name: &Bound<'py, PyAny>, groupname: &Bound<'py, PyAny>, consumername: &Bound<'py, PyAny>, min_idle_time: u64, start_id: &str, count: Option<usize>) -> PyResult<Bound<'py, PyAny>>`:
   - Extract name, groupname, consumername as Bytes. Parse start_id via parse_stream_id (treat "0" as (0,0)).
   - Call store.xautoclaim(&key, &group, consumer, min_idle_time, start, count).
   - Return as Python tuple: (next_id_bytes, [(id_bytes, {field: value}), ...], [deleted_id_bytes, ...]). Use Python::try_attach to construct the tuple.
   - Matches redis-py return format: tuple of (next_start_id, claimed_entries, deleted_ids).

   `#[pyo3(signature = (name, type="groups", group=None))]`
   OR better, use two separate methods matching redis-py's pattern:

   Actually, redis-py uses `xinfo_groups(name)` and `xinfo_consumers(name, groupname)` as separate methods. Follow that pattern:

   `fn xinfo_groups<'py>(&self, py: Python<'py>, name: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>>`:
   - Extract name. Call store.xinfo_groups.
   - Return as Python list of dicts. Each dict has bytes keys and bytes/int values matching redis-py: {b"name": b"groupname", b"consumers": int, b"pending": int, b"last-delivered-id": b"id-string"}.
   - Use Python::try_attach to build list of PyDicts.

   `fn xinfo_consumers<'py>(&self, py: Python<'py>, name: &Bound<'py, PyAny>, groupname: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>>`:
   - Extract name and groupname. Call store.xinfo_consumers.
   - Return as Python list of dicts: {b"name": b"consumer", b"pending": int, b"idle": int}.
   - Use Python::try_attach to build list of PyDicts.

2. Append tests to `tests/test_streams.py`:

   STRM-09 (XAUTOCLAIM):
   - `test_xautoclaim_claims_idle_messages`: Consumer A reads messages, wait or set idle time, Consumer B xautoclaim reclaims them. Verify claimed entries returned with field data.
   - `test_xautoclaim_increments_delivery_count`: After autoclaim, the delivery count for the message increases.
   - `test_xautoclaim_returns_deleted_ids`: If a pending message was trimmed from the stream, it appears in deleted_ids list.
   - `test_xautoclaim_respects_min_idle_time`: Messages not idle long enough are NOT claimed.
   - `test_xautoclaim_respects_count`: count parameter limits how many messages are claimed.
   - `test_xautoclaim_returns_next_start_id`: When not all idle messages are claimed (due to count), next_start_id indicates where to continue.

   For testing idle time: since Instant-based tracking makes real-time tests flaky, use min_idle_time=0 to claim immediately (tests that anything pending is claimable). For the "respects min_idle_time" test, use a large value like 999999 and verify nothing is claimed.

   STRM-10 (XINFO GROUPS):
   - `test_xinfo_groups_returns_group_info`: Create group, add entries, verify xinfo_groups returns correct metadata.
   - `test_xinfo_groups_multiple_groups`: Create 2 groups, verify both returned.
   - `test_xinfo_groups_empty_stream`: XINFO GROUPS on stream with no groups returns empty list.
   - `test_xinfo_groups_pending_count`: After XREADGROUP without XACK, pending count is accurate.

   STRM-11 (XINFO CONSUMERS):
   - `test_xinfo_consumers_returns_consumer_info`: After XREADGROUP, xinfo_consumers shows the consumer with pending count.
   - `test_xinfo_consumers_multiple_consumers`: Two consumers read, both appear in info.
   - `test_xinfo_consumers_after_ack`: After XACK, consumer's pending count decreases.
   - `test_xinfo_consumers_nogroup_error`: XINFO CONSUMERS on non-existent group raises error.
  </action>
  <verify>
    <automated>cd /Users/desertaxle/dev/prefectlabs/burner-redis && maturin develop 2>&1 | tail -3 && python -m pytest tests/test_streams.py -x -v 2>&1 | tail -40</automated>
  </verify>
  <acceptance_criteria>
    - grep -q "fn xautoclaim" src/lib.rs
    - grep -q "fn xinfo_groups" src/lib.rs
    - grep -q "fn xinfo_consumers" src/lib.rs
    - grep -q "test_xautoclaim_claims_idle_messages" tests/test_streams.py
    - grep -q "test_xinfo_groups_returns_group_info" tests/test_streams.py
    - grep -q "test_xinfo_consumers_returns_consumer_info" tests/test_streams.py
    - grep -q "STRM-09" tests/test_streams.py
    - grep -q "STRM-10" tests/test_streams.py
    - grep -q "STRM-11" tests/test_streams.py
    - python -m pytest tests/test_streams.py passes
  </acceptance_criteria>
  <done>All 11 stream requirements complete. XAUTOCLAIM reclaims idle messages with correct delivery count tracking and deleted ID reporting. XINFO GROUPS returns group metadata (name, consumers, pending, last-delivered-id). XINFO CONSUMERS returns per-consumer state (name, pending, idle). Full test suite passes covering all stream commands.</done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|----------|-------------|
| Python -> Rust | min_idle_time, start_id, count parameters from user code |

## STRIDE Threat Register

| Threat ID | Category | Component | Disposition | Mitigation Plan |
|-----------|----------|-----------|-------------|-----------------|
| T-05-07 | Tampering | XAUTOCLAIM start_id | mitigate | Validate StreamId format; reject malformed with PyValueError |
| T-05-08 | Information Disclosure | XINFO idle time | accept | Idle time reveals when consumers last read; expected operational visibility |
| T-05-09 | Denial of Service | XAUTOCLAIM scan | accept | Scans all PELs linearly; acceptable for in-process use. Could optimize with sorted PEL if needed in future. |
</threat_model>

<verification>
1. `cargo build` compiles without errors
2. `maturin develop` installs the updated package
3. `python -m pytest tests/test_streams.py -v` ALL stream tests pass (STRM-01 through STRM-11)
4. XAUTOCLAIM correctly transfers pending messages between consumers
5. XINFO commands return accurate group and consumer metadata
</verification>

<success_criteria>
- XAUTOCLAIM claims idle messages, increments delivery count, reports deleted entries
- XINFO GROUPS returns accurate group-level metadata for all groups on a stream
- XINFO CONSUMERS returns per-consumer pending count and idle time
- All 11 stream requirements (STRM-01 through STRM-11) fully implemented and tested
- Complete pytest suite passes with no failures
</success_criteria>

<output>
After completion, create `.planning/phases/05-stream-commands-and-consumer-groups/05-03-SUMMARY.md`
</output>
