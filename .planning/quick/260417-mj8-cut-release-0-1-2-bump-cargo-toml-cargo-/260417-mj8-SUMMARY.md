# Quick Task 260417-mj8 — Summary

**Description:** Cut release 0.1.2: bump Cargo.toml / Cargo.lock, generate release notes, propose a release name.
**Date:** 2026-04-17
**Status:** Complete (files staged for user review — nothing published)

## What changed

| File | Change |
|------|--------|
| `Cargo.toml` | `version = "0.1.1"` → `version = "0.1.2"` |
| `Cargo.lock` | `burner-redis` `[[package]]` entry `version = "0.1.1"` → `"0.1.2"` (via `cargo update -p burner-redis`, no other package versions moved) |
| `.planning/quick/260417-mj8-*/RELEASE_NOTES.md` | New file — proposed release name(s) + noir-voiced release body |

`pyproject.toml` intentionally untouched — maturin derives the wheel version from `Cargo.toml` (`dynamic = ["version"]`).

## Proposed release names

- **Primary:** `0.1.2 — No loose threads`
- Alt A: `0.1.2 — Lock the back door`
- Alt B: `0.1.2 — Don't leave the loop running`

The release notes body is agnostic to which title the user picks — pick one, strip the proposed-names header, paste the rest into the GitHub release description.

## Release body coverage

- Opening paragraph continuing the 0.1.0 → 0.1.1 → 0.1.2 arc, framing Windows as the hold-out jurisdiction.
- **Closing the exits** — `PubSub.aclose()` three-pronged stop, `BurnerRedis.aclose()/close()` waking blocking `xread`/`xreadgroup` + stopping pubsub listeners, `__aenter__`/`__aexit__` parity with `redis.asyncio.Redis`, pubsub delivery decoupled from the asyncio loop.
- **Paper trail** — new docket Windows CI workflow (Py 3.10–3.14), release-workflow `verify-version` tag↔manifest guard, Rust unit tests for `Store` shutdown semantics.
- **Upgrading** — one-liner pip command, no breaking changes.
- Closes with the compare URL `v0.1.1...v0.1.2`.
- No "Test Plan" section, no emojis, no references to the internal `docs(*)` commits.

## Commits

- `b9e5b9d chore(quick-260417-mj8-01): bump version to 0.1.2` (Cargo.toml + Cargo.lock)

`RELEASE_NOTES.md` and this `SUMMARY.md` are untracked in `.planning/` — they will be committed by the orchestrator's docs commit step, not by the executor.

## Remaining manual steps (user)

- [ ] Review `Cargo.toml`, `Cargo.lock`, and `RELEASE_NOTES.md`.
- [ ] Pick a release title from the proposed-names header.
- [ ] `git tag -a v0.1.2 -m "0.1.2 — <chosen name>"`
- [ ] `git push origin main v0.1.2` (release workflow's `verify-version` job will gate on the tag ↔ manifest match).
- [ ] Once CI + `pydocket compatibility` are green on the tag, `gh release create v0.1.2 --title "0.1.2 — <chosen name>" --notes-file .planning/quick/260417-mj8-*/RELEASE_NOTES.md` (strip the proposed-names header before submitting, or paste the body section only).

## Guardrails honored

- No tags created (`git tag --list 'v0.1.2'` is empty).
- No branches pushed.
- No GitHub releases opened.
- Diff reviewable in one sitting.
