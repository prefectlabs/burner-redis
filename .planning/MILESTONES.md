# Milestones — Burner Redis

Historical record of shipped versions.

---

## v0.1.6 — Wiring and Coverage Gaps

**Status:** ✅ SHIPPED 2026-04-27
**Phases:** 15 phases (1–15)
**Plans:** 32 plans (31 executed, 1 deferred — 13-03 staged-recipes PR submission)
**Quick tasks:** 25 ad-hoc fixes
**Timeline:** 2026-04-10 → 2026-04-27 (17 days)
**Git stats:** 249 commits, 246 files changed, +72,844 LOC
**Tag:** v0.1.6

### Delivered

An embedded, in-process Redis-compatible database in Rust + PyO3 that drops into
`redis.asyncio.Redis` use sites. Self-hosted Prefect server can run flows without an
external Redis deployment.

### Key Accomplishments

1. **Foundation + 6 data types** — String, Hash, Set, Sorted Set, Stream (with consumer
   groups + PEL + autoclaim), and List (LPUSH/RPUSH/LPOP/RPOP/LRANGE/LLEN/LINDEX/LSET/
   LREM/LTRIM/LINSERT/LMOVE/RPOPLPUSH/BLPOP/BRPOP/BLMOVE), all `redis.asyncio.Redis`-shape.
2. **Lua scripting** — Embedded mlua Lua 5.4 with EVAL/EVALSHA and `redis.call()` dispatch
   covering all command surfaces, lock-ordering enforced, atomic multi-key operations.
3. **Pipelines + distributed locks** — Native Pipeline.execute() (sync fast-path) and
   redis-py-compatible Lock/AsyncLock with token-based ownership.
4. **Persistence** — Crash-safe MessagePack snapshots with atexit registration, TTL
   preserved as relative duration, all 6 ValueData variants round-trip.
5. **Pub/Sub + redis-py compatibility** — SUBSCRIBE/PUBLISH/PSUBSCRIBE/PUBSUB introspection
   wired through Tokio broadcast → asyncio.Queue, with full pydocket integration test pass.
6. **Distribution** — PyPI v0.1.5 published with manylinux + macOS (x86_64/arm64) wheels;
   conda-forge feedstock submitted; trusted-publisher OIDC; release version guards.

### Phase Highlights

- Phase 1: Foundation + String commands (PyO3 bridge proven)
- Phase 2: Hash + Set commands
- Phase 3: Sorted Set (dual-index BTreeMap+HashMap)
- Phase 4: Key expiration (passive + active sweep)
- Phase 5: Streams + Consumer Groups (XREADGROUP/XACK/XAUTOCLAIM)
- Phase 6: Lua scripting (mlua 0.10 + Lua 5.4 vendored)
- Phase 7: Pipeline + Locking
- Phase 8: Persistence (MessagePack + atexit)
- Phase 9: Distribution (PyPI 4-target wheels)
- Phase 10: Pub/Sub
- Phase 11: redis-py compatibility gaps for pydocket
- Phase 12: Drop-in replacement compat closure
- Phase 13: conda-forge feedstock (Plan 03 PR submission deferred)
- Phase 14: List data type (16 list commands incl. blocking)
- Phase 15: Close v0.1.6 audit ISSUE-1/2/3 (NoScriptError wiring, Pipeline regression
  tests, list persistence coverage)

### Audit Outcome

Milestone audit (`milestones/v0.1.6-MILESTONE-AUDIT.md`) status: `tech_debt`. All 69 v1
REQ-IDs satisfied. Phase 15 closed 3 minor wiring/coverage gaps (ISSUE-1, ISSUE-2,
ISSUE-3). Remaining tech debt accepted as deferred:

- 9 phases lack VERIFICATION.md (procedural — predate verification workflow rollout)
- 5 VALIDATION.md drafts have `nyquist_compliant: false` (formal Nyquist work scaffolded but not completed)
- ZRANGESTORE/ZCOUNT and 5 stream commands not in Lua dispatch (consistent with real Redis Lua scope; documented)
- PUBLISH from Lua returns 0 subscribers (documented design tradeoff)
- 13-03-SUMMARY.md missing (staged-recipes PR submission still pending developer action)

### Known Deferred Items at Close

26 items acknowledged via `audit-open` and recorded in STATE.md:
- 1 verification gap (Phase 10 VERIFICATION.md status `human_needed` — already addressed
  per the audit; PUBSUB-01..12 entries verified by current REQUIREMENTS.md state)
- 25 quick-task slugs flagged `missing` by the scanner — all completed and committed
  (commit hashes in STATE.md `## Quick Tasks Completed` table). Scanner false positive due
  to absent frontmatter; tasks themselves are shipped.

### Archives

- `milestones/v0.1.6-ROADMAP.md` — full phase details
- `milestones/v0.1.6-REQUIREMENTS.md` — frozen requirements with checkbox state
- `milestones/v0.1.6-MILESTONE-AUDIT.md` — pre-close audit report
- `milestones/v0.1.6-phases/` — all 15 phase directories with plans, summaries, UAT, security

---

_For current project status, see `.planning/ROADMAP.md`. Next milestone planning: `/gsd-new-milestone`._
