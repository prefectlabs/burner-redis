# T02: Implement consumer group core operations: creating/destroying groups, reading as a consumer with PEL tracking, and acknowledging messages.

**Slice:** S05 — **Milestone:** M001

## Description

Implement consumer group core operations: creating/destroying groups, reading as a consumer with PEL tracking, and acknowledging messages.

Purpose: Consumer groups are how Prefect distributes work across consumers with at-least-once delivery guarantees. This plan implements the read-process-ack loop that forms the heart of the messaging subsystem.

Output: Working XGROUP CREATE/DESTROY, XREADGROUP, and XACK commands with full Python test coverage.

## Legacy Source

---
phase: 05-stream-commands-and-consumer-groups
plan: 02
type: execute
wave: 2
depends_on: ["05-01"]
files_modified:
  - src/store.rs
  - src/lib.rs
  - tests/test_streams.py
autonomous: true
requirements:
  - STRM-05
  - STRM-06
  - STRM-07
  - STRM-08

must_haves:
  truths:
    - "User can XGROUP CREATE to create a consumer group on a stream"
    - "User can XGROUP DESTROY to remove a consumer group"
    - "User can XREADGROUP to read new messages as a consumer in a group"
    - "User can XACK to acknowledge processed messages and remove them from PEL"
  artifacts:
    - path: "src/store.rs"
      provides: "xgroup_create, xgroup_destroy, xreadgroup, xack store methods"
      contains: "pub fn xgroup_create"
    - path: "src/lib.rs"
      provides: "Python async bindings for xgroup_create, xgroup_destroy, xreadgroup, xack"
      contains: "fn xgroup_create"
    - path: "tests/test_streams.py"
      provides: "Pytest tests covering STRM-05 through STRM-08"
      contains: "test_xgroup_create"
  key_links:
    - from: "src/lib.rs"
      to: "src/store.rs"
      via: "store.xgroup_create(), store.xreadgroup(), store.xack()"
      pattern: "store\\.x(group|readgroup|ack)"
    - from: "tests/test_streams.py"
      to: "src/lib.rs"
      via: "await r.xgroup_create(), await r.xreadgroup(), await r.xack()"
      pattern: "await r\\.x(group_create|readgroup|ack)"
---

<objective>
Implement consumer group core operations: creating/destroying groups, reading as a consumer with PEL tracking, and acknowledging messages.

Purpose: Consumer groups are how Prefect distributes work across consumers with at-least-once delivery guarantees. This plan implements the read-process-ack loop that forms the heart of the messaging subsystem.

Output: Working XGROUP CREATE/DESTROY, XREADGROUP, and XACK commands with full Python test coverage.
</objective>

<execution_context>
@$HOME/.claude/get-shit-done/workflows/execute-plan.md
@$HOME/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@.planning/PROJECT.md
@.planning/ROADMAP.md
@.planning/STATE.md
@.planning/phases/05-stream-commands-and-consumer-groups/05-01-SUMMARY.md
@src/store.rs
@src/lib.rs
@src/commands/streams.rs
@tests/test_streams.py

<interfaces>
<!-- From Plan 01 (will exist when this executes) -->

From src/store.rs (Stream structures):
```rust
pub type StreamId = (u64, u64); // imported from commands::streams

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
  <name>Task 1: Implement consumer group Rust store methods</name>
  <files>src/store.rs</files>
  <read_first>src/store.rs, src/commands/streams.rs</read_first>
  <action>
Add these methods to the Store impl in `src/store.rs`. Also add a new StoreError variant for stream-specific errors.

1. Extend `StoreError` enum with:
   - `#[error("NOGROUP No such consumer group '{0}' for key name '{1}'")] NoGroup(String, String)` — for when group doesn't exist
   - `#[error("BUSYGROUP Consumer Group name already exists")] BusyGroup` — for duplicate group creation

   Update `store_err_to_py` in `src/lib.rs` to handle new variants (both map to PyException with the error message string, same pattern as WrongType).

