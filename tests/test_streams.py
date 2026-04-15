"""Tests for stream commands: XADD, XREAD, XLEN, XTRIM, XGROUP, XREADGROUP, XACK, XAUTOCLAIM, XINFO, XCLAIM.

Covers requirements: STRM-01, STRM-02, STRM-03, STRM-04, STRM-05, STRM-06, STRM-07, STRM-08, STRM-09, STRM-10, STRM-11, D-03, D-06, D-07, D-08.
"""
import asyncio
import time

import pytest
from burner_redis import BurnerRedis


# --- STRM-01: XADD ---


async def test_xadd_returns_id(r):
    """STRM-01: XADD returns bytes ID in 'ms-seq' format."""
    result = await r.xadd("mystream", {"field1": "value1"})
    assert isinstance(result, bytes)
    assert b"-" in result
    # Should be in format "ms-seq" where both are numeric
    parts = result.split(b"-")
    assert len(parts) == 2
    assert int(parts[0]) > 0
    assert int(parts[1]) >= 0


async def test_xadd_sequential_ids(r):
    """STRM-01: Two XADDs return different IDs, second >= first."""
    id1 = await r.xadd("mystream", {"f": "v1"})
    id2 = await r.xadd("mystream", {"f": "v2"})
    assert id1 != id2
    # Parse and compare: second ID should be greater
    ms1, seq1 = id1.split(b"-")
    ms2, seq2 = id2.split(b"-")
    assert (int(ms2), int(seq2)) > (int(ms1), int(seq1))


async def test_xadd_creates_stream(r):
    """STRM-01: After XADD, XLEN returns 1."""
    await r.xadd("mystream", {"key": "val"})
    length = await r.xlen("mystream")
    assert length == 1


async def test_xadd_multiple_fields(r):
    """STRM-01: XADD with multiple field-value pairs, XREAD returns all fields."""
    await r.xadd("mystream", {"f1": "v1", "f2": "v2", "f3": "v3"})
    result = await r.xread({"mystream": "0-0"})
    assert result is not None
    # result is [[stream_name, [(id, {field: value}), ...]]]
    stream_name, entries = result[0]
    assert stream_name == b"mystream"
    entry_id, fields = entries[0]
    assert fields[b"f1"] == b"v1"
    assert fields[b"f2"] == b"v2"
    assert fields[b"f3"] == b"v3"


async def test_xadd_bytes_input(r):
    """STRM-01: XADD works with bytes keys/fields."""
    entry_id = await r.xadd(b"mystream", {b"field": b"value"})
    assert isinstance(entry_id, bytes)
    assert b"-" in entry_id

    result = await r.xread({b"mystream": "0-0"})
    assert result is not None
    stream_name, entries = result[0]
    assert stream_name == b"mystream"
    _, fields = entries[0]
    assert fields[b"field"] == b"value"


async def test_xadd_wrongtype(r):
    """STRM-01: XADD on a string key raises WRONGTYPE."""
    await r.set("strkey", "value")
    with pytest.raises(Exception, match="WRONGTYPE"):
        await r.xadd("strkey", {"f": "v"})


# --- STRM-02: XREAD ---


async def test_xread_all_entries(r):
    """STRM-02: XREAD with id='0-0' returns all entries."""
    await r.xadd("mystream", {"f": "v1"})
    await r.xadd("mystream", {"f": "v2"})
    await r.xadd("mystream", {"f": "v3"})

    result = await r.xread({"mystream": "0-0"})
    assert result is not None
    stream_name, entries = result[0]
    assert stream_name == b"mystream"
    assert len(entries) == 3


async def test_xread_from_offset(r):
    """STRM-02: XREAD with a specific ID returns only entries after it."""
    id1 = await r.xadd("mystream", {"f": "v1"})
    await r.xadd("mystream", {"f": "v2"})
    await r.xadd("mystream", {"f": "v3"})

    # Read after the first entry
    result = await r.xread({"mystream": id1.decode()})
    assert result is not None
    stream_name, entries = result[0]
    assert len(entries) == 2  # Only v2 and v3


