# Burner Redis — Retrospective

Living retrospective. New milestone sections appended above the "Cross-Milestone Trends" section.

---

## Milestone: v0.1.6 — Wiring and Coverage Gaps

**Shipped:** 2026-04-27
**Phases:** 15 | **Plans:** 31 of 32 (Plan 13-03 deferred — staged-recipes PR pending external action)
**Quick tasks:** 25 ad-hoc fixes
**Timeline:** 2026-04-10 → 2026-04-27 (17 days)
**Git stats:** 249 commits, 246 files changed, +72,844 LOC

### What Was Built

An embedded, in-process Redis-compatible database in Rust + PyO3 that drops into
`redis.asyncio.Redis` use sites without code changes:

- 6 data types (String, Hash, Set, Sorted Set, Stream w/ consumer groups + PEL + autoclaim, List w/ blocking)
- Lua scripting via mlua (Lua 5.4 vendored) with `redis.call()` covering all command surfaces and lock-ordering enforcement
- Native Pipeline (sync fast-path) + redis-py-compatible distributed Lock/AsyncLock with token-based ownership
- Crash-safe MessagePack persistence with atexit and TTL-as-relative-duration
- Pub/Sub via Tokio broadcast → asyncio.Queue
- 4-target wheel matrix (manylinux x86_64/aarch64 + macOS x86_64/arm64) published via PyPI trusted-publisher OIDC
- conda-forge feedstock submitted (final PR landing pending external action)

### What Worked

- **Single-keyspace ValueData enum.** One RwLock + one HashMap covers six data types, which made
  Lua atomicity, pipelines, and persistence consistent without per-type lock orchestration. This
  paid off repeatedly — every new type (Stream, List) slotted in without touching shared infra.
- **Phase 13's "spec-already-satisfied" recognition.** The conda-forge audit revealed sdist v0.1.2
  was already feedstock-ready — no pyproject.toml fix or 0.1.3 cut needed. Saved a packaging churn
  cycle by reading the existing artifacts before assuming work.
- **Python-side monkey-patching for Pipeline/Lock factories.** Pure-Python wrappers around Rust
  dispatch matched redis-py shape without expanding the Rust API surface. Same pattern used for
  Phase 14's blocking-list async wrappers — replicated cleanly.
- **Phase 15's audit-driven scope discipline.** ISSUE-2 was already wired in commit `de9d259`
  (the audit had stale line numbers); discovered before adding redundant code. The phase added
  regression tests instead of unnecessary source changes — exactly the right move.
- **Sync fast-path (quick task 260415-an2).** Eliminating async overhead in pipeline execute
  preserved redis-py compat without sacrificing throughput.
- **PyO3 0.28 idioms locked in early.** `Python::try_attach` for GIL re-attach in async blocks
  became the convention by Phase 6 and stayed consistent through Phases 14+15.

### What Was Inefficient

- **Audit line-number rot.** v0.1.6-MILESTONE-AUDIT.md cited `src/lib.rs:3823-3824` for the
  Pipeline zrangestore/zcount issue, but the dispatch arms had moved to lines 3110/3146 in commit
  `de9d259` weeks earlier. The audit nearly triggered a duplicate-code edit before Phase 15
  discussed the discrepancy. **Lesson:** Audit findings need verification against current code,
  not memorized line numbers.
- **Plan D-01 conflict in Phase 15.** The plan specified `redis.exceptions.NoScriptError` first
  in the resolution chain, but `burner_redis.NoScriptError` is a *subclass* of that — so raising
  the parent class breaks `pytest.raises(burner_redis.NoScriptError)`. Caught at first test run,
  but the conflict was logically deducible from D-04 alone. Plan-time review missed it.
  **Lesson:** When a plan locks both "raise X" and "test asserts subclass-of-X", trace the
  isinstance relationship in the actual import graph during planning.
- **VERIFICATION.md backfill drift.** 9 of 14 phases never produced VERIFICATION.md because the
  workflow was integrated mid-milestone. Audit caught this but it's now persistent debt.
  **Lesson:** When a workflow stage is added, retroactively backfill before the stage gets
  established as "the way we work."
- **Build error from `cargo build --release`.** The phase 15 first build attempt hit a Python
  linkage error (release builds need maturin's linker config). Cost one cycle to recognize
  `cargo check --lib` is the right Rust-side smoke check. **Lesson:** Document the build dance
  for mixed PyO3 projects in CLAUDE.md.

### Patterns Established

- **Audit closure as a dedicated phase.** When a milestone audit ships with `tech_debt` status
  and minor wiring/coverage gaps, a short focused phase (Phase 15) is preferable to leaving the
  debt indefinitely. Three issues, three tasks, one commit, one UAT, one SECURITY.md — clean close.
- **`burner_redis.X subclasses redis.exceptions.X` dual-class pattern.** Define a plain Exception
  class first, then conditionally re-define as a subclass of the redis-py equivalent inside a
  `try: import redis.exceptions; class X(...): ...; except: pass` block. Rust-side error helpers
  resolve to `burner_redis.X` first so both `except` forms work and `pytest.raises` passes.
- **Two-client save/restore as the canonical persistence test.** Any new ValueData variant
  needs both: (a) a Rust unit test entry in `test_round_trip_all_types` (additive seed + verify)
  and (b) a Python `test_X_persistence` exercising the BurnerRedis(persistence_path=) constructor.
- **Cross-check pipeline-vs-standalone in regression tests.** When a pipeline arm and a
  standalone pymethod share a Rust dispatch path, the regression test should call both with the
  same input and assert equality. Catches silent dispatch removals.
- **Branch on error-message prefix at the call site, never globally.** `make_response_error`
  stayed unchanged in Phase 15; the NOSCRIPT routing lives at the single evalsha Err arm.
  Generalizing whole-binding error sniffing was explicitly rejected (D-03).

### Key Lessons

1. **Audits decay.** Line numbers, file paths, and "this is missing" claims all rot. Verify
   audit findings against `git log`/`grep`/the current file before acting on them.
2. **Subclass relationships matter for `pytest.raises`.** `pytest.raises(Subclass)` does NOT
   catch instances of the parent class. When designing a Rust-to-Python error chain, raise the
   most-derived class first in the resolution order.
3. **Test against the artifact, not the assumption.** Phase 15 ran the test scripts directly
   against the installed wheel (not just the test suite) to confirm UAT outcomes — caught the
   D-01 issue immediately.
4. **Single-keyspace + single RwLock scales further than expected.** Six data types, Lua,
   pipelines, persistence — all on one HashMap with parking_lot RwLock. No need for sharded maps
   or fine-grained locking through v0.1.6.
5. **Workflow integration mid-milestone creates persistent debt.** VERIFICATION.md, VALIDATION.md,
   and SECURITY.md were all added mid-stream. The phases that predate them now lack those files
   permanently unless backfilled. Plan workflow rollouts at milestone boundaries.

### Cost Observations

- Model mix: Predominantly Opus for plan/execute, Sonnet for verifier. No Haiku usage.
- Sessions: ~25+ across the milestone (one per phase + ad-hoc quick tasks).
- Notable: Phase 15 took one session end-to-end (discuss → plan → execute → verify → secure).
  When a phase has a tight scope (3 tasks, all autonomous, all backed by automated tests),
  inline execution beats subagent spawning — full context stays in one place.

---

## Cross-Milestone Trends

(First milestone — populate on v0.1.7+.)
