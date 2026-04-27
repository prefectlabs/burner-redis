# Phase 5: Stream Commands and Consumer Groups - Context

**Gathered:** 2026-04-11
**Status:** Ready for planning

<domain>
## Phase Boundary

Implement Redis Streams with consumer groups: XADD, XREAD, XREADGROUP, XLEN, XACK, XAUTOCLAIM, XTRIM, XINFO GROUPS, XINFO CONSUMERS, XGROUP CREATE, XGROUP DESTROY. This is Prefect's entire messaging subsystem.

</domain>

<decisions>
## Implementation Decisions

### Stream Entry Model
- Store entries in `BTreeMap<StreamId, HashMap<Bytes, Bytes>>` where StreamId is `(u64, u64)` (ms timestamp + sequence) — ordered by insertion time.
- Auto-generated IDs use current system time in milliseconds + auto-incrementing sequence. If time goes backward, use `last_id.ms` with `last_id.seq + 1` to maintain monotonicity.
- XADD returns the generated stream ID as `bytes` (e.g., `b"1234567890123-0"`) — matching redis-py.
- XTRIM supports both MAXLEN and MINID strategies. MAXLEN trims to N entries, MINID removes entries with ID < minid.

### Consumer Group Model
- Per-group: `last_delivered_id: StreamId` tracking the last ID delivered to any consumer.
- Per-consumer: `HashMap<StreamId, PendingEntry>` (PEL). PendingEntry tracks `delivery_time: Instant` and `delivery_count: u64`.
- XREADGROUP with `>` delivers entries with ID > group's `last_delivered_id`, advances `last_delivered_id`, and adds each entry to the consumer's PEL.
- XREADGROUP with explicit ID (e.g., `"0"`) returns pending entries for that consumer.
- XAUTOCLAIM claims messages idle longer than `min_idle_time` from any consumer's PEL, transfers to claiming consumer's PEL, resets idle time. Returns `(next_start_id, claimed_entries, deleted_ids)`.
- XACK removes entries from the consumer's PEL. Returns count of actually acknowledged entries.
- XINFO GROUPS returns list of dicts: `{name, consumers, pending, last-delivered-id}`.
- XINFO CONSUMERS returns list of dicts: `{name, pending, idle}`.
- XGROUP CREATE creates a new consumer group with the specified start ID (`$` = latest, `0` = beginning).
- XGROUP DESTROY removes a consumer group entirely.

### Data Structure
- Stream stored as new `ValueData::Stream` variant containing:
  - `entries: BTreeMap<StreamId, HashMap<Bytes, Bytes>>`
  - `last_id: StreamId` (for auto-increment)
  - `groups: HashMap<Bytes, ConsumerGroup>`
- ConsumerGroup struct:
  - `last_delivered_id: StreamId`
  - `consumers: HashMap<Bytes, Consumer>`
- Consumer struct:
  - `pending: HashMap<StreamId, PendingEntry>`
- PendingEntry struct:
  - `delivery_time: Instant`
  - `delivery_count: u64`

### Claude's Discretion
No items deferred to Claude's discretion — all questions resolved.

</decisions>

<code_context>
## Existing Code Insights

### Reusable Assets
- `src/store.rs` — Store engine with `ValueData` enum, `StoreError::WrongType`, RwLock pattern.
- `src/commands/strings.rs` — `extract_bytes` helper.
- `src/lib.rs` — BurnerRedis with `Arc<Store>`, `future_into_py`, `store_err_to_py`.
- `python/burner_redis/__init__.py` — ResponseError exception class.

### Established Patterns
- All commands async via `future_into_py` with `Arc<Store>` clone.
- Store methods return `Result<T, StoreError>`.
- One pytest file per command group.
- Accept both str/bytes for keys.

### Integration Points
- `src/store.rs` ValueData enum needs Stream variant.
- `src/lib.rs` needs ~11 new `#[pymethods]`.
- New `src/commands/streams.rs` module for helpers (ID parsing, etc.).
- New `tests/test_streams.py` file.

</code_context>

<specifics>
## Specific Ideas

No specific requirements — follow established patterns and redis-py compatibility.

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope.

</deferred>