async def test_xread_multiple_streams(r):
    """STRM-02: XREAD from 2 streams returns both."""
    await r.xadd("stream1", {"f": "v1"})
    await r.xadd("stream2", {"f": "v2"})

    result = await r.xread({"stream1": "0-0", "stream2": "0-0"})
    assert result is not None
    assert len(result) == 2

    # Collect stream names
    stream_names = {entry[0] for entry in result}
    assert stream_names == {b"stream1", b"stream2"}


async def test_xread_count_limit(r):
    """STRM-02: XREAD with count=1 returns only 1 entry."""
    await r.xadd("mystream", {"f": "v1"})
    await r.xadd("mystream", {"f": "v2"})
    await r.xadd("mystream", {"f": "v3"})

    result = await r.xread({"mystream": "0-0"}, count=1)
    assert result is not None
    stream_name, entries = result[0]
    assert len(entries) == 1


async def test_xread_empty_stream(r):
    """STRM-02: XREAD on non-existent stream returns None."""
    result = await r.xread({"nonexistent": "0-0"})
    assert result is None


async def test_xread_returns_field_dict(r):
    """STRM-02: Each entry's fields are a dict with bytes keys/values."""
    await r.xadd("mystream", {"key1": "val1", "key2": "val2"})

    result = await r.xread({"mystream": "0-0"})
    assert result is not None
    stream_name, entries = result[0]
    entry_id, fields = entries[0]

    assert isinstance(entry_id, bytes)
    assert isinstance(fields, dict)
    for k, v in fields.items():
        assert isinstance(k, bytes)
        assert isinstance(v, bytes)


# --- STRM-03: XLEN ---


async def test_xlen_with_entries(r):
    """STRM-03: XLEN returns correct count after multiple XADDs."""
    await r.xadd("mystream", {"f": "v1"})
    await r.xadd("mystream", {"f": "v2"})
    await r.xadd("mystream", {"f": "v3"})

    length = await r.xlen("mystream")
    assert length == 3


async def test_xlen_empty(r):
    """STRM-03: XLEN on non-existent key returns 0."""
    length = await r.xlen("nonexistent")
    assert length == 0


async def test_xlen_wrongtype(r):
    """STRM-03: XLEN on a string key raises WRONGTYPE."""
    await r.set("strkey", "value")
    with pytest.raises(Exception, match="WRONGTYPE"):
        await r.xlen("strkey")


# --- STRM-04: XTRIM ---


async def test_xtrim_maxlen(r):
    """STRM-04: After adding 5 entries, XTRIM maxlen=3 removes 2, XLEN returns 3."""
    for i in range(5):
        await r.xadd("mystream", {"f": f"v{i}"})

    trimmed = await r.xtrim("mystream", maxlen=3)
    assert trimmed == 2

    length = await r.xlen("mystream")
    assert length == 3


async def test_xtrim_minid(r):
    """STRM-04: XTRIM with minid removes entries below that ID."""
    ids = []
    for i in range(5):
        entry_id = await r.xadd("mystream", {"f": f"v{i}"})
        ids.append(entry_id)

    # Trim entries below the 3rd entry ID (should remove first 2)
    minid_str = ids[2].decode()
    trimmed = await r.xtrim("mystream", minid=minid_str)
    assert trimmed == 2

    length = await r.xlen("mystream")
    assert length == 3


async def test_xtrim_nonexistent(r):
    """STRM-04: XTRIM on missing key returns 0."""
    trimmed = await r.xtrim("nonexistent", maxlen=5)
    assert trimmed == 0


async def test_xtrim_wrongtype(r):
    """STRM-04: XTRIM on a string key raises WRONGTYPE."""
    await r.set("strkey", "value")
    with pytest.raises(Exception, match="WRONGTYPE"):
        await r.xtrim("strkey", maxlen=5)


# --- STRM-05: XGROUP CREATE ---


async def test_xgroup_create_on_existing_stream(r):
    """STRM-05: XGROUP CREATE on an existing stream succeeds."""
    await r.xadd("mystream", {"f": "v"})
    result = await r.xgroup_create("mystream", "mygroup", id="0")
    assert result is True


async def test_xgroup_create_mkstream(r):
    """STRM-05: XGROUP CREATE with mkstream=True on non-existent key succeeds."""
    result = await r.xgroup_create("newstream", "mygroup", id="0", mkstream=True)
    assert result is True


