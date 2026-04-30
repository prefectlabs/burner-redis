# T01: Bootstrap the burner-redis project: Rust crate with PyO3 bindings, maturin build system, in-memory store engine, and a working Python import path.

**Slice:** S01 — **Milestone:** M001

## Legacy Summary

---
phase: 01-foundation-and-string-commands
plan: 01
subsystem: foundation
tags: [pyo3, maturin, tokio, parking_lot, bytes, rust, python-bindings]

# Dependency graph
requires: []
provides:
  - "Rust crate with PyO3 bindings compiling and importable from Python"
  - "In-memory Store engine with get/set/delete/exists and passive expiration"
  - "BurnerRedis pyclass instantiable from Python with Arc<Store>"
  - "Tokio current-thread runtime initialized for async bridge"
  - "maturin build system producing mixed python/rust wheels"
affects: [01-02-string-commands, 02-hash-commands, 03-set-commands, 04-sorted-set-commands]

# Tech tracking
tech-stack:
  added: [pyo3 0.28.3, pyo3-async-runtimes 0.28.0, tokio 1.51, parking_lot 0.12.5, bytes 1.11, thiserror 2.0, maturin 1.13]
  patterns: [mixed python/rust project layout, passive expiration on read, RwLock-based keyspace, Arc<Store> shared ownership]

key-files:
  created: [Cargo.toml, pyproject.toml, python/burner_redis/__init__.py, src/lib.rs, src/store.rs, src/commands/mod.rs, src/commands/strings.rs, .gitignore]
  modified: []

key-decisions:
  - "Used tokio::runtime::Builder variable pattern to satisfy pyo3_async_runtimes::tokio::init owned Builder requirement"
  - "Added python-source = python to pyproject.toml for mixed python/rust project recognition by maturin"
  - "Created .gitignore with Rust target, Python venv, and native extension patterns"

patterns-established:
  - "Mixed python/rust layout: Rust src/ + python/burner_redis/ with maturin python-source"
  - "Store engine pattern: RwLock<HashMap<Bytes, ValueEntry>> with passive expiration on read"
  - "BurnerRedis pyclass holds Arc<Store> for shared ownership across async calls"
  - "Tokio current-thread runtime (not multi-thread) to respect Python GIL"

requirements-completed: [FOUND-01]

# Metrics
duration: 5min
completed: 2026-04-10
---

# Phase 01 Plan 01: Project Bootstrap Summary

**Rust crate with PyO3 bindings, maturin build system, in-memory Store engine with passive expiration, and importable BurnerRedis Python class**

## Performance

- **Duration:** 5 min
- **Started:** 2026-04-10T19:25:45Z
- **Completed:** 2026-04-10T19:30:42Z
- **Tasks:** 2
- **Files modified:** 8

## Accomplishments
- Bootstrapped complete Rust-to-Python bridge: `from burner_redis import BurnerRedis` works end-to-end
- Implemented Store engine with get/set/delete/exists methods, NX/XX conditional flags, and passive TTL expiration
- All 7 Rust unit tests pass covering core store operations, conditional writes, and expiration
- Configured maturin for mixed python/rust project with abi3-py39 stable ABI wheels

## Task Commits

Each task was committed atomically:

1. **Task 1: Create project scaffold** - `2694a65` (feat)
2. **Task 2: Implement Store engine, BurnerRedis pyclass, and pymodule** - `56733e7` (feat)

## Files Created/Modified
- `Cargo.toml` - Rust crate config with all Phase 1 dependencies (PyO3, Tokio, parking_lot, bytes, thiserror)
- `pyproject.toml` - Python package config with maturin backend, pytest, and python-source setting
- `python/burner_redis/__init__.py` - Python package re-export of BurnerRedis from native module
- `src/lib.rs` - PyO3 module entry point with BurnerRedis pyclass and Tokio current-thread init
- `src/store.rs` - In-memory key-value store with passive expiration and 7 unit tests
- `src/commands/mod.rs` - Command module declarations (strings submodule)
- `src/commands/strings.rs` - Placeholder for string command implementations (Plan 02)
- `.gitignore` - Ignore patterns for Rust target, Python venv, native extensions, IDE files
- `Cargo.lock` - Dependency lock file (35 packages)

## Decisions Made
- Used `let mut builder = Builder::new_current_thread(); builder.enable_all();` pattern because `pyo3_async_runtimes::tokio::init()` requires an owned `Builder`, but `enable_all()` returns `&mut Builder`
- Added `python-source = "python"` to `[tool.maturin]` so maturin recognizes the mixed python/rust layout and places the native `.so` inside `burner_redis/` package (not at top-level `_burner_redis/`)
- Created `.gitignore` to exclude build artifacts, virtual environment, and native extensions from version control

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed Builder type mismatch in Tokio init**
- **Found during:** Task 2 (cargo test compilation)
- **Issue:** Plan's code `pyo3_async_runtimes::tokio::init(Builder::new_current_thread().enable_all())` fails because `enable_all()` returns `&mut Builder` but `init()` requires owned `Builder`
- **Fix:** Changed to variable pattern: create mutable builder, call `enable_all()`, pass owned builder to `init()`
- **Files modified:** src/lib.rs
- **Verification:** cargo test passes, maturin develop succeeds
- **Committed in:** 56733e7 (Task 2 commit)

**2. [Rule 3 - Blocking] Added python-source to pyproject.toml for correct module installation**
- **Found during:** Task 2 (maturin develop + Python import test)
- **Issue:** Without `python-source = "python"`, maturin installs native module as `_burner_redis/` package at top-level, making `from burner_redis import BurnerRedis` fail with ModuleNotFoundError
- **Fix:** Added `python-source = "python"` to `[tool.maturin]` section
- **Files modified:** pyproject.toml
- **Verification:** `from burner_redis import BurnerRedis; BurnerRedis()` succeeds
- **Committed in:** 56733e7 (Task 2 commit)

**3. [Rule 2 - Missing Critical] Created .gitignore for build artifacts**
- **Found during:** Task 2 (post-build git status check)
- **Issue:** Compiled native extensions (.so), target/ directory, and .venv/ would be committed without .gitignore
- **Fix:** Created .gitignore with patterns for Rust, Python, and IDE artifacts
- **Files modified:** .gitignore (new)
- **Verification:** `git status` no longer shows .so files or target/
- **Committed in:** 56733e7 (Task 2 commit)

---

**Total deviations:** 3 auto-fixed (1 bug, 1 blocking, 1 missing critical)
**Impact on plan:** All auto-fixes necessary for correctness and clean repo. No scope creep.

## Issues Encountered
None beyond the auto-fixed deviations above.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Rust crate compiles and all tests pass -- ready for string command implementations (Plan 02)
- BurnerRedis pyclass is importable from Python -- ready to add async methods
- Store engine has get/set/delete/exists -- Plan 02 will wire these to Python via pyo3-async-runtimes
- Virtual environment with maturin, pytest, pytest-asyncio is ready for integration testing

## Self-Check: PASSED

All 9 files verified present. Both task commits (2694a65, 56733e7) verified in git log.

---
*Phase: 01-foundation-and-string-commands*
*Completed: 2026-04-10*
