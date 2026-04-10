//! Hash command helpers for the Python binding layer.
//!
//! This module provides helper functions for Redis hash commands.
//! The actual Python method implementations live in lib.rs following
//! the established pattern (via #[pymethods] on BurnerRedis).
//!
//! Hash commands implemented:
//! - HSET: Set field-value pairs in a hash
//! - HGET: Get the value of a hash field
//! - HDEL: Delete one or more hash fields
//! - HVALS: Get all values in a hash
//!
//! The core hash logic lives in Store (src/store.rs). This module
//! will house any future helper functions specific to hash argument
//! extraction from Python objects.