async def test_xgroup_create_no_mkstream(r):
    """STRM-05: XGROUP CREATE without mkstream on missing key raises error."""
    with pytest.raises(Exception, match="XGROUP subcommand requires the key to exist"):
        await r.xgroup_create("nonexistent", "mygroup", id="0")


async def test_xgroup_create_duplicate(r):
    """STRM-05: Creating same group twice raises BUSYGROUP error."""
    await r.xadd("mystream", {"f": "v"})
    await r.xgroup_create("mystream", "mygroup", id="0")
    with pytest.raises(Exception, match="BUSYGROUP"):
        await r.xgroup_create("mystream", "mygroup", id="0")


async def test_xgroup_create_dollar_id(r):
    """STRM-05: Creating group with '$' means it starts at the latest entry."""
    # Add entries before creating the group
    await r.xadd("mystream", {"f": "v1"})
    await r.xadd("mystream", {"f": "v2"})

    # Create group at "$" (latest)
    await r.xgroup_create("mystream", "mygroup", id="$")

    # Reading with ">" should return nothing since group starts at latest
    result = await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"})
    assert not result  # None or empty list

    # Add a new entry after group creation
    await r.xadd("mystream", {"f": "v3"})

    # Now reading with ">" should return the new entry
    result = await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"})
    assert result is not None
    stream_name, entries = result[0]
    assert len(entries) == 1
    _, fields = entries[0]
    assert fields[b"f"] == b"v3"


# --- STRM-06: XGROUP DESTROY ---


async def test_xgroup_destroy_existing(r):
    """STRM-06: XGROUP DESTROY returns 1 for existing group."""
    await r.xadd("mystream", {"f": "v"})
    await r.xgroup_create("mystream", "mygroup", id="0")
    result = await r.xgroup_destroy("mystream", "mygroup")
    assert result == 1


async def test_xgroup_destroy_nonexistent(r):
    """STRM-06: XGROUP DESTROY returns 0 for non-existent group."""
    await r.xadd("mystream", {"f": "v"})
    result = await r.xgroup_destroy("mystream", "nogroup")
    assert result == 0


# --- STRM-07: XREADGROUP ---


async def test_xreadgroup_new_messages(r):
    """STRM-07: After XADD, XREADGROUP with '>' returns new entries."""
    await r.xadd("mystream", {"f": "v1"})
    await r.xadd("mystream", {"f": "v2"})
    await r.xgroup_create("mystream", "mygroup", id="0")

    result = await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"})
    assert result is not None
    stream_name, entries = result[0]
    assert stream_name == b"mystream"
    assert len(entries) == 2


async def test_xreadgroup_advances_delivery(r):
    """STRM-07: After reading, subsequent '>' returns only newer entries."""
    await r.xadd("mystream", {"f": "v1"})
    await r.xgroup_create("mystream", "mygroup", id="0")

    # First read gets v1
    result = await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"})
    assert result is not None
    assert len(result[0][1]) == 1

    # Second read with ">" should return nothing (no new entries)
    result = await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"})
    assert not result  # None or empty list

    # Add a new entry
    await r.xadd("mystream", {"f": "v2"})

    # Now ">" should return only v2
    result = await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"})
    assert result is not None
    assert len(result[0][1]) == 1
    _, fields = result[0][1][0]
    assert fields[b"f"] == b"v2"


async def test_xreadgroup_pending_with_zero(r):
    """STRM-07: XREADGROUP with '0' returns pending (unacked) entries."""
    await r.xadd("mystream", {"f": "v1"})
    await r.xadd("mystream", {"f": "v2"})
    await r.xgroup_create("mystream", "mygroup", id="0")

    # Read new messages (adds to PEL)
    await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"})

    # Read with "0" returns pending entries
    result = await r.xreadgroup("mygroup", "consumer1", {"mystream": "0"})
    assert result is not None
    stream_name, entries = result[0]
    assert len(entries) == 2


async def test_xreadgroup_empty_after_ack(r):
    """STRM-07: After XACK, '0' returns empty for that consumer."""
    id1 = await r.xadd("mystream", {"f": "v1"})
    id2 = await r.xadd("mystream", {"f": "v2"})
    await r.xgroup_create("mystream", "mygroup", id="0")

    # Read and get pending
    await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"})

    # ACK both messages
    await r.xack("mystream", "mygroup", id1.decode(), id2.decode())

    # Now "0" should return no pending entries
    result = await r.xreadgroup("mygroup", "consumer1", {"mystream": "0"})
    assert not result  # None or empty list


