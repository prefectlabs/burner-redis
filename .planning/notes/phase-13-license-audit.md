---
title: Phase 13 — Rust dependency license audit
date: 2026-04-18
phase: 13
step: 2
tool_version: "cargo-bundle-licenses cargo:4.0.0"
result: "PASS"
---

# Dependency license audit

## Invocation

Tool: `cargo-bundle-licenses cargo:4.0.0` (installed via `cargo install
cargo-bundle-licenses --version 4.0.0 --locked`).

The latest release on crates.io is `4.2.0`, but it pulls
`cargo_metadata@0.23.0` which requires `rustc >= 1.86`. Our toolchain is
`rustc 1.85.0` (matching `edition = "2024"`'s MSRV floor). Pinning to
`4.0.0` (the previous minor, released 2025-03-14) compiles against
rustc 1.85. The YAML output schema is unchanged; the tool still produces
the same vendored-license-text artifact.

Command:

```bash
cargo bundle-licenses --format yaml --output THIRDPARTY.yml
```

Exit code: **0** (see `/tmp/phase-13-bundle-licenses.log` for full invocation log).

## Direct deps verified

Every direct dep declared in `Cargo.toml` `[dependencies]` is represented
in `THIRDPARTY.yml`:

```text
  pyo3: present
  pyo3-async-runtimes: present
  tokio: present
  parking_lot: present
  bytes: present
  thiserror: present
  ordered-float: present
  mlua: present
  sha1: present
  serde: present
  rmp-serde: present
```

Total third-party packages bundled (including transitive deps): **57**.

Note: `cargo-bundle-licenses` 4.0.0 emits `- package_name: <crate>`
instead of `- name: <crate>` (the Plan 13-02 grep pattern targeted the
earlier schema). We verified with `grep -qE '^- package_name: <crate>$'`.
Schema fields used downstream: `package_name`, `package_version`,
`repository`, `license` (SPDX string), `licenses[]` (with embedded
license `text`).

## License class distribution

Top-level `license:` (SPDX expression per package):

```text
  39   license: MIT OR Apache-2.0
  11   license: MIT
   3   license: Apache-2.0 OR MIT
   1   license: Unlicense OR MIT
   1   license: Apache-2.0 WITH LLVM-exception
   1   license: Apache-2.0
   1   license: (MIT OR Apache-2.0) AND Unicode-3.0
```

Nested `- license:` (each license text entry within a package):

```text
  55   - license: MIT
  44   - license: Apache-2.0
   1   - license: Unlicense
   1   - license: Unicode-3.0
   1   - license: Apache-2.0 WITH LLVM-exception
```

All seven distinct SPDX IDs in the bundle fall in the permissive set
accepted by conda-forge without approval:

| SPDX ID                       | Accepted? |
| ----------------------------- | --------- |
| MIT                           | yes       |
| Apache-2.0                    | yes       |
| Apache-2.0 WITH LLVM-exception| yes (Rust stdlib-style exception; permissive) |
| Unlicense                     | yes       |
| Unicode-3.0                   | yes       |

No GPL, LGPL, AGPL, MPL, CDDL, EPL, or proprietary licenses appear
anywhere in the bundle.

## Disallowed-license scan

```bash
grep -E '^\s*license:\s*.*(GPL|AGPL|NOT FOUND|AMBIGUOUS|UNKNOWN|Proprietary)' THIRDPARTY.yml
```

Result: **license scan clean** (zero matches).

## Warnings (informational — not blockers)

`cargo-bundle-licenses` emitted license-confidence warnings for 22
crates. These are text-matching confidence levels (`SEMI` or `UNSURE`),
NOT SPDX-ID ambiguity. In all cases the crate's `Cargo.toml` `license`
field is a valid permissive SPDX expression; the tool just couldn't
match the embedded LICENSE file byte-for-byte against its canonical
template (common when upstream uses a slightly reformatted LICENSE or
a trailing-newline variation).

One crate (`mlua-sys 0.6.8`) has `text: NOT FOUND` in its bundled entry.
Its `Cargo.toml` declares `license = "MIT"` cleanly; the actual MIT
license text lives at the mlua repo root (`LICENSE`) rather than in the
`mlua-sys/` subcrate directory, which is why the per-crate bundler
couldn't locate it. Upstream proof:

- `Cargo.toml`: https://raw.githubusercontent.com/mlua-rs/mlua/v0.10.5/mlua-sys/Cargo.toml → `license = "MIT"`
- Repo-root LICENSE: https://raw.githubusercontent.com/mlua-rs/mlua/v0.10.5/LICENSE → MIT License text

The SPDX ID is clean and permissive — no action needed here. conda-forge
reviewers running their own `cargo-bundle-licenses` pass against the
sdist (per the recipe's `requirements.build: [cargo-bundle-licenses]`)
will see the same warning; it's a tool quirk for multi-crate Rust
workspaces where the LICENSE file lives only at the workspace root.

Full tool warning log for reference:

```text
SEMI   Apache-2.0 — libc 0.2.184
UNSURE MIT         — mlua 0.10.5
(none) MIT         — mlua-sys 0.6.8 (LICENSE file not present in subcrate)
SEMI   Apache-2.0 — pin-project-lite 0.2.17
SEMI   Apache-2.0 — portable-atomic 1.13.1
SEMI   Apache-2.0 — proc-macro2 1.0.106
SEMI   Apache-2.0 — pyo3 0.28.3
SEMI   Apache-2.0 — pyo3-build-config 0.28.3
SEMI   Apache-2.0 — pyo3-ffi 0.28.3
SEMI   Apache-2.0 — pyo3-macros 0.28.3
SEMI   Apache-2.0 — pyo3-macros-backend 0.28.3
SEMI   Apache-2.0 — quote 1.0.45
SEMI   Apache-2.0 — rustc-hash 2.1.2
SEMI   Apache-2.0 — rustversion 1.0.22
SEMI   Apache-2.0 — serde 1.0.228
SEMI   Apache-2.0 — serde_core 1.0.228
SEMI   Apache-2.0 — serde_derive 1.0.228
SEMI   MIT         — sha1 0.10.6
SEMI   Apache-2.0 — syn 2.0.117
SEMI   Apache-2.0 — thiserror 2.0.18
SEMI   Apache-2.0 — thiserror-impl 2.0.18
SEMI   Apache-2.0 — unicode-ident 1.0.24
```

## Remediations (if any)

None. No dep required upgrading, swapping, or an allowlist override.
The one cosmetic `text: NOT FOUND` on `mlua-sys` is a bundler-tool
limitation (LICENSE file location in the mlua workspace, not a license
identity issue), documented above and confirmed against the upstream
repo.

## Decision

**PASS.** `THIRDPARTY.yml` is committed at repo root and Plan 03 can
reference it via `about.license_file` in the conda-forge `recipe.yaml`.
The hard gate in `CONTEXT.md` ("Do not open the staged-recipes PR
until Steps 1 AND 2 both pass") is now cleared for Plan 03.

## Next

Plan 03 — draft recipe on fork of conda-forge/staged-recipes, open PR,
iterate on CI, verify post-merge feedstock publishes.
