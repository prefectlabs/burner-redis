use bytes::Bytes;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::collections::HashMap;

use super::strings::extract_bytes;

/// A stream entry ID: (milliseconds_since_epoch, sequence_number).
pub type StreamId = (u64, u64);

/// Format a StreamId as the Redis canonical string: "ms-seq".
pub fn format_stream_id(id: StreamId) -> String {
    format!("{}-{}", id.0, id.1)
}

/// Parse a "ms-seq" string into a StreamId.
/// Returns None if the format is invalid.
pub fn parse_stream_id(s: &str) -> Option<StreamId> {
    let parts: Vec<&str> = s.splitn(2, '-').collect();
    if parts.len() != 2 {
        return None;
    }
    let ms = parts[0].parse::<u64>().ok()?;
    let seq = parts[1].parse::<u64>().ok()?;
    Some((ms, seq))
}

/// Extract a Python dict of field -> value pairs into a HashMap<Bytes, Bytes>.
pub fn extract_stream_fields(dict: &Bound<'_, PyDict>) -> PyResult<HashMap<Bytes, Bytes>> {
    let mut fields = HashMap::new();
    for (k, v) in dict.iter() {
        let key = extract_bytes(&k)?;
        let val = extract_bytes(&v)?;
        fields.insert(key, val);
    }
    Ok(fields)
}