async def test_xreadgroup_count_limit(r):
    """STRM-07: count parameter limits returned entries."""
    for i in range(5):
        await r.xadd("mystream", {"f": f"v{i}"})
    await r.xgroup_create("mystream", "mygroup", id="0")

    result = await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"}, count=2)
    assert result is not None
    stream_name, entries = result[0]
    assert len(entries) == 2


async def test_xreadgroup_nogroup_error(r):
    """STRM-07: XREADGROUP on non-existent group raises NOGROUP error."""
    await r.xadd("mystream", {"f": "v1"})
    with pytest.raises(Exception, match="NOGROUP"):
        await r.xreadgroup("nogroup", "consumer1", {"mystream": ">"})


# --- STRM-08: XACK ---


async def test_xack_removes_from_pel(r):
    """STRM-08: After XREADGROUP and XACK, message no longer pending."""
    entry_id = await r.xadd("mystream", {"f": "v1"})
    await r.xgroup_create("mystream", "mygroup", id="0")

    # Read (adds to PEL)
    await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"})

    # Verify it's in PEL
    pending = await r.xreadgroup("mygroup", "consumer1", {"mystream": "0"})
    assert pending is not None
    assert len(pending[0][1]) == 1

    # ACK it
    await r.xack("mystream", "mygroup", entry_id.decode())

    # No longer in PEL
    pending = await r.xreadgroup("mygroup", "consumer1", {"mystream": "0"})
    assert not pending  # None or empty list


async def test_xack_returns_count(r):
    """STRM-08: XACK returns number of actually acknowledged messages."""
    id1 = await r.xadd("mystream", {"f": "v1"})
    id2 = await r.xadd("mystream", {"f": "v2"})
    await r.xgroup_create("mystream", "mygroup", id="0")

    await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"})

    count = await r.xack("mystream", "mygroup", id1.decode(), id2.decode())
    assert count == 2


async def test_xack_idempotent(r):
    """STRM-08: ACKing already-acked message returns 0."""
    entry_id = await r.xadd("mystream", {"f": "v1"})
    await r.xgroup_create("mystream", "mygroup", id="0")

    await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"})

    # First ACK
    count = await r.xack("mystream", "mygroup", entry_id.decode())
    assert count == 1

    # Second ACK (already acked)
    count = await r.xack("mystream", "mygroup", entry_id.decode())
    assert count == 0


async def test_xack_nonexistent_stream(r):
    """STRM-08: XACK on missing stream returns 0."""
    count = await r.xack("nonexistent", "mygroup", "1-1")
    assert count == 0


# --- STRM-09: XAUTOCLAIM ---


async def test_xautoclaim_claims_idle_messages(r):
    """STRM-09: XAUTOCLAIM reclaims idle pending messages from other consumers."""
    # Add entries and read with consumer1
    id1 = await r.xadd("mystream", {"f": "v1"})
    id2 = await r.xadd("mystream", {"f": "v2"})
    await r.xgroup_create("mystream", "mygroup", id="0")
    await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"})

    # Consumer2 claims all idle messages (min_idle_time=0 means claim immediately)
    result = await r.xautoclaim("mystream", "mygroup", "consumer2", 0, start_id="0-0")
    assert isinstance(result, tuple)
    assert len(result) == 3

    next_id, claimed, deleted = result
    assert len(claimed) == 2
    # Claimed entries should have field data
    assert claimed[0][1][b"f"] == b"v1"
    assert claimed[1][1][b"f"] == b"v2"
    # Deleted list should be empty (entries still exist)
    assert len(deleted) == 0


async def test_xautoclaim_increments_delivery_count(r):
    """STRM-09: After autoclaim, delivery count increases."""
    await r.xadd("mystream", {"f": "v1"})
    await r.xgroup_create("mystream", "mygroup", id="0")
    await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"})

    # First claim by consumer2 (delivery_count goes from 1 to 2)
    await r.xautoclaim("mystream", "mygroup", "consumer2", 0, start_id="0-0")

    # Second claim by consumer3 (delivery_count goes from 2 to 3)
    await r.xautoclaim("mystream", "mygroup", "consumer3", 0, start_id="0-0")

    # Verify consumer3 has the message in its PEL
    info = await r.xinfo_consumers("mystream", "mygroup")
    consumer3_info = [c for c in info if c["name"] == b"consumer3"]
    assert len(consumer3_info) == 1
    assert consumer3_info[0]["pending"] == 1


