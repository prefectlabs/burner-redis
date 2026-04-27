---
phase: 11
slug: close-redis-py-compatibility-gaps-for-pydocket-integration
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-04-14
---

# Phase 11 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | pytest 7.x |
| **Config file** | `pyproject.toml` |
| **Quick run command** | `uv run pytest tests/test_pydocket_compat.py -x` |
| **Full suite command** | `uv run pytest tests/ -x` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `uv run pytest tests/test_pydocket_compat.py -x`
- **After every plan wave:** Run `uv run pytest tests/ -x`
- **Before `/gsd-verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 11-01-01 | 01 | 1 | D-06, D-07, D-08 | T-11-01, T-11-02 | Block duration capped by timeout; lock released before waiting | unit | `.venv/bin/python -m pytest tests/test_streams.py -k "test_xreadgroup_block" -x -q --tb=short` | Wave 0 | pending |
| 11-01-02 | 01 | 1 | D-03, D-06 | T-11-03, T-11-05 | Validate argument count before processing XCLAIM | unit | `.venv/bin/python -m pytest tests/test_streams.py -k "test_xclaim or test_xtrim_accepts_approximate" -x -q --tb=short` | Wave 0 | pending |
| 11-02-01 | 02 | 2 | D-01, D-02, D-04, D-05 | T-11-06 | Test fixture monkey-patch is test-only | integration | `.venv/bin/python -m pytest tests/test_pydocket_compat.py -m integration --runxfail -v --tb=short` | Yes | pending |
| 11-02-02 | 02 | 2 | D-09, D-10 | -- | N/A | unit+integration | `.venv/bin/python -m pytest tests/ -q --tb=short -x && .venv/bin/python -m pytest tests/ -q -m integration --tb=short -x` | Partial | pending |

*Status: pending · green · red · flaky*

---

## Wave 0 Requirements

*Existing infrastructure covers all phase requirements.*

---

## Manual-Only Verifications

*All phase behaviors have automated verification.*

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
