# v0.1.2 — Proposed release names

> Header is for user review. Pick one title when publishing; use only the body below it as the GitHub release description.

- **Primary:** 0.1.2 — No loose threads
- Alt A: 0.1.2 — Lock the back door
- Alt B: 0.1.2 — Don't leave the loop running

---

`v0.1.0` struck the match. `v0.1.1` wiped prints. Everyone who'd been talking had stopped talking — except one jurisdiction. Windows was still keeping notes: pubsub listeners outliving the client, `call_soon_threadsafe` firing into an event loop that had already packed up for the night, blocking readers who never got the memo that the shop was closed. `v0.1.2` walks back to that exit, makes sure the door latches, and checks that nobody's still inside.

No new features. The shutdown path, which was supposed to go quietly, was the one thing still making noise.

### Closing the exits

The shutdown path had three separate tells. Each one looked harmless on its own. Together they were a signature.

- **`PubSub.aclose()` now actually stops the listener.** The background Tokio task got a three-pronged kill switch — an `AtomicBool stop_flag`, a oneshot `stop_tx`, and its own `JoinHandle` — and `aclose()` uses all three before returning. No more deliveries into an asyncio loop that's already turned off the lights.
- **`BurnerRedis.aclose()` / `close()` wakes everyone up on the way out.** The `Store` now carries a shutdown flag. Setting it wakes every blocking `xread` / `xreadgroup` waiter and stops every registered pubsub listener in the same breath. The blocking loops check `is_shutdown()` and return empty instead of sitting in the dark waiting for a message that's never going to arrive.
- **`async with BurnerRedis(...) as r:` works.** `__aenter__` and `__aexit__` were conspicuously absent — a tell, if you knew what you were looking for. Now they match `redis.asyncio.Redis`, and teardown happens through the same `aclose()` path that does the real work.
- **Pubsub delivery is decoupled from the asyncio loop.** The listener no longer assumes the loop it was born into is the loop it will die into. The same PR fixed this as a follow-up once the teardown race became visible.

### Paper trail

Nothing about this release should've required paperwork. But the v0.1.1 build almost shipped with a tag that didn't match the manifest, and pretending that wasn't a close call would be the sloppiest move of all.

- **docket runs against us on Windows now.** A new CI workflow builds burner-redis in-tree on `windows-latest` and runs docket's full test suite across Python 3.10–3.14. The canary doesn't get to die in a customer's CI first anymore.
- **Release workflow won't let the tag walk without the manifest.** A `verify-version` job at the top of the release pipeline compares the pushed tag against `Cargo.toml` and `Cargo.lock`, and gates every downstream job on the match. The v0.1.1 almost-mishap is on film; this is what keeps it from having a sequel.
- **Rust unit tests for `Store` shutdown.** `is_shutdown()` initial state, `shutdown()` flips it, calling it twice is a no-op, `notify_waiters` actually wakes blocked readers, and shutdown takes registered pubsub listeners with it. The boss reads the reports.

### Upgrading

```
pip install --upgrade burner-redis
```

No breaking changes. If you were already calling `close()` / `aclose()` on your client and pubsub, the only difference is that now they finish the job.

**Full Changelog**: https://github.com/prefectlabs/burner-redis/compare/v0.1.1...v0.1.2
