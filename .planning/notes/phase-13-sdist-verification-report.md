---
title: Phase 13 — sdist feedstock-readiness verification
date: 2026-04-18
phase: 13
step: 1
pinned_version: "0.1.2"
sdist_url: "https://files.pythonhosted.org/packages/a4/30/8b219fc8863c652ef294d9a6075752cf14eade2f050e956410873f6f0270/burner_redis-0.1.2.tar.gz"
sdist_filename: "burner_redis-0.1.2.tar.gz"
sha256: "189698190835809f73fdb5af9ead4962975181c7fc8297045a5d831c0d465add"
---

# sdist feedstock-readiness verification

## Overview

This report records the Phase 13 Plan 01 gate: prove the PyPI sdist for
burner-redis 0.1.2 is feedstock-ready for conda-forge. The two contract fields
downstream plans consume are:

- `pinned_version` — locked to **0.1.2** (the 0.1.2 sdist passed audit; no 0.1.3 bump required).
- `sha256` — `189698190835809f73fdb5af9ead4962975181c7fc8297045a5d831c0d465add`.

These two values feed Plan 03's `recipe.yaml` `source.url` and `source.sha256`.

## Step 1.1 — PyPI index fetch

Command:

```bash
curl -sSL https://pypi.org/pypi/burner-redis/0.1.2/json > pypi.json
```

Exit: 0. JSON parsed via Python; resolved:

| field      | value                                                                                                                   |
| ---------- | ----------------------------------------------------------------------------------------------------------------------- |
| URL        | `https://files.pythonhosted.org/packages/a4/30/8b219fc8863c652ef294d9a6075752cf14eade2f050e956410873f6f0270/burner_redis-0.1.2.tar.gz` |
| FILENAME   | `burner_redis-0.1.2.tar.gz` (PEP 625 normalized — underscore)                                                           |
| SHA256     | `189698190835809f73fdb5af9ead4962975181c7fc8297045a5d831c0d465add`                                                      |

## Step 1.2 — sdist download + sha256 verify

Commands:

```bash
curl -sSL "$SDIST_URL" -o burner_redis-0.1.2.tar.gz
shasum -a 256 burner_redis-0.1.2.tar.gz
```

- `expected=189698190835809f73fdb5af9ead4962975181c7fc8297045a5d831c0d465add`
- `actual=189698190835809f73fdb5af9ead4962975181c7fc8297045a5d831c0d465add`
- Result: **sha256 OK** (exit 0)

## Step 1.3 — Archive contents audit

Command:

```bash
tar -tzf burner_redis-0.1.2.tar.gz | sort > sdist-contents.txt
wc -l sdist-contents.txt   # 190 entries
```

Required-files grep results:

```text
Cargo.toml: OK
Cargo.lock: OK
pyproject.toml: OK
src/lib.rs: OK
vendor/: absent (builder will fetch from crates.io — OK if online, risky if air-gapped)
```

Root-level manifest entries (matched `^[^/]+/Cargo\.(toml|lock)$`):

```text
burner_redis-0.1.2/Cargo.lock
burner_redis-0.1.2/Cargo.toml
burner_redis-0.1.2/pyproject.toml
burner_redis-0.1.2/PKG-INFO
burner_redis-0.1.2/LICENSE
burner_redis-0.1.2/README.md
burner_redis-0.1.2/src/lib.rs
```

All required files are present at the tarball root (`burner_redis-0.1.2/...`).
`Cargo.lock` is shipped by maturin's default sdist packager — no pyproject.toml
fix required, and consequently no 0.1.3 release cut. Tasks 2 and 3 of Plan 01
skipped.

