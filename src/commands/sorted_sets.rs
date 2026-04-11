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
