# T02: Audit the Rust dependency tree with `cargo-bundle-licenses` and commit `THIRDPARTY.

**Slice:** S13 — **Milestone:** M001

## Description

Audit the Rust dependency tree with `cargo-bundle-licenses` and commit `THIRDPARTY.yml` to the repo root. `cargo-bundle-licenses` is invoked by conda-forge during feedstock builds; it fails loudly on ambiguous or missing licenses. Running it ourselves first avoids a mid-review surprise on the staged-recipes PR.

Purpose: Gate before Plan 03. If any dep is NOT FOUND / AMBIGUOUS or carries an incompatible license (MPL/GPL/AGPL/proprietary), fix it here — either by upgrading the dep, swapping it, or documenting an allowlist override with justification.

Output: `THIRDPARTY.yml` at repo root + `.planning/notes/phase-13-license-audit.md` summary.

## Legacy Source

---
phase: 13-publish-burner-redis-to-conda-forge
plan: 02
type: execute
wave: 2
depends_on:
  - 13-01
files_modified:
  - THIRDPARTY.yml
  - .planning/notes/phase-13-license-audit.md
autonomous: true
requirements: []

must_haves:
  truths:
    - "cargo-bundle-licenses exits 0 against the current Cargo.toml dep tree"
    - "THIRDPARTY.yml is committed at the repo root and contains at least one entry per direct dependency listed in Cargo.toml [dependencies]"
    - "No dep is flagged NOT FOUND or AMBIGUOUS in the tool output"
    - "Every license string in THIRDPARTY.yml falls in the permissive set (MIT, Apache-2.0, BSD-*, ISC, Unicode-3.0, Unlicense, CC0-1.0) — no GPL/AGPL/MPL/proprietary/unknown"
  artifacts:
    - path: "THIRDPARTY.yml"
      provides: "Vendored third-party Rust license text, meeting the conda-forge supply-chain requirement"
      contains: "pyo3"
    - path: ".planning/notes/phase-13-license-audit.md"
      provides: "Audit summary recording tool version, invocation, result, and the license-class distribution"
      contains: "license_summary"
  key_links:
    - from: ".planning/notes/phase-13-license-audit.md"
      to: "Plan 03 recipe.yaml `about.license` + `about.license_file`"
      via: "audit confirms THIRDPARTY.yml is safe to reference as license_file[] entry"
      pattern: "license_file"
---

<objective>
Audit the Rust dependency tree with `cargo-bundle-licenses` and commit `THIRDPARTY.yml` to the repo root. `cargo-bundle-licenses` is invoked by conda-forge during feedstock builds; it fails loudly on ambiguous or missing licenses. Running it ourselves first avoids a mid-review surprise on the staged-recipes PR.

Purpose: Gate before Plan 03. If any dep is NOT FOUND / AMBIGUOUS or carries an incompatible license (MPL/GPL/AGPL/proprietary), fix it here — either by upgrading the dep, swapping it, or documenting an allowlist override with justification.

Output: `THIRDPARTY.yml` at repo root + `.planning/notes/phase-13-license-audit.md` summary.
</objective>

<execution_context>
@$HOME/.claude/get-shit-done/workflows/execute-plan.md
@$HOME/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@.planning/PROJECT.md
@.planning/ROADMAP.md
@.planning/phases/13-publish-burner-redis-to-conda-forge/CONTEXT.md
@.planning/notes/conda-forge-feedstock-setup-research.md
@.planning/todos/pending/audit-rust-dep-licenses.md
@.planning/notes/phase-13-sdist-verification-report.md
@Cargo.toml

<interfaces>
<!-- Dependency surface from Cargo.toml [dependencies] (verbatim). -->

Direct dependencies to verify in THIRDPARTY.yml:
- pyo3 = "0.28.3"                         (expected: MIT OR Apache-2.0)
- pyo3-async-runtimes = "0.28.0"          (expected: MIT OR Apache-2.0)
- tokio = "1.51"                          (expected: MIT)
- parking_lot = "0.12.5"                  (expected: MIT OR Apache-2.0)
- bytes = "1.11"                          (expected: MIT)
- thiserror = "2.0"                       (expected: MIT OR Apache-2.0)
- ordered-float = "5"                     (expected: MIT)
- mlua = "0.10" features=[lua54,send,vendored]  (expected: MIT; also bundles Lua source under MIT)
- sha1 = "0.10"                           (expected: MIT OR Apache-2.0)
- serde = "1.0" features=["derive"]       (expected: MIT OR Apache-2.0)
- rmp-serde = "1.3"                       (expected: MIT)

No `[dev-dependencies]` block currently declared — criterion is not yet added. Only production deps are audited here.

Permissive license set accepted by conda-forge (no approval needed):
- MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Unicode-3.0, Unlicense, CC0-1.0, Zlib