async def test_xautoclaim_returns_deleted_ids(r):
    """STRM-09: If a pending message was trimmed, it appears in deleted_ids."""
    # Add entries and read them
    await r.xadd("mystream", {"f": "v1"})
    await r.xadd("mystream", {"f": "v2"})
    await r.xadd("mystream", {"f": "v3"})
    await r.xgroup_create("mystream", "mygroup", id="0")
    await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"})

    # Trim the stream to only keep 1 entry (removes first 2)
    await r.xtrim("mystream", maxlen=1)

    # Autoclaim: the trimmed entries should appear in deleted_ids
    result = await r.xautoclaim("mystream", "mygroup", "consumer2", 0, start_id="0-0")
    next_id, claimed, deleted = result

    # One entry was kept (the last one), two were trimmed
    assert len(claimed) == 1
    assert len(deleted) == 2


async def test_xautoclaim_respects_min_idle_time(r):
    """STRM-09: Messages not idle long enough are NOT claimed."""
    await r.xadd("mystream", {"f": "v1"})
    await r.xgroup_create("mystream", "mygroup", id="0")
    await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"})

    # Use a very large min_idle_time so nothing qualifies
    result = await r.xautoclaim("mystream", "mygroup", "consumer2", 999999, start_id="0-0")
    next_id, claimed, deleted = result

    # Nothing should be claimed
    assert len(claimed) == 0
    assert len(deleted) == 0


async def test_xautoclaim_respects_count(r):
    """STRM-09: count parameter limits how many messages are claimed."""
    # Add 3 entries
    await r.xadd("mystream", {"f": "v1"})
    await r.xadd("mystream", {"f": "v2"})
    await r.xadd("mystream", {"f": "v3"})
    await r.xgroup_create("mystream", "mygroup", id="0")
    await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"})

    # Claim only 2 of 3
    result = await r.xautoclaim("mystream", "mygroup", "consumer2", 0, start_id="0-0", count=2)
    next_id, claimed, deleted = result

    assert len(claimed) == 2


async def test_xautoclaim_returns_next_start_id(r):
    """STRM-09: When not all idle messages claimed (count limit), next_start_id indicates continuation."""
    id1 = await r.xadd("mystream", {"f": "v1"})
    id2 = await r.xadd("mystream", {"f": "v2"})
    id3 = await r.xadd("mystream", {"f": "v3"})
    await r.xgroup_create("mystream", "mygroup", id="0")
    await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"})

    # Claim only 2 of 3
    result = await r.xautoclaim("mystream", "mygroup", "consumer2", 0, start_id="0-0", count=2)
    next_id, claimed, deleted = result

    # next_id should be the ID of the 3rd entry (unclaimed)
    assert next_id == id3
    # Signal that there's more to process (non-zero)
    assert next_id != b"0-0"

    # Claim again from next_id -- should get the remaining one
    result2 = await r.xautoclaim(
        "mystream", "mygroup", "consumer2", 0, start_id=next_id.decode(), count=10
    )
    next_id2, claimed2, deleted2 = result2
    assert len(claimed2) == 1
    assert next_id2 == b"0-0"  # All done


# --- STRM-10: XINFO GROUPS ---


async def test_xinfo_groups_returns_group_info(r):
    """STRM-10: XINFO GROUPS returns correct metadata for a group."""
    await r.xadd("mystream", {"f": "v1"})
    await r.xgroup_create("mystream", "mygroup", id="0")

    info = await r.xinfo_groups("mystream")
    assert len(info) == 1
    assert info[0]["name"] == b"mygroup"
    assert info[0]["consumers"] == 0
    assert info[0]["pending"] == 0


async def test_xinfo_groups_multiple_groups(r):
    """STRM-10: XINFO GROUPS returns all groups on a stream."""
    await r.xadd("mystream", {"f": "v1"})
    await r.xgroup_create("mystream", "group1", id="0")
    await r.xgroup_create("mystream", "group2", id="0")

    info = await r.xinfo_groups("mystream")
    assert len(info) == 2
    names = {entry["name"] for entry in info}
    assert b"group1" in names
    assert b"group2" in names


