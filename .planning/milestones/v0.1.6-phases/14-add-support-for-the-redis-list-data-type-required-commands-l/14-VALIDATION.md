---
phase: 14
slug: add-support-for-the-redis-list-data-type-required-commands-l
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-04-24
---

# Phase 14 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | pytest 7.x with `asyncio_mode = "auto"` (existing) + Rust `cargo test` for store-level unit tests |
| **Config file** | `pyproject.toml` (pytest config), `tests/conftest.py` (fixture `r`) — already installed, no Wave 0 setup needed |
| **Quick run command** | `uv run pytest tests/test_lists.py -x` |
| **Full suite command** | `uv run pytest && cargo test --lib` |
| **Estimated runtime** | ~30s (pytest), ~8s (cargo test) |

---

## Sampling Rate

- **After every task commit:** Run `uv run pytest tests/test_lists.py -x -k <command_name>` (scoped to the command(s) touched)
- **After every plan wave:** Run `uv run pytest tests/test_lists.py && cargo test --lib`
- **Before `/gsd-verify-work`:** Full suite (`uv run pytest && cargo test --lib`) must be green
- **Max feedback latency:** 30s

---

## Per-Task Verification Map

> Concrete task IDs assigned by the planner. This table enumerates the behavioral contract. Each row maps a LIST-* requirement (to be added to REQUIREMENTS.md per CONTEXT D-21) to its test anchor.

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| TBD | 01 (engine) | 1 | LIST-01 (ValueData variant) | — | WRONGTYPE on non-list ops against string keys | cargo unit | `cargo test --lib store::tests::list_` | ❌ W0 | ⬜ pending |
| TBD | 01 (engine) | 1 | LIST-02 (LPUSH/RPUSH) | — | Multi-value insertion order matches redis-py | pytest | `uv run pytest tests/test_lists.py::test_lpush_rpush -x` | ❌ W0 | ⬜ pending |
| TBD | 01 (engine) | 1 | LIST-03 (LPOP/RPOP count semantics) | — | count=None→bytes, count=0→[], count=N→list, missing→None | pytest | `uv run pytest tests/test_lists.py::test_lpop_rpop_count -x` | ❌ W0 | ⬜ pending |
| TBD | 01 (engine) | 1 | LIST-04 (LRANGE negative-index) | — | All 9 index-matrix cases pass | pytest | `uv run pytest tests/test_lists.py::test_lrange -x` | ❌ W0 | ⬜ pending |
| TBD | 01 (engine) | 1 | LIST-05 (LLEN) | — | Returns 0 for missing, correct length for existing | pytest | `uv run pytest tests/test_lists.py::test_llen -x` | ❌ W0 | ⬜ pending |
| TBD | 01 (engine) | 1 | LIST-06 (LINDEX) | — | Negative indices, out-of-range returns None | pytest | `uv run pytest tests/test_lists.py::test_lindex -x` | ❌ W0 | ⬜ pending |
| TBD | 01 (engine) | 1 | LIST-07 (LINSERT) | — | BEFORE/AFTER, pivot-not-found returns -1, missing key returns 0 | pytest | `uv run pytest tests/test_lists.py::test_linsert -x` | ❌ W0 | ⬜ pending |
| TBD | 01 (engine) | 1 | LIST-08 (LREM count-sign) | — | Positive=head, negative=tail, 0=all; returns removed count | pytest | `uv run pytest tests/test_lists.py::test_lrem -x` | ❌ W0 | ⬜ pending |
| TBD | 01 (engine) | 1 | LIST-09 (LSET) | — | Out-of-range raises ResponseError with Redis wording | pytest | `uv run pytest tests/test_lists.py::test_lset -x` | ❌ W0 | ⬜ pending |
| TBD | 01 (engine) | 1 | LIST-10 (LTRIM) | — | Empty-after-trim deletes key | pytest | `uv run pytest tests/test_lists.py::test_ltrim -x` | ❌ W0 | ⬜ pending |
| TBD | 01 (engine) | 1 | LIST-11 (LMOVE/RPOPLPUSH) | — | Same-key rotation valid, cross-key atomic under single write lock | pytest | `uv run pytest tests/test_lists.py::test_lmove -x` | ❌ W0 | ⬜ pending |
| TBD | 01 (engine) | 1 | LIST-12 (BRPOP/BLPOP blocking) | — | timeout=0 blocks forever; multi-key scans in order; returns (key, value) tuple; None on timeout | pytest | `uv run pytest tests/test_lists.py::test_brpop_blpop -x` | ❌ W0 | ⬜ pending |
| TBD | 01 (engine) | 1 | LIST-13 (BLMOVE) | — | Cross-key atomicity, direction args, timeout semantics | pytest | `uv run pytest tests/test_lists.py::test_blmove -x` | ❌ W0 | ⬜ pending |
| TBD | 01 (engine) | 1 | LIST-14 (asyncio cancellation safety) | — | asyncio.CancelledError during block: no partial state, Rust future cancel-safe | pytest | `uv run pytest tests/test_lists.py::test_blocking_cancellation -x` | ❌ W0 | ⬜ pending |
| TBD | 02 (python surface) | 2 | LIST-15 (Pipeline blocking + non-blocking) | — | Non-blocking fast path preserved; blocking commands fall through to async loop | pytest | `uv run pytest tests/test_lists.py::test_pipeline_blocking_mix -x` | ❌ W0 | ⬜ pending |
| TBD | 02 (python surface) | 2 | LIST-16 (Lua integration) | — | Non-blocking commands dispatch in Lua; blocking commands return error matching Redis wording; LPUSH-in-Lua wakes BRPOP waiter | pytest | `uv run pytest tests/test_lists.py::test_lua_lists -x` | ❌ W0 | ⬜ pending |
| TBD | 02 (python surface) | 2 | REQUIREMENTS.md update | — | Out-of-Scope entry removed; LIST-01..LIST-16 added to Traceability with Phase 14 mapping | grep | `grep -q "LIST-01" .planning/REQUIREMENTS.md` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `tests/test_lists.py` — new pytest module; all LIST-01..LIST-16 test stubs created before implementation begins (optional if the planner uses TDD ordering per task)
- [ ] `REQUIREMENTS.md` — LIST-* section added (happens inside the phase, not pre-phase)
- [ ] Rust unit tests under `#[cfg(test)] mod tests { ... }` in `src/store.rs` for list-level mutations

*Existing infrastructure covers all other phase needs — pytest, asyncio, `conftest.py` fixture `r`, `cargo test --lib`, redis-py dev dep all in place.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Shutdown-during-block race | LIST-12 / LIST-13 | Requires exercising `Store::shutdown()` while a blocking call is mid-`select!`. Automatable but non-deterministic without a timing shim. | Start `BurnerRedis`, launch `asyncio.create_task(r.blpop("k", timeout=0))`, call `await r.aclose()` within 100ms, assert the task completes with `None` rather than hanging. |
| Benchmark regression — non-blocking pipeline fast path | D-16 | Requires criterion baseline comparison; not part of CI. | `cargo bench --bench pipeline_bench` (if added) or one-shot Python `timeit` comparing pre/post pipeline of 1000 LPUSH/LRANGE ops — must stay within 5% of Phase 7 baseline. |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
