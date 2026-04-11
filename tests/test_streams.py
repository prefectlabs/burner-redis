"""Tests for stream commands: XADD, XREAD, XLEN, XTRIM.

Covers requirements: STRM-01, STRM-02, STRM-03, STRM-04.
"""
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
    assert result is None

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
    assert result is None

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
    assert result is None


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
    assert pending is None


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
