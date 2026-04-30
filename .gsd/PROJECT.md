# Burner Redis

## What This Is

An embedded, in-process Redis-compatible database written in Rust with Python bindings. It provides a drop-in replacement for `redis.asyncio.Redis` that runs inside the host process with no external server needed. The primary use case is backing a self-hosted Prefect server without requiring a separate Redis deployment.

## Core Value

A self-hosted Prefect server can start, run flows, and manage state using this library instead of an external Redis server — zero infrastructure, zero configuration.

## Current State

**Shipped:** v0.1.6 — Wiring and Coverage Gaps (2026-04-27)
**Tag:** v0.1.6
**PyPI:** v0.1.5 (latest published; see archived audit for version-vs-tag context)
**Phases:** 15 (1–15)
**Plans:** 31 of 32 (Plan 13-03 staged-recipes PR submission pending external action)
**Tests:** Rust 151 pass, Python 540 pass

See [MILESTONES.md](MILESTONES.md) for the full v0.1.6 record.

## Requirements

### Validated (v0.1.6)

- ✓ Embedded in-process Redis engine in Rust with Python bindings (PyO3/maturin) — v0.1.6 Phase 1
- ✓ String commands: SET (with EX, PX, NX, XX flags), GET, DELETE, EXISTS — v0.1.6 Phase 1
- ✓ Hash commands: HSET, HGET, HDEL, HVALS — v0.1.6 Phase 2
- ✓ Set commands: SADD, SMEMBERS, SISMEMBER, SREM — v0.1.6 Phase 2
- ✓ Sorted set commands: ZADD, ZREM, ZRANGE, ZRANGEBYSCORE, ZRANGESTORE, ZREMRANGEBYSCORE — v0.1.6 Phase 3
- ✓ Key expiration: TTL-based expiry (seconds and milliseconds), passive + active sweep — v0.1.6 Phase 4
- ✓ Stream commands: XADD, XREAD, XREADGROUP, XLEN, XACK, XAUTOCLAIM, XTRIM, XINFO GROUPS, XINFO CONSUMERS, XGROUP CREATE, XGROUP DESTROY — v0.1.6 Phase 5
- ✓ Lua scripting: EVAL/EVALSHA with `redis.call()` dispatch covering all command surfaces — v0.1.6 Phase 6
- ✓ Pipeline support: batch multiple commands with atomic execution + sync fast-path — v0.1.6 Phase 7
- ✓ Distributed locking: Lock/AsyncLock with token-based ownership — v0.1.6 Phase 7
- ✓ Flush to disk + reload from disk: MessagePack snapshots with atexit + crash-safe writes — v0.1.6 Phase 8
- ✓ Published as a PyPI package with pre-built wheels (manylinux + macOS x86_64/arm64) — v0.1.6 Phase 9
- ✓ Pub/Sub: SUBSCRIBE, UNSUBSCRIBE, PUBLISH, PSUBSCRIBE, PUNSUBSCRIBE, PUBSUB introspection — v0.1.6 Phase 10
- ✓ Drop-in compatible with `redis.asyncio.Redis` API surface used by Prefect (verified via pydocket integration suite) — v0.1.6 Phases 11+12
- ✓ List commands: LPUSH, RPUSH, LPOP, RPOP, LRANGE, LLEN, LINDEX, LSET, LREM, LTRIM, LINSERT, LMOVE, RPOPLPUSH, BLPOP, BRPOP, BLMOVE — v0.1.6 Phase 14
- ✓ NoScriptError wiring + pipeline regression coverage + list persistence coverage — v0.1.6 Phase 15

### Active

(None — define next milestone with `/gsd-new-milestone`.)

Candidates for next milestone (carried from accepted v0.1.6 tech debt):
- VERIFICATION.md backfill for Phases 1–9 + 13 (procedural)
- Complete Nyquist VALIDATION.md drafts (Phases 1, 10, 11, 12, 14)
- conda-forge feedstock PR (Plan 13-03 — pending external action on staged-recipes fork)
- Add ZRANGESTORE/ZCOUNT/stream commands to Lua dispatch (currently consistent with real Redis but undocumented)
- Resolve PyPI version-vs-tag drift (PyPI v0.1.5 vs tag v0.1.6)

### Out of Scope

- Network server / Redis wire protocol — this is embedded, not a server
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
| Embedded in-process, not standalone server | Eliminates infrastructure complexity for self-hosted Prefect | ✓ Good — pydocket integration confirms drop-in works in real Prefect server flows (v0.1.6 Phase 11) |
| Drop-in redis-py API compatibility | Prefect code doesn't need to change | ✓ Good — pydocket suite passes against burner_redis without modification (v0.1.6 Phases 11+12) |
| Rust + PyO3 for core engine | Performance + safety for data structures, Python bindings for usability | ✓ Good — sync fast-path eliminated async overhead (v0.1.6 Phase 7) |
| Full Lua EVAL support over built-in atomics | Prefect's Lua scripts are complex; a real Lua engine is more maintainable than reimplementing each script | ✓ Good — mlua 0.10 + Lua 5.4 vendored handles all Prefect scripts with lock-ordering enforced (v0.1.6 Phase 6) |
| Start narrow, design for growth | Only implement Prefect's commands now, but architecture should allow adding more later | ✓ Good — added Pub/Sub (Phase 10) and List type (Phase 14) without core refactors |
| Custom persistence format | No need for Redis RDB/AOF compatibility; simpler and more efficient | ✓ Good — MessagePack chosen over bincode (RUSTSEC-2025-0141); all 6 ValueData variants round-trip (v0.1.6 Phase 8 + Phase 15 list coverage) |
| ValueData enum (single keyspace) over per-type maps | Matches Redis single-key model; one RwLock for atomic multi-key operations | ✓ Good — pipelines and Lua scripts share consistent locking (v0.1.6 Phase 2+) |
| Dual-index SortedSet (BTreeMap + HashMap) | O(1) member lookup + O(log n) range queries; better cache locality than skiplist for single-writer scenarios | ✓ Good (v0.1.6 Phase 3) |
| Python-side monkey-patching for Pipeline/Lock factories | Pure Python wrappers around Rust dispatch; matches redis-py shape without expanding Rust API | ✓ Good (v0.1.6 Phase 7) |
| 4-target build matrix: linux x86_64/aarch64 + macOS x86_64/arm64 | Covers Prefect's deployment surface; Windows deferred | ✓ Good — published to PyPI with trusted-publisher OIDC (v0.1.6 Phase 9) |
| `burner_redis.NoScriptError` subclasses `redis.exceptions.NoScriptError` (when redis is installed) | Caller `except` from either form catches; rust resolves `burner_redis` first to satisfy `pytest.raises(burner_redis.NoScriptError)` | ✓ Good (v0.1.6 Phase 15) |

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
*Last updated: 2026-04-27 after v0.1.6 milestone completion*