Copyleft licenses that REQUIRE action if encountered:
- MPL-2.0 (file-level copyleft, typically acceptable but needs note)
- LGPL-* (dynamic linking only — must not statically link)
- GPL-*, AGPL-* (incompatible — swap the dep)
</interfaces>
</context>

<tasks>

<task type="auto">
  <name>Task 1: Install cargo-bundle-licenses and generate THIRDPARTY.yml</name>
  <files>
    - THIRDPARTY.yml (new)
    - .planning/notes/phase-13-license-audit.md (new)
  </files>
  <read_first>
    - Cargo.toml (full — snapshot the [dependencies] block for comparison)
    - .planning/todos/pending/audit-rust-dep-licenses.md (full — expected license shape)
    - .planning/notes/conda-forge-feedstock-setup-research.md (§ "Gotchas audited for burner-redis" row 2)
    - .planning/notes/phase-13-sdist-verification-report.md (to confirm Plan 01 is green before running this plan)
  </read_first>
  <action>
**Step 1.1 — Install cargo-bundle-licenses:**

```bash
cd /Users/alexander/dev/prefectlabs/burner-redis
cargo install cargo-bundle-licenses --locked
cargo bundle-licenses --version
```

Record the installed version string (e.g. `cargo-bundle-licenses 2.0.0`) in the audit note.

**Step 1.2 — Generate THIRDPARTY.yml:**

```bash
cd /Users/alexander/dev/prefectlabs/burner-redis
cargo bundle-licenses --format yaml --output THIRDPARTY.yml 2>&1 | tee /tmp/phase-13-bundle-licenses.log
echo "exit=$?"
```

If exit is non-zero, STOP and jump to Task 2 (remediation). If exit is zero, continue.

**Step 1.3 — Sanity-check the generated file:**

```bash
# File must exist and be non-empty
test -s THIRDPARTY.yml && echo "THIRDPARTY.yml size OK"

# Every direct dep from Cargo.toml [dependencies] must appear at least once
for dep in pyo3 pyo3-async-runtimes tokio parking_lot bytes thiserror ordered-float mlua sha1 serde rmp-serde; do
  grep -qE "^- name: ${dep}$" THIRDPARTY.yml && echo "  $dep: present" || echo "  $dep: MISSING"
done
```

If any direct dep is MISSING, jump to Task 2.

**Step 1.4 — Scan for disallowed license strings:**

```bash
# Must NOT appear anywhere in THIRDPARTY.yml
grep -E '^\s*license:\s*.*(GPL|AGPL|NOT FOUND|AMBIGUOUS|UNKNOWN|Proprietary)' THIRDPARTY.yml \
  && echo "DISALLOWED LICENSE DETECTED" \
  || echo "license scan clean"
```

Any hit here is a blocker — jump to Task 2. Note: `LGPL` substring is allowed only via a dependency we explicitly dynamically link (none currently expected).

**Step 1.5 — Write the audit summary note:**

Create `.planning/notes/phase-13-license-audit.md` with this structure:

```markdown
---
title: Phase 13 — Rust dependency license audit
date: <YYYY-MM-DD>
phase: 13
step: 2
tool_version: "<cargo-bundle-licenses version from Step 1.1>"
result: "PASS"              # "PASS" or "REMEDIATED" or "BLOCKED"
---

# Dependency license audit

## Invocation
`cargo bundle-licenses --format yaml --output THIRDPARTY.yml`
Exit code: 0

## Direct deps verified
<paste Step 1.3 output>

## License class distribution
<run: `grep -E '^\s*license:' THIRDPARTY.yml | sort | uniq -c | sort -rn`>

## Disallowed-license scan
<paste Step 1.4 result>

## Remediations (if any)
<empty unless Task 2 ran>
```
  </action>
  <verify>
    <automated>cd /Users/alexander/dev/prefectlabs/burner-redis && test -s THIRDPARTY.yml && grep -qE '^- name: pyo3$' THIRDPARTY.yml && ! grep -qE '^\s*license:\s*.*(GPL|AGPL|NOT FOUND|AMBIGUOUS|UNKNOWN|Proprietary)' THIRDPARTY.yml && echo PASS || echo FAIL</automated>
  </verify>
  <acceptance_criteria>
    - `cargo bundle-licenses --format yaml --output THIRDPARTY.yml` exits with code 0 (stdout/stderr captured in the audit note)
    - `THIRDPARTY.yml` exists at the repo root and is non-empty
    - `grep -E '^- name: <dep>$' THIRDPARTY.yml` returns a hit for EACH of: pyo3, pyo3-async-runtimes, tokio, parking_lot, bytes, thiserror, ordered-float, mlua, sha1, serde, rmp-serde
    - `grep -E '^\s*license:\s*.*(GPL|AGPL|NOT FOUND|AMBIGUOUS|UNKNOWN|Proprietary)' THIRDPARTY.yml` returns NO matches
    - `.planning/notes/phase-13-license-audit.md` exists with frontmatter `result: "PASS"` or `"REMEDIATED"`
  </acceptance_criteria>
  <done>THIRDPARTY.yml is committed, every direct dep is represented, and no disallowed license string is present.</done>
