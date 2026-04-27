---
phase: 1
slug: foundation-and-string-commands
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-04-10
---

# Phase 1 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | pytest + pytest-asyncio (Python), cargo test (Rust) |
| **Config file** | pyproject.toml (pytest config), Cargo.toml (Rust) |
| **Quick run command** | `cargo test && python -m pytest tests/ -x -q` |
| **Full suite command** | `cargo test && python -m pytest tests/ -v` |
| **Estimated runtime** | ~15 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test && python -m pytest tests/ -x -q`
- **After every plan wave:** Run `cargo test && python -m pytest tests/ -v`
- **Before `/gsd-verify-work`:** Full suite must be green
- **Max feedback latency:** 15 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 1-01-01 | 01 | 1 | FOUND-01 | — | N/A | build | `cargo build && python -c "import burner_redis"` | ❌ W0 | ⬜ pending |
| 1-01-02 | 01 | 1 | FOUND-02 | — | N/A | unit | `python -m pytest tests/test_strings.py -x -q` | ❌ W0 | ⬜ pending |
| 1-01-03 | 01 | 1 | FOUND-03 | — | N/A | unit | `python -m pytest tests/test_strings.py -x -q` | ❌ W0 | ⬜ pending |
| 1-02-01 | 02 | 1 | STR-01 | — | N/A | unit | `python -m pytest tests/test_strings.py::test_set_get -x -q` | ❌ W0 | ⬜ pending |
| 1-02-02 | 02 | 1 | STR-02 | — | N/A | unit | `python -m pytest tests/test_strings.py::test_set_nx_xx -x -q` | ❌ W0 | ⬜ pending |
| 1-02-03 | 02 | 1 | STR-03 | — | N/A | unit | `python -m pytest tests/test_strings.py::test_set_ex_px -x -q` | ❌ W0 | ⬜ pending |
| 1-02-04 | 02 | 1 | STR-04 | — | N/A | unit | `python -m pytest tests/test_strings.py::test_get -x -q` | ❌ W0 | ⬜ pending |
| 1-02-05 | 02 | 1 | STR-05 | — | N/A | unit | `python -m pytest tests/test_strings.py::test_delete -x -q` | ❌ W0 | ⬜ pending |
| 1-02-06 | 02 | 1 | STR-06 | — | N/A | unit | `python -m pytest tests/test_strings.py::test_exists -x -q` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `tests/test_strings.py` — stubs for STR-01 through STR-06, FOUND-02, FOUND-03
- [ ] `tests/conftest.py` — shared fixture for BurnerRedis instance
- [ ] pytest + pytest-asyncio — install via pyproject.toml

*If none: "Existing infrastructure covers all phase requirements."*

---

## Manual-Only Verifications

*All phase behaviors have automated verification.*

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 15s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
