# Burner Redis

## What This Is

An embedded, in-process Redis-compatible database written in Rust with Python bindings. It provides a drop-in replacement for `redis.asyncio.Redis` that runs inside the host process with no external server needed. The primary use case is backing a self-hosted Prefect server without requiring a separate Redis deployment.

## Core Value

A self-hosted Prefect server can start, run flows, and manage state using this library instead of an external Redis server — zero infrastructure, zero configuration.

## Requirements

### Validated

- [x] Embedded in-process Redis engine in Rust with Python bindings (PyO3/maturin) — Validated in Phase 1
- [x] String commands: SET (with EX, PX, NX, XX flags), GET, DELETE, EXISTS — Validated in Phase 1

### Active

- [ ] Drop-in compatible with `redis.asyncio.Redis` API surface used by Prefect
- [ ] Hash commands: HSET, HGET, HDEL, HVALS
- [ ] Set commands: SADD, SMEMBERS, SISMEMBER, SREM
- [ ] Sorted set commands: ZADD, ZREM, ZRANGE, ZRANGEBYSCORE, ZRANGESTORE, ZREMRANGEBYSCORE
- [ ] Stream commands: XADD, XREAD, XREADGROUP, XLEN, XACK, XAUTOCLAIM, XTRIM, XINFO GROUPS, XINFO CONSUMERS, XGROUP CREATE, XGROUP DESTROY
- [ ] Lua scripting: EVAL/EVALSHA with full script support for Prefect's atomic operations
- [ ] Pipeline support: batch multiple commands with atomic execution
- [ ] Key expiration: TTL-based expiry (seconds and milliseconds)
- [ ] Distributed locking: Lock/AsyncLock compatible semantics
- [ ] Flush to disk: manual save/flush API + automatic persist on shutdown
- [ ] Reload from disk: restore state from a previous flush on startup
- [ ] Published as a PyPI package with pre-built wheels

### Out of Scope

- Network server / Redis wire protocol — this is embedded, not a server
- Pub/Sub (SUBSCRIBE/PUBLISH) — Prefect uses Streams, not pub/sub
- Cluster/sentinel support — single-process embedded use only
- Replication — no multi-node support needed
- ACL / authentication — runs in-process, no auth boundary
- Full Redis command coverage — only commands Prefect uses
- Mobile or browser targets — server-side Python only

## Context

- Prefect's Redis usage lives in `prefect-redis` integration (`src/integrations/prefect-redis/`)
- Core subsystems using Redis: messaging (streams + consumer groups), event ordering (Lua scripts + sorted sets), distributed locking, concurrency lease storage (Lua scripts + sorted sets + hashes)
- Prefect uses `redis.asyncio.Redis` exclusively — async-first
- Lua scripts are used for atomic multi-key operations: lease create/revoke/renew, event completion with follower tracking
- Consumer groups with XREADGROUP are central to the messaging system
- Dead letter queue pattern uses hashes + sets together
- Pipeline usage for batching related operations atomically
- The library name is "burner-redis" — ephemeral by design but with optional persistence

## Constraints

- **Language**: Rust core with Python bindings via PyO3/maturin
- **Python API**: Must be compatible with `redis.asyncio.Redis` interface (drop-in replacement)
- **Async**: Must support Python async/await — Prefect is async-first
- **Distribution**: PyPI package with pre-built wheels (manylinux, macOS, Windows)
- **Persistence format**: Custom (no need for RDB/AOF compatibility)
- **Lua engine**: Embedded Lua interpreter in Rust for EVAL/EVALSHA support

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Embedded in-process, not standalone server | Eliminates infrastructure complexity for self-hosted Prefect | — Pending |
| Drop-in redis-py API compatibility | Prefect code doesn't need to change | — Pending |
| Rust + PyO3 for core engine | Performance + safety for data structures, Python bindings for usability | — Pending |
| Full Lua EVAL support over built-in atomics | Prefect's Lua scripts are complex; a real Lua engine is more maintainable than reimplementing each script | — Pending |
| Start narrow, design for growth | Only implement Prefect's commands now, but architecture should allow adding more later | — Pending |
| Custom persistence format | No need for Redis RDB/AOF compatibility; simpler and more efficient | — Pending |

## Evolution

This document evolves at phase transitions and milestone boundaries.

**After each phase transition** (via `/gsd-transition`):
1. Requirements invalidated? → Move to Out of Scope with reason
2. Requirements validated? → Move to Validated with phase reference
3. New requirements emerged? → Add to Active
4. Decisions to log? → Add to Key Decisions
5. "What This Is" still accurate? → Update if drifted

**After each milestone** (via `/gsd-complete-milestone`):
1. Full review of all sections
2. Core Value check — still the right priority?
3. Audit Out of Scope — reasons still valid?
4. Update Context with current state

---
*Last updated: 2026-04-10 after initialization*
