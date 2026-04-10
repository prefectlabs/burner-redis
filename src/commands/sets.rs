//! Set command helpers for the Python binding layer.
//!
//! This module provides helper functions for Redis set commands.
//! The actual Python method implementations live in lib.rs following
//! the established pattern (via #[pymethods] on BurnerRedis).
//!
//! Set commands implemented:
//! - SADD: Add one or more members to a set
//! - SMEMBERS: Get all members in a set
//! - SISMEMBER: Determine if a given value is a member of a set
//! - SREM: Remove one or more members from a set
//!
//! The core set logic lives in Store (src/store.rs). This module
//! will house any future helper functions specific to set argument
//! extraction from Python objects.