2. Add Store methods:

   `pub fn xgroup_create(&self, key: &Bytes, group: Bytes, id: StreamId, mkstream: bool) -> Result<(), StoreError>`:
   - Acquire write lock. Passive expiration check.
   - If key doesn't exist: if mkstream is true, create an empty Stream. Otherwise return Err — use a new variant or just return WrongType (since redis returns ERR for missing key without MKSTREAM). Actually: add `#[error("ERR The XGROUP subcommand requires the key to exist")] KeyNotFound` variant.
   - WrongType if not Stream.
   - If group name already exists in stream.groups, return Err(BusyGroup).
   - Insert new ConsumerGroup with last_delivered_id = id, empty consumers map.
   - Return Ok(()).

   `pub fn xgroup_destroy(&self, key: &Bytes, group: &Bytes) -> Result<bool, StoreError>`:
   - Acquire write lock. Passive expiration check.
   - If key doesn't exist, return Ok(false). WrongType if not Stream.
   - Remove group from stream.groups. Return Ok(true) if existed, Ok(false) if not.

   `pub fn xreadgroup(&self, group: &Bytes, consumer: &Bytes, keys: &[Bytes], ids: &[String], count: Option<usize>) -> Result<Vec<(Bytes, Vec<(StreamId, HashMap<Bytes, Bytes>)>)>, StoreError>`:
   - Acquire write lock (needs write because we mutate PEL and last_delivered_id).
   - For each key:
     - Passive expiration check. If key doesn't exist or expired, return NoGroup error. WrongType if not Stream.
     - Get the ConsumerGroup by name or return NoGroup.
     - Parse the id string: if ">" then deliver NEW messages (entries after group.last_delivered_id). Otherwise parse as StreamId and return pending entries for this consumer with id >= parsed id.
     - For ">" delivery:
       - Collect entries where entry_id > group.last_delivered_id. Apply count limit.
       - For each delivered entry: update group.last_delivered_id to the entry's ID. Add to consumer's PEL with delivery_time = Instant::now(), delivery_count = 1. Auto-create consumer in group.consumers if not present.
     - For explicit ID (e.g., "0" means return all pending):
       - Parse id (treat "0" as (0,0)). Look at consumer's PEL, return entries from stream that match IDs in PEL >= parsed id. Apply count limit.
   - Return vec of (key, entries) — same format as xread. Skip keys with no results (include only if entries found).

   `pub fn xack(&self, key: &Bytes, group: &Bytes, ids: &[StreamId]) -> Result<i64, StoreError>`:
   - Acquire write lock. Passive expiration check.
   - If key doesn't exist, return Ok(0). WrongType if not Stream.
   - Get ConsumerGroup or return Ok(0) if group doesn't exist (Redis returns 0, not error).
   - For each id in ids: iterate all consumers in the group, remove the id from their PEL. Count successful removals.
   - Return count of acknowledged entries.
  </action>
  <verify>
    <automated>cd /Users/desertaxle/dev/prefectlabs/burner-redis && cargo build 2>&1 | tail -5</automated>
  </verify>
  <acceptance_criteria>
    - grep -q "pub fn xgroup_create" src/store.rs
    - grep -q "pub fn xgroup_destroy" src/store.rs
    - grep -q "pub fn xreadgroup" src/store.rs
    - grep -q "pub fn xack" src/store.rs
    - grep -q "NoGroup" src/store.rs
    - grep -q "BusyGroup" src/store.rs
    - cargo build succeeds with no errors
  </acceptance_criteria>
  <done>Consumer group store methods compile. XGROUP CREATE/DESTROY manage groups on streams. XREADGROUP delivers new entries or returns pending entries, tracking PEL with delivery time and count. XACK removes entries from consumer PELs.</done>
</task>

