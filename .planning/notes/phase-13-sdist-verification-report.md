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

<filled in by Task 4>

## Decision

<filled in by Task 4>