**vendor/ note:** The sdist does not vendor Rust deps. conda-forge builders are
typically able to reach crates.io (recipe builds are NOT strictly air-gapped in
practice; the research note's concern is defensive). Task 4 validates offline
build via `CARGO_NET_OFFLINE=true` with a pre-populated cache, which is the
realistic "offline" failure mode that would also break conda-forge CI.

## Offline build audit

### Step 4.1 — Pre-populate cargo cache

Command (from project root):

```bash
cargo fetch --locked
```

Exit: 0. Post-fetch cache size:

```text
CARGO_HOME=/Users/alexander/.cargo
$CARGO_HOME/registry/cache/index.crates.io-1949cf8c6b5b557f/ → 641 .crate files
```

### Step 4.2 — Offline pip install from the PyPI sdist

Environment: fresh uv-managed venv at `/tmp/phase-13-sdist-check/.venv-offline`
with `maturin==1.13.1` and `pip==26.0.1` installed (one-time online, before the
offline flip). Then:

```bash
cd /tmp/phase-13-sdist-check
source .venv-offline/bin/activate
CARGO_NET_OFFLINE=true pip install --no-build-isolation --no-index --no-deps \
    burner_redis-0.1.2.tar.gz
```

- `--no-index` — pip refuses to consult PyPI for any dependency.
- `--no-build-isolation` — pip uses the already-installed maturin, no pip-side
  fetch of build tooling.
- `CARGO_NET_OFFLINE=true` — cargo refuses to hit crates.io; must resolve from
  `$CARGO_HOME/registry/cache` (pre-populated in Step 4.1).

Exit code: **0** (see `Successfully built burner-redis` / `Successfully
installed burner-redis-0.1.2` in the log).

Last 11 lines of `offline-build.log` (full log — the entire transcript):

```text
Processing ./burner_redis-0.1.2.tar.gz
  Preparing metadata (pyproject.toml): started
  Preparing metadata (pyproject.toml): finished with status 'done'
Building wheels for collected packages: burner-redis
  Building wheel for burner-redis (pyproject.toml): started
  Building wheel for burner-redis (pyproject.toml): finished with status 'done'
  Created wheel for burner-redis: filename=burner_redis-0.1.2-cp310-abi3-macosx_11_0_arm64.whl size=1151858 sha256=4615db4fa5907173b2c84ea1e849d8eb7c0ade753a998440f3b919373cd9472a
  Stored in directory: /Users/alexander/Library/Caches/pip/wheels/fa/5d/ae/b1ef211bfad61fef044542905d51ecf7c102d5563e1645c1be
Successfully built burner-redis
Installing collected packages: burner-redis
Successfully installed burner-redis-0.1.2
```

The wheel was built from the sdist entirely offline: cargo resolved all Rust
dependencies from the local registry cache (with `CARGO_NET_OFFLINE=true`),
maturin compiled the cdylib, and the resulting abi3 wheel was installed into
the venv. Platform: `cp310-abi3-macosx_11_0_arm64`.

### Step 4.3 — Import smoke test

```bash
python -c "import burner_redis; r = burner_redis.BurnerRedis(); print('import+instantiate OK:', type(r).__name__)"
```

Exit code: **0**. Last lines of `offline-import.log`:

```text
import+instantiate OK: BurnerRedis
```

The compiled extension loads; `BurnerRedis()` instantiates cleanly — the Rust
runtime, Tokio executor, and PyO3 async bridge are all wired correctly in the
offline-built wheel.

## Decision

PASS — pinned_version = 0.1.2; proceed to Plan 02 (license audit).

**Summary:**

- 0.1.2 sdist on PyPI ships `Cargo.lock` at tarball root (maturin default) — no
  `pyproject.toml` fix required, no 0.1.3 release cut.
- `CARGO_NET_OFFLINE=true pip install --no-index --no-build-isolation --no-deps
  burner_redis-0.1.2.tar.gz` succeeds end-to-end against a pre-populated cargo
  cache. conda-forge CI (which can reach crates.io) is strictly easier than
  this verification — the sdist is feedstock-ready.
- `import burner_redis; BurnerRedis()` works from the offline-built wheel.
- Contract fields for Plan 03:
  - `pinned_version: "0.1.2"`
  - `sha256: "189698190835809f73fdb5af9ead4962975181c7fc8297045a5d831c0d465add"`
  - `sdist_url: "https://files.pythonhosted.org/packages/a4/30/8b219fc8863c652ef294d9a6075752cf14eade2f050e956410873f6f0270/burner_redis-0.1.2.tar.gz"`
