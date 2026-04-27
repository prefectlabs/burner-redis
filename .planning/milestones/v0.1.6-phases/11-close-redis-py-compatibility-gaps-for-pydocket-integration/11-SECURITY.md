---
phase: 11
slug: close-redis-py-compatibility-gaps-for-pydocket-integration
status: verified
threats_open: 0
asvs_level: 1
created: 2026-04-14
---

# Phase 11 — Security

> Per-phase security contract: threat register, accepted risks, and audit trail.

---

## Trust Boundaries

| Boundary | Description | Data Crossing |
|----------|-------------|---------------|
| Python -> Rust | User-provided arguments (stream IDs, consumer names) cross into Rust via PyO3 | Stream IDs, consumer names, group names, message payloads |
| Lua -> Store | Lua scripts dispatch commands with user-controlled arguments | Redis command arguments including XCLAIM parameters |
| Test fixture -> BurnerRedis | Test monkey-patches inject BurnerRedis into pydocket's Redis connection path | Test data only |

---

## Threat Register

| Threat ID | Category | Component | Disposition | Mitigation | Status |
|-----------|----------|-----------|-------------|------------|--------|
| T-11-01 | D (DoS) | XREADGROUP block | mitigate | `src/lib.rs:1189` — `tokio::select!` branches on `notify.notified()` and `tokio::time::sleep(timeout_duration)` where duration is from user-supplied `block_ms`. No unbounded wait path. | closed |
| T-11-02 | D (DoS) | Store::stream_notify | accept | Global Notify spurious wakeups — see Accepted Risks Log | closed |
| T-11-03 | T (Tampering) | XCLAIM | accept | PEL transfer without auth — see Accepted Risks Log | closed |
| T-11-04 | I (Info Disclosure) | XCLAIM | accept | Message content revealed to claimer — see Accepted Risks Log | closed |
| T-11-05 | D (DoS) | Lua XCLAIM dispatch | mitigate | `src/scripting.rs:1462` — `if args.len() < 5` guard returns ERR before parsing. `min_idle_time` parse failure returns ERR via `map_err`. No panic path. | closed |
| T-11-06 | T (Tampering) | Test fixture monkey-patch | accept | Test-only code path — see Accepted Risks Log | closed |
| T-11-07 | D (DoS) | pydocket test suite clone | accept | Temporary /tmp clone — see Accepted Risks Log | closed |

*Status: open / closed*
*Disposition: mitigate (implementation required) / accept (documented risk) / transfer (third-party)*

---

## Accepted Risks Log

| Risk ID | Threat Ref | Rationale | Accepted By | Date |
|---------|------------|-----------|-------------|------|
| AR-01 | T-11-02 | Global Notify wakes all blocked readers on any XADD. Spurious wakeup causes one O(1) re-read. Acceptable for embedded single-process use. | burner-redis maintainers | 2026-04-14 |
| AR-02 | T-11-03 | XCLAIM transfers PEL ownership without auth. In-process DB with no network exposure — all callers share same trust level. Auth out of scope per REQUIREMENTS.md. | burner-redis maintainers | 2026-04-14 |
| AR-03 | T-11-04 | XCLAIM reveals message content to claiming consumer. Same trust boundary as all read ops (XREADGROUP, XRANGE). In-process use means all readers equally trusted. | burner-redis maintainers | 2026-04-14 |
| AR-04 | T-11-06 | Test fixture monkey-patches RedisConnection. Test-only code path, no production impact. Intended mechanism for drop-in replacement testing. | burner-redis maintainers | 2026-04-14 |
| AR-05 | T-11-07 | Temporary pydocket clone to /tmp for gap inventory. One-time plan-time step, cleaned up after use. No persistence or production impact. | burner-redis maintainers | 2026-04-14 |

---

## Unregistered Threat Flags

None. No threat flags raised in summaries without a mapping to the threat register.

---

## Security Audit Trail

| Audit Date | Threats Total | Closed | Open | Run By |
|------------|---------------|--------|------|--------|
| 2026-04-14 | 7 | 7 | 0 | gsd-security-auditor |

---

## Sign-Off

- [x] All threats have a disposition (mitigate / accept / transfer)
- [x] Accepted risks documented in Accepted Risks Log
- [x] `threats_open: 0` confirmed
- [x] `status: verified` set in frontmatter

**Approval:** verified 2026-04-14