async def test_xinfo_groups_empty_stream(r):
    """STRM-10: XINFO GROUPS on stream with no groups returns empty list."""
    await r.xadd("mystream", {"f": "v1"})
    info = await r.xinfo_groups("mystream")
    assert info == []


async def test_xinfo_groups_pending_count(r):
    """STRM-10: After XREADGROUP without XACK, pending count is accurate."""
    await r.xadd("mystream", {"f": "v1"})
    await r.xadd("mystream", {"f": "v2"})
    await r.xgroup_create("mystream", "mygroup", id="0")
    await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"})

    info = await r.xinfo_groups("mystream")
    assert info[0]["pending"] == 2
    assert info[0]["consumers"] == 1


# --- STRM-11: XINFO CONSUMERS ---


async def test_xinfo_consumers_returns_consumer_info(r):
    """STRM-11: XINFO CONSUMERS shows consumer with pending count."""
    await r.xadd("mystream", {"f": "v1"})
    await r.xadd("mystream", {"f": "v2"})
    await r.xgroup_create("mystream", "mygroup", id="0")
    await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"})

    info = await r.xinfo_consumers("mystream", "mygroup")
    assert len(info) == 1
    assert info[0]["name"] == b"consumer1"
    assert info[0]["pending"] == 2
    assert "idle" in info[0]
    assert info[0]["idle"] >= 0


async def test_xinfo_consumers_multiple_consumers(r):
    """STRM-11: Two consumers read, both appear in XINFO CONSUMERS."""
    await r.xadd("mystream", {"f": "v1"})
    await r.xadd("mystream", {"f": "v2"})
    await r.xgroup_create("mystream", "mygroup", id="0")

    # consumer1 reads first message
    await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"}, count=1)
    # consumer2 reads second message
    await r.xreadgroup("mygroup", "consumer2", {"mystream": ">"}, count=1)

    info = await r.xinfo_consumers("mystream", "mygroup")
    assert len(info) == 2
    names = {entry["name"] for entry in info}
    assert b"consumer1" in names
    assert b"consumer2" in names


async def test_xinfo_consumers_after_ack(r):
    """STRM-11: After XACK, consumer's pending count decreases."""
    id1 = await r.xadd("mystream", {"f": "v1"})
    await r.xadd("mystream", {"f": "v2"})
    await r.xgroup_create("mystream", "mygroup", id="0")
    await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"})

    # ACK one message
    await r.xack("mystream", "mygroup", id1.decode())

    info = await r.xinfo_consumers("mystream", "mygroup")
    assert info[0]["pending"] == 1


async def test_xinfo_consumers_nogroup_error(r):
    """STRM-11: XINFO CONSUMERS on non-existent group raises error."""
    await r.xadd("mystream", {"f": "v1"})

    with pytest.raises(Exception):
        await r.xinfo_consumers("mystream", "nogroup")


# --- XPENDING_RANGE ---


async def test_xpending_range_returns_all_pending(r):
    """xpending_range with '-' and '+' returns all pending entries with correct dict keys."""
    await r.xadd("mystream", {"f": "v1"})
    await r.xadd("mystream", {"f": "v2"})
    await r.xgroup_create("mystream", "mygroup", id="0")
    await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"})

    result = await r.xpending_range("mystream", "mygroup", "-", "+", 10)
    assert isinstance(result, list)
    assert len(result) == 2

    # Each entry should be a dict with the correct keys
    for entry in result:
        assert isinstance(entry, dict)
        assert b"message_id" in entry
        assert b"consumer" in entry
        assert b"time_since_delivered" in entry
        assert b"times_delivered" in entry
        assert entry[b"consumer"] == b"consumer1"
        assert isinstance(entry[b"time_since_delivered"], int)
        assert entry[b"time_since_delivered"] >= 0
        assert entry[b"times_delivered"] >= 1


