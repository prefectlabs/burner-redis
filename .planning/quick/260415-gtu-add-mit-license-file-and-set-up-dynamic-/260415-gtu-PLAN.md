---
quick_id: 260415-gtu
description: Add MIT LICENSE file and set up dynamic versioning
date: 2026-04-15
---

# Quick Task: Add MIT LICENSE file and set up dynamic versioning

## Task 1: Add LICENSE and configure dynamic version

**Files:** LICENSE (new), pyproject.toml
**Action:**
1. Create MIT LICENSE file in repo root
2. Replace static `version = "0.1.0"` with `dynamic = ["version"]` in pyproject.toml
3. Cargo.toml remains the single source of truth for version (maturin reads it automatically)

**Verify:** `uv run maturin develop` succeeds and reads version from Cargo.toml
**Done:** LICENSE exists, pyproject.toml uses dynamic version, build works
