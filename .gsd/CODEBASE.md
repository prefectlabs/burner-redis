# Codebase Map

Generated: 2026-04-30T16:18:15Z | Files: 48 | Described: 0/48
<!-- gsd:codebase-meta {"generatedAt":"2026-04-30T16:18:15Z","fingerprint":"e8ffe052fe9d1924fa6b4f202a748e98000c9da9","fileCount":48,"truncated":false} -->

### (root)/
- `.gitignore`
- `Cargo.toml`
- `CLAUDE.md`
- `LICENSE`
- `pyproject.toml`
- `README.md`
- `THIRDPARTY.yml`

### .github/conda-smoke/
- `.github/conda-smoke/environment-win.yml`
- `.github/conda-smoke/recipe.yaml`
- `.github/conda-smoke/win64.yaml`

### .github/workflows/
- `.github/workflows/ci.yml`
- `.github/workflows/conda-smoke.yml`
- `.github/workflows/docket-windows.yml`
- `.github/workflows/pydocket-compat.yml`
- `.github/workflows/release.yml`

### python/burner_redis/
- `python/burner_redis/__init__.py`
- `python/burner_redis/lock.py`
- `python/burner_redis/pipeline.py`
- `python/burner_redis/pubsub.py`

### src/
- `src/lib.rs`
- `src/persistence.rs`
- `src/scripting.rs`
- `src/store.rs`

### src/commands/
- `src/commands/hashes.rs`
- `src/commands/lists.rs`
- `src/commands/mod.rs`
- `src/commands/pubsub.rs`
- `src/commands/sets.rs`
- `src/commands/sorted_sets.rs`
- `src/commands/streams.rs`
- `src/commands/strings.rs`

### tests/
- `tests/conftest.py`
- `tests/test_coercion.py`
- `tests/test_expiration.py`
- `tests/test_graceful_shutdown.py`
- `tests/test_hashes.py`
- `tests/test_lists.py`
- `tests/test_locking.py`
- `tests/test_persistence.py`
- `tests/test_pipeline.py`
- `tests/test_prefect_integration.py`
- `tests/test_pubsub.py`
- `tests/test_pydocket_compat.py`
- `tests/test_scripting.py`
- `tests/test_sets.py`
- `tests/test_sorted_sets.py`
- `tests/test_streams.py`
- `tests/test_strings.py`