async def test_xpending_range_consumer_filter(r):
    """xpending_range with consumername filter returns only that consumer's entries."""
    await r.xadd("mystream", {"f": "v1"})
    await r.xadd("mystream", {"f": "v2"})
    await r.xadd("mystream", {"f": "v3"})
    await r.xgroup_create("mystream", "mygroup", id="0")

    # consumer1 reads first 2
    await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"}, count=2)
    # consumer2 reads the 3rd
    await r.xreadgroup("mygroup", "consumer2", {"mystream": ">"})

    # Filter by consumer1
    result = await r.xpending_range("mystream", "mygroup", "-", "+", 10, consumername="consumer1")
    assert len(result) == 2
    for entry in result:
        assert entry[b"consumer"] == b"consumer1"

    # Filter by consumer2
    result2 = await r.xpending_range("mystream", "mygroup", "-", "+", 10, consumername="consumer2")
    assert len(result2) == 1
    assert result2[0][b"consumer"] == b"consumer2"


async def test_xpending_range_count_limits_results(r):
    """xpending_range count parameter limits number of results."""
    for i in range(5):
        await r.xadd("mystream", {"f": f"v{i}"})
    await r.xgroup_create("mystream", "mygroup", id="0")
    await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"})

    result = await r.xpending_range("mystream", "mygroup", "-", "+", 2)
    assert len(result) == 2


async def test_xpending_range_nogroup_error(r):
    """xpending_range on non-existent group raises error."""
    await r.xadd("mystream", {"f": "v1"})
    with pytest.raises(Exception, match="NOGROUP"):
        await r.xpending_range("mystream", "nogroup", "-", "+", 10)


async def test_xpending_range_empty(r):
    """xpending_range with no pending entries returns empty list."""
    await r.xadd("mystream", {"f": "v1"})
    await r.xgroup_create("mystream", "mygroup", id="0")

    result = await r.xpending_range("mystream", "mygroup", "-", "+", 10)
    assert result == []


async def test_xpending_range_idle_filter(r):
    """xpending_range idle filter excludes recently-delivered entries."""
    await r.xadd("mystream", {"f": "v1"})
    await r.xgroup_create("mystream", "mygroup", id="0")
    await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"})

    # With a very high idle threshold, nothing should qualify
    result = await r.xpending_range("mystream", "mygroup", "-", "+", 10, idle=999999)
    assert result == []

    # With idle=0, everything qualifies
    result2 = await r.xpending_range("mystream", "mygroup", "-", "+", 10, idle=0)
    assert len(result2) == 1


# --- XREADGROUP Blocking ---


async def test_xreadgroup_block_returns_new_entries(r):
    """XREADGROUP with block waits for new entries added after the call."""
    await r.xadd("mystream", {"f": "v1"})
    await r.xgroup_create("mystream", "mygroup", id="0")
    # Read existing entry
    await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"})

    # Schedule an XADD after a short delay
    async def add_later():
        await asyncio.sleep(0.05)
        await r.xadd("mystream", {"f": "v2"})

    task = asyncio.create_task(add_later())
    # Block for up to 2000ms -- should return quickly after add_later fires
    result = await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"}, block=2000)
    await task
    assert len(result) > 0
    # Verify we got the new entry
    stream_name, entries = result[0]
    assert entries[0][1][b"f"] == b"v2"


async def test_xreadgroup_block_timeout_returns_empty(r):
    """XREADGROUP with block returns empty after timeout if no new data."""
    await r.xadd("mystream", {"f": "v1"})
    await r.xgroup_create("mystream", "mygroup", id="0")
    await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"})

    # Block for 50ms with no new data
    start = time.monotonic()
    result = await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"}, block=50)
    elapsed = time.monotonic() - start
    # Should return empty (either [] or None-ish)
    assert len(result) == 0
    # Should have waited approximately 50ms (at least 30ms to allow for timing variance)
    assert elapsed >= 0.03


async def test_xreadgroup_block_lua_xadd_wakes_reader(r):
    """XREADGROUP with block wakes up when XADD is done from a Lua script."""
    await r.xadd("mystream", {"f": "v1"})
    await r.xgroup_create("mystream", "mygroup", id="0")
    await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"})

    lua_script = r.register_script("""
    redis.call('XADD', KEYS[1], '*', 'f', ARGV[1])
    return 1
    """)

    async def lua_add_later():
        await asyncio.sleep(0.05)
        await lua_script(keys=["mystream"], args=["from_lua"])

    task = asyncio.create_task(lua_add_later())
    result = await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"}, block=2000)
    await task
    assert len(result) > 0
    stream_name, entries = result[0]
    assert entries[0][1][b"f"] == b"from_lua"


