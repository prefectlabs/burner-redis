use bytes::Bytes;
use pyo3::prelude::*;
use pyo3::types::PyAny;
use std::time::Duration;

/// Extract a key or value from a Python object (str or bytes).
/// Matches redis-py behavior: str auto-encoded to UTF-8 bytes.
pub fn extract_bytes(obj: &Bound<'_, PyAny>) -> PyResult<Bytes> {
    // Try str first (extract as owned String for PyO3 0.28+ compatibility)
    if let Ok(s) = obj.extract::<String>() {
        Ok(Bytes::from(s.into_bytes()))
    } else if let Ok(b) = obj.extract::<Vec<u8>>() {
        Ok(Bytes::from(b))
    } else {
        Err(pyo3::exceptions::PyTypeError::new_err(
            "expected str or bytes",
        ))
    }
}

/// Extract an expiration value from a Python object.
/// Accepts int (seconds or milliseconds) or datetime.timedelta.
/// Returns Duration.
pub fn extract_expiry(obj: &Bound<'_, PyAny>, unit_millis: bool) -> PyResult<Duration> {
    // Try extracting as integer first
    if let Ok(val) = obj.extract::<u64>() {
        return Ok(if unit_millis {
            Duration::from_millis(val)
        } else {
            Duration::from_secs(val)
        });
    }
    // Try extracting as timedelta via total_seconds()
    if let Ok(total_secs) = obj
        .call_method0("total_seconds")
        .and_then(|v| v.extract::<f64>())
    {
        if total_secs < 0.0 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "expiration must be non-negative",
            ));
        }
        return Ok(Duration::from_secs_f64(total_secs));
    }
    Err(pyo3::exceptions::PyTypeError::new_err(
        "expected int or timedelta for expiration",
    ))
}