<task type="auto">
  <name>Task 2: Python bindings and tests for consumer group commands</name>
  <files>src/lib.rs, tests/test_streams.py</files>
  <read_first>src/lib.rs, src/store.rs, src/commands/streams.rs, tests/test_streams.py</read_first>
  <action>
1. Update `store_err_to_py` in `src/lib.rs` to handle the new StoreError variants:
   - StoreError::NoGroup(group, key) -> PyException with the formatted error string
   - StoreError::BusyGroup -> PyException with the BUSYGROUP message
   - StoreError::KeyNotFound -> PyException with the ERR message

2. Add Python methods to BurnerRedis in `src/lib.rs`:

   `#[pyo3(signature = (name, groupname, id="$", mkstream=false))]`
   `fn xgroup_create<'py>(&self, py: Python<'py>, name: &Bound<'py, PyAny>, groupname: &Bound<'py, PyAny>, id: &str, mkstream: bool) -> PyResult<Bound<'py, PyAny>>`:
   - Extract name and groupname as Bytes.
   - Parse id: "$" means use stream's last_id (get it from store — or pass a sentinel). Better approach: parse "$" as (u64::MAX, u64::MAX) and handle in store method — if id == (u64::MAX, u64::MAX) then use stream.last_id. OR: pass id as a string to the store method and let store parse it. Simplest: parse "$" to mean "latest" — in store, if we receive (u64::MAX, u64::MAX) treat it as stream.last_id. For "0" or "0-0", parse as (0,0).
   - Call store.xgroup_create. Return True on success (matches redis-py).

   `#[pyo3(signature = (name, groupname))]`
   `fn xgroup_destroy<'py>(&self, py: Python<'py>, name: &Bound<'py, PyAny>, groupname: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>>`:
   - Extract name and groupname. Call store.xgroup_destroy. Return i64 (1 if destroyed, 0 if not).

   `#[pyo3(signature = (groupname, consumername, streams, count=None))]`
   `fn xreadgroup<'py>(&self, py: Python<'py>, groupname: &Bound<'py, PyAny>, consumername: &Bound<'py, PyAny>, streams: &Bound<'py, PyDict>, count: Option<usize>) -> PyResult<Bound<'py, PyAny>>`:
   - Extract groupname and consumername as Bytes.
   - Extract streams dict: keys are stream names (Bytes), values are ID strings (">", "0", or specific ID).
   - Call store.xreadgroup(&group, &consumer, &keys, &id_strs, count).
   - Return same nested list format as xread: [[stream_name, [(id_bytes, {field: value}), ...]], ...]. Return None if no results.

   `#[pyo3(signature = (name, groupname, *ids))]`
   `fn xack<'py>(&self, py: Python<'py>, name: &Bound<'py, PyAny>, groupname: &Bound<'py, PyAny>, ids: &Bound<'py, pyo3::types::PyTuple>) -> PyResult<Bound<'py, PyAny>>`:
   - Extract name and groupname as Bytes.
   - Parse each id in ids tuple as a string, then parse_stream_id to StreamId.
   - Call store.xack. Return count as i64.