# --- XCLAIM ---


async def test_xclaim_transfers_ownership(r):
    """XCLAIM transfers pending entries from one consumer to another."""
    id1 = await r.xadd("mystream", {"f": "v1"})
    id2 = await r.xadd("mystream", {"f": "v2"})
    await r.xgroup_create("mystream", "mygroup", id="0")
    await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"})

    # consumer2 claims both entries
    result = await r.xclaim("mystream", "mygroup", "consumer2", 0, [id1, id2])
    assert len(result) == 2
    assert result[0][1][b"f"] == b"v1"
    assert result[1][1][b"f"] == b"v2"


async def test_xclaim_resets_idle_time(r):
    """XCLAIM with idle=0 resets the entry's idle time."""
    id1 = await r.xadd("mystream", {"f": "v1"})
    await r.xgroup_create("mystream", "mygroup", id="0")
    await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"})

    # Wait a tiny bit so entry has some idle time
    await asyncio.sleep(0.01)

    # Claim with idle=0 (reset idle time) -- same consumer (lease renewal pattern)
    result = await r.xclaim("mystream", "mygroup", "consumer1", 0, [id1], idle=0)
    assert len(result) == 1


async def test_xclaim_respects_min_idle_time(r):
    """XCLAIM skips entries not idle long enough."""
    id1 = await r.xadd("mystream", {"f": "v1"})
    await r.xgroup_create("mystream", "mygroup", id="0")
    await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"})

    # Claim with huge min_idle_time -- nothing qualifies
    result = await r.xclaim("mystream", "mygroup", "consumer2", 999999, [id1])
    assert len(result) == 0


async def test_xclaim_justid_returns_ids_only(r):
    """XCLAIM with justid=True returns only IDs, not field data."""
    id1 = await r.xadd("mystream", {"f": "v1"})
    await r.xgroup_create("mystream", "mygroup", id="0")
    await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"})

    result = await r.xclaim("mystream", "mygroup", "consumer2", 0, [id1], justid=True)
    assert len(result) == 1
    # Should be just the ID bytes, not a tuple
    assert isinstance(result[0], bytes)


async def test_xclaim_nonexistent_id_is_skipped(r):
    """XCLAIM silently skips IDs not in any consumer's PEL."""
    id1 = await r.xadd("mystream", {"f": "v1"})
    await r.xgroup_create("mystream", "mygroup", id="0")
    await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"})

    result = await r.xclaim("mystream", "mygroup", "consumer2", 0, ["99999-0"])
    assert len(result) == 0


# --- XTRIM approximate ---


async def test_xtrim_accepts_approximate_parameter(r):
    """XTRIM accepts approximate parameter without error."""
    await r.xadd("mystream", {"f": "v1"})
    await r.xadd("mystream", {"f": "v2"})
    await r.xadd("mystream", {"f": "v3"})

    # Should work with approximate=False (pydocket's docket.clear() pattern)
    trimmed = await r.xtrim("mystream", maxlen=0, approximate=False)
    assert trimmed == 3


# ---- XPENDING Summary Tests (D-11) ----


async def test_xpending_summary_with_pending(r):
    """xpending() summary returns dict with pending messages."""
    await r.xadd("mystream", {"data": "value1"})
    await r.xadd("mystream", {"data": "value2"})
    await r.xgroup_create("mystream", "mygroup", id="0")
    await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"}, count=2)

    result = await r.xpending("mystream", "mygroup")
    assert result["pending"] == 2
    assert result["min"] is not None
    assert result["max"] is not None
    assert len(result["consumers"]) == 1
    assert result["consumers"][0]["name"] == b"consumer1"
    assert result["consumers"][0]["pending"] == 2


async def test_xpending_summary_empty(r):
    """xpending() summary returns zeros when no messages are pending."""
    await r.xadd("mystream", {"data": "value"})
    await r.xgroup_create("mystream", "mygroup", id="0")
    # Read and ACK
    msgs = await r.xreadgroup("mygroup", "consumer1", {"mystream": ">"}, count=1)
    msg_id = msgs[0][1][0][0]
    await r.xack("mystream", "mygroup", msg_id)

    result = await r.xpending("mystream", "mygroup")
    assert result["pending"] == 0
    assert result["min"] is None
    assert result["max"] is None
    assert result["consumers"] == []
