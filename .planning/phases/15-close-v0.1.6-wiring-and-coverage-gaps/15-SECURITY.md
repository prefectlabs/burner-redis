---
phase: 15
slug: close-v0-1-6-wiring-and-coverage-gaps
status: verified
threats_open: 0
asvs_level: 1
created: 2026-04-27
---

# Phase 15 — Security

> Per-phase security contract: threat register, accepted risks, and audit trail.
> Source artifacts: `15-01-PLAN.md` (threat model), `15-01-SUMMARY.md` (deliverables), live codebase (mitigation verification).

---

## Trust Boundaries

| Boundary | Description | Data Crossing |
|----------|-------------|---------------|
| Python ↔ Rust binding (PyO3) | Internal error-class helper + prefix-match on a Rust-internal error string. No new boundary semantics introduced by this phase. | Internal error message (Rust `String` → Python exception type). Originates from `Store::evalsha` Lua-script-registry lookup, not from user input. |
| File system ↔ persistence load | No changes to wire format. Phase 15 only adds tests exercising the existing serialize/deserialize path. | Test-only data (3 ordered byte values: `alpha`/`bravo`/`charlie`, `a`/`b`/`c`). |
| Audit doc ↔ workflow tooling | New `historical_note:` YAML field added to `.planning/v0.1.6-MILESTONE-AUDIT.md` is informational only. | None — no consumer parses this field for behavior. |

---

## Threat Register

| Threat ID | Category | Component | Disposition | Mitigation | Status |
|-----------|----------|-----------|-------------|------------|--------|
| T-15-01 | Spoofing | `make_noscript_error` resolution chain (`src/lib.rs:98-126`) | accept | The chain only imports symbols that are already trusted module-level imports (`burner_redis`, `redis.exceptions`); spoofing would require having already compromised the Python import path, which is out of scope for this in-process embedded library. | closed |
| T-15-02 | Tampering | `msg.starts_with("NOSCRIPT")` prefix check (`src/lib.rs:1862-1868`) | mitigate | The `msg` argument originates from `Store::evalsha`'s internal Rust error path (not user-controlled). Mitigation verified in code: (a) exact-match, case-sensitive prefix check (`starts_with("NOSCRIPT")`); (b) single call site — `grep -c 'msg.starts_with("NOSCRIPT")' src/lib.rs` returns `1`; (c) `make_response_error` unchanged — no whole-binding NOSCRIPT sniffing per D-03. Worst-case impact of mis-routing: surfacing `NoScriptError` instead of `ResponseError`, both Python-side exceptions, no privilege boundary crossed. | closed |
| T-15-03 | Repudiation | (n/a) | accept | No logging or audit fields touched by this phase. | closed |
| T-15-04 | Information disclosure | NoScriptError message body | accept | The NOSCRIPT error message comes from the Lua script registry and contains a SHA1 hex string (the missing script's hash). SHA1 of an unknown script is not sensitive — it is precisely the value the caller already passed in. Existing `make_response_error` already returns the same message body for the un-routed case, so no information-disclosure delta. | closed |
| T-15-05 | Denial of service | Pipeline regression tests (`tests/test_pipeline.py`) | accept | Tests use bounded inputs (3-4 sorted set members, 3 list elements). No new code paths exposed to user input. | closed |
| T-15-06 | Elevation of privilege | (n/a) | accept | No auth boundary in the project (embedded in-process library; ACL out of scope per `REQUIREMENTS.md`). | closed |
| T-15-07 | Tampering | List persistence round-trip test data (`src/persistence.rs`, `tests/test_persistence.py`) | accept | Test inputs (`alpha`/`bravo`/`charlie`, `a`/`b`/`c`) are constants in test code, not user-supplied. | closed |

*Status: open · closed*
*Disposition: mitigate (implementation required) · accept (documented risk) · transfer (third-party)*

**Severity summary:** All threats LOW or N/A. The phase touches an internal error-routing helper, two Python regression tests, one Rust unit-test extension, and one audit-doc clarification — no new external attack surface, no new serialization/deserialization paths, no new auth boundary changes.

---

## Accepted Risks Log

| Risk ID | Threat Ref | Rationale | Accepted By | Date |
|---------|------------|-----------|-------------|------|
| AR-15-01 | T-15-01 | Import-path spoofing requires prior compromise of the Python module loader; out of scope for an embedded in-process library. | Phase 15 plan author (locked in 15-01-PLAN.md threat_model) | 2026-04-27 |
| AR-15-02 | T-15-03 | No logging/audit surface touched by this phase. | Phase 15 plan author | 2026-04-27 |
| AR-15-03 | T-15-04 | NOSCRIPT error message contains the caller-supplied SHA1; no information-disclosure delta vs. existing `make_response_error` path. | Phase 15 plan author | 2026-04-27 |
| AR-15-04 | T-15-05 | Tests use bounded constant inputs; no user-controlled DoS surface. | Phase 15 plan author | 2026-04-27 |
| AR-15-05 | T-15-06 | No auth boundary in this project per REQUIREMENTS.md scope. | Phase 15 plan author | 2026-04-27 |
| AR-15-06 | T-15-07 | Test inputs are hard-coded constants. | Phase 15 plan author | 2026-04-27 |

---

## Security Audit Trail

| Audit Date | Threats Total | Closed | Open | Run By |
|------------|---------------|--------|------|--------|
| 2026-04-27 | 7 | 7 | 0 | /gsd-secure-phase 15 (orchestrator inline; no external auditor agent — all dispositions either `accept` with documented rationale in PLAN.md threat_model or `mitigate` verified directly against `src/lib.rs:1862-1868`) |

### Verification Evidence (T-15-02 mitigation — the only `mitigate` disposition)

```rust
// src/lib.rs:1860-1872 — evalsha Err arm
Err(msg) => {
    if msg.starts_with("NOSCRIPT") {
        Err(make_noscript_error(msg))
    } else {
        Err(make_response_error(msg))
    }
}
```

- ✓ Exact-match, case-sensitive prefix check (no fuzzy/lowercase matching) — D-03 holds
- ✓ Single call site: `grep -c 'msg.starts_with("NOSCRIPT")' src/lib.rs` → `1`
- ✓ `make_response_error` (`src/lib.rs:79-96`) unchanged — git diff shows only `make_noscript_error` added and the evalsha Err arm modified
- ✓ Source of `msg`: `Store::evalsha` returns canonical Redis NOSCRIPT wording; not user-controlled

---

## Sign-Off

- [x] All threats have a disposition (mitigate / accept / transfer)
- [x] Accepted risks documented in Accepted Risks Log
- [x] `threats_open: 0` confirmed
- [x] `status: verified` set in frontmatter

**Approval:** verified 2026-04-27
