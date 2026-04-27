---
phase: 10
slug: add-pub-sub-support-subscribe-unsubscribe-publish-psubscribe
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-04-13
---

# Phase 10 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | pytest + cargo test |
| **Config file** | `pyproject.toml` (pytest), `Cargo.toml` (rust) |
| **Quick run command** | `cargo test --lib && uv run pytest tests/ -x -q` |
| **Full suite command** | `cargo test && uv run pytest tests/ -v` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test --lib && uv run pytest tests/ -x -q`
- **After every plan wave:** Run `cargo test && uv run pytest tests/ -v`
- **Before `/gsd-verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| TBD | TBD | TBD | TBD | — | N/A | unit/integration | TBD | ⬜ | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

*Note: Task IDs will be populated after PLAN.md files are created.*

---

## Wave 0 Requirements

- [ ] `tests/test_pubsub.py` — stubs for pub/sub command tests
- [ ] Rust unit tests in `src/commands/pubsub.rs` — for registry and pattern matching

*Existing test infrastructure (pytest, cargo test) covers framework needs.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| run_in_thread() daemon behavior | D-07 | Thread lifecycle hard to test deterministically | Start thread, publish messages, verify receipt, stop thread |

*Most pub/sub behaviors have automated verification via pytest integration tests.*

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