3. Append tests to `tests/test_streams.py`:

   STRM-05 (XGROUP CREATE):
   - `test_xgroup_create_on_existing_stream`: Create stream with XADD, then XGROUP CREATE succeeds (returns True)
   - `test_xgroup_create_mkstream`: XGROUP CREATE with mkstream=True on non-existent key succeeds
   - `test_xgroup_create_no_mkstream`: XGROUP CREATE without mkstream on missing key raises error
   - `test_xgroup_create_duplicate`: Creating same group twice raises BUSYGROUP error
   - `test_xgroup_create_dollar_id`: Creating group with "$" means it starts at the latest entry

   STRM-06 (XGROUP DESTROY):
   - `test_xgroup_destroy_existing`: XGROUP DESTROY returns 1 for existing group
   - `test_xgroup_destroy_nonexistent`: XGROUP DESTROY returns 0 for non-existent group

   STRM-07 (XREADGROUP):
   - `test_xreadgroup_new_messages`: After XADD, XREADGROUP with ">" returns new entries
   - `test_xreadgroup_advances_delivery`: After reading, subsequent ">" returns only newer entries
   - `test_xreadgroup_pending_with_zero`: XREADGROUP with "0" returns pending (unacked) entries
   - `test_xreadgroup_empty_after_ack`: After XACK, "0" returns empty for that consumer
   - `test_xreadgroup_count_limit`: count parameter limits returned entries
   - `test_xreadgroup_nogroup_error`: XREADGROUP on non-existent group raises NOGROUP error

   STRM-08 (XACK):
   - `test_xack_removes_from_pel`: After XREADGROUP and XACK, message no longer pending
   - `test_xack_returns_count`: XACK returns number of actually acknowledged messages
   - `test_xack_idempotent`: ACKing already-acked message returns 0
   - `test_xack_nonexistent_stream`: XACK on missing stream returns 0
  </action>
  <verify>
    <automated>cd /Users/desertaxle/dev/prefectlabs/burner-redis && maturin develop 2>&1 | tail -3 && python -m pytest tests/test_streams.py -x -v 2>&1 | tail -40</automated>
  </verify>
  <acceptance_criteria>
    - grep -q "fn xgroup_create" src/lib.rs
    - grep -q "fn xgroup_destroy" src/lib.rs
    - grep -q "fn xreadgroup" src/lib.rs
    - grep -q "fn xack" src/lib.rs
    - grep -q "test_xgroup_create_on_existing_stream" tests/test_streams.py
    - grep -q "test_xgroup_destroy_existing" tests/test_streams.py
    - grep -q "test_xreadgroup_new_messages" tests/test_streams.py
    - grep -q "test_xack_removes_from_pel" tests/test_streams.py
    - grep -q "STRM-05" tests/test_streams.py
    - grep -q "STRM-06" tests/test_streams.py
    - grep -q "STRM-07" tests/test_streams.py
    - grep -q "STRM-08" tests/test_streams.py
    - python -m pytest tests/test_streams.py passes
  </acceptance_criteria>
  <done>Consumer group commands (XGROUP CREATE, XGROUP DESTROY, XREADGROUP, XACK) are fully functional from Python. Tests prove: groups can be created/destroyed, consumers read new messages via ">", pending entries tracked in PEL, XACK removes from PEL, NOGROUP/BUSYGROUP errors raised appropriately.</done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|----------|-------------|
| Python -> Rust | Group names, consumer names, stream IDs from user code |

## STRIDE Threat Register

| Threat ID | Category | Component | Disposition | Mitigation Plan |
|-----------|----------|-----------|-------------|-----------------|
| T-05-04 | Tampering | XREADGROUP ID parsing | mitigate | Validate ">" or valid StreamId format; reject malformed with PyValueError |
| T-05-05 | Denial of Service | PEL growth | accept | PEL grows with unacked messages; in-process library, user responsible for ACKing |
| T-05-06 | Spoofing | Consumer identity | accept | No auth boundary in embedded library; any consumer name accepted (matches Redis in-process) |
</threat_model>

<verification>
1. `cargo build` compiles without errors
2. `maturin develop` installs the updated package
3. `python -m pytest tests/test_streams.py -v` all tests pass
4. Consumer group lifecycle works: create -> read -> ack -> destroy
5. Error cases handled: NOGROUP, BUSYGROUP, WRONGTYPE
</verification>

<success_criteria>
- XGROUP CREATE creates consumer groups with configurable start position ($ or 0)
- XGROUP DESTROY removes groups entirely
- XREADGROUP delivers new messages with ">" and returns pending with "0"
- XACK removes messages from PEL, returns count acknowledged
- PEL correctly tracks delivery_time and delivery_count
- All error conditions properly raised as Python exceptions
</success_criteria>

<output>
After completion, create `.planning/phases/05-stream-commands-and-consumer-groups/05-02-SUMMARY.md`
</output>