</task>

<task type="auto">
  <name>Task 2: (CONDITIONAL — run only if Task 1 flagged a dep) Remediate ambiguous or disallowed licenses</name>
  <files>
    - Cargo.toml (if a dep upgrade/swap is required)
    - THIRDPARTY.yml (regenerated)
    - .planning/notes/phase-13-license-audit.md (append remediation record)
  </files>
  <read_first>
    - /tmp/phase-13-bundle-licenses.log (the specific error from Task 1)
    - THIRDPARTY.yml (the partial or problematic output from Task 1)
    - The crate's page on https://crates.io/crates/<flagged-dep> via WebFetch — confirm actual upstream license
  </read_first>
  <action>
Skip this task if Task 1 passed. Otherwise, apply one remediation per flagged dep in this priority order (per CONTEXT.md Risk #2):

**Priority 1 — Upgrade the dep:**
A newer version of the crate often declares its license more cleanly. Example:

```bash
cargo update -p <flagged-dep>
# or for a semver bump, edit Cargo.toml and `cargo build`
```

Re-run `cargo bundle-licenses --format yaml --output THIRDPARTY.yml`. If still flagged, try Priority 2.

**Priority 2 — Swap the dep:**
If the crate is unmaintained or has a genuinely bad license (GPL, unknown), find a permissive replacement. Document the replacement path in the audit note. Only swap if the API surface is compatible — otherwise escalate to the developer.

**Priority 3 — Explicit allowlist override (last resort):**
Only if (a) the crate is a transitive dep we cannot swap AND (b) we have verified via the upstream repo that the license text IS a permissive standard license but is just badly declared in the manifest. Use `cargo-bundle-licenses`'s explicit override syntax. Commit the override AND a justification paragraph in `.planning/notes/phase-13-license-audit.md` linking to the upstream LICENSE file that proves the real license.

**After remediation:**

```bash
cd /Users/alexander/dev/prefectlabs/burner-redis
cargo bundle-licenses --format yaml --output THIRDPARTY.yml
echo "exit=$?"
```

Repeat Task 1 Steps 1.3 and 1.4 checks. Update `.planning/notes/phase-13-license-audit.md`:
- Change frontmatter `result:` to `"REMEDIATED"`
- Fill in the `## Remediations (if any)` section with, for each flagged dep:
  - name, offending license string, action taken (upgrade/swap/override), new license string, URL to upstream proof

If after three remediation attempts the audit still fails, set `result: "BLOCKED"` in the frontmatter, commit the current state, and halt the plan — this requires developer escalation.
  </action>
  <verify>
    <automated>cd /Users/alexander/dev/prefectlabs/burner-redis && cargo bundle-licenses --format yaml --output /tmp/phase-13-THIRDPARTY-recheck.yml 2>&1 | tee /tmp/phase-13-recheck.log && test $? -eq 0 && echo PASS || echo FAIL</automated>
  </verify>
  <acceptance_criteria>
    - `cargo bundle-licenses --format yaml --output THIRDPARTY.yml` re-run exits with code 0
    - `.planning/notes/phase-13-license-audit.md` frontmatter has `result: "REMEDIATED"` (not `"BLOCKED"`)
    - The `## Remediations (if any)` section is non-empty and lists each originally-flagged dep with a concrete action (upgrade / swap / override) and an upstream-proof URL
    - THIRDPARTY.yml passes the same disallowed-license scan as Task 1 Step 1.4
  </acceptance_criteria>
  <done>All flagged deps are resolved and THIRDPARTY.yml passes the audit. If escalation was required (result: BLOCKED), surface this to the developer before Plan 03 runs.</done>
</task>

</tasks>

<verification>
- `THIRDPARTY.yml` exists at repo root, non-empty, and committed.
- `cargo bundle-licenses` exits 0 (capturable in CI as well).
- `.planning/notes/phase-13-license-audit.md` records the tool version and audit outcome.
- No disallowed license strings (GPL/AGPL/UNKNOWN/NOT FOUND/AMBIGUOUS) remain.
</verification>

<success_criteria>
- Step 2 from CONTEXT.md ("Audit Rust dep licenses") is complete.
- The hard gate in CONTEXT.md ("Do not open the staged-recipes PR until Steps 1 AND 2 both pass") is now ready to clear — Plan 03 can proceed.
- THIRDPARTY.yml serves as an in-repo artifact conda-forge recipe can reference via `about.license_file` if the reviewer asks.
</success_criteria>

<output>
After completion, create `.planning/phases/13-publish-burner-redis-to-conda-forge/13-02-SUMMARY.md` capturing:
- cargo-bundle-licenses version used
- Direct-dep coverage confirmation
- Whether Task 2 remediation ran (and if so, which deps were remediated and how)
- A one-line "next: Plan 03 recipe submission" pointer
</output>
