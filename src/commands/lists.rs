//! List command helpers for the Python binding layer.
//!
//! This module provides helper functions for Redis list commands.
//! The actual Python method implementations live in lib.rs following
//! the established pattern (via #[pymethods] on BurnerRedis).
//!
//! List commands implemented:
//! - LPUSH, RPUSH, LPOP, RPOP, LRANGE, LLEN, LINDEX, LINSERT,
//! - LREM, LSET, LTRIM, LMOVE, RPOPLPUSH, BRPOP, BLPOP, BLMOVE
//!
//! The core list logic lives in Store (src/store.rs).

// NOTE: Helpers and tests are added in Task 2 of Plan 14-01.
