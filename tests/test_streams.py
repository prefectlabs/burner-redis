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
