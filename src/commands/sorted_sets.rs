//! Sorted set command helpers for the Python binding layer.
//!
//! This module provides helper functions for Redis sorted set commands.
//! The actual Python method implementations live in lib.rs following
//! the established pattern (via #[pymethods] on BurnerRedis).
//!
//! Sorted set commands implemented:
//! - ZADD: Add members with scores to a sorted set (with NX/XX/GT/LT/CH flags)
//! - ZREM: Remove one or more members from a sorted set
//! - ZRANGE: Get members by index range
//! - ZRANGEBYSCORE: Get members by score range
//! - ZRANGESTORE: Store a score-range result into a destination key
//! - ZREMRANGEBYSCORE: Remove members by score range
//!
//! The core sorted set logic lives in Store (src/store.rs) using the dual-index
//! pattern: BTreeMap<(OrderedFloat<f64>, Bytes), ()> for score-ordered range queries
//! plus HashMap<Bytes, f64> for O(1) member-to-score lookup.

use pyo3::prelude::*;
use pyo3::types::PyAny;

/// Parse a score bound from a Python object.
/// Accepts float, int, or string ("-inf", "+inf", "inf").
/// Returns Err(PyValueError) for invalid input.
pub fn parse_score_bound(obj: &Bound<'_, PyAny>) -> PyResult<f64> {
    // Try float first
    if let Ok(f) = obj.extract::<f64>() {
        return Ok(f);
    }
    // Try string for -inf/+inf or numeric strings
    if let Ok(s) = obj.extract::<String>() {
        match s.as_str() {
            "-inf" => return Ok(f64::NEG_INFINITY),
            "+inf" | "inf" => return Ok(f64::INFINITY),
            _ => {}
        }
        if let Ok(f) = s.parse::<f64>() {
            return Ok(f);
        }
    }
    Err(pyo3::exceptions::PyValueError::new_err(
        "min/max must be a float or '-inf'/'+inf'",
    ))
}
