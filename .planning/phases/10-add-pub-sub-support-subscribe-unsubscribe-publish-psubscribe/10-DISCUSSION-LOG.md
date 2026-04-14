# Phase 10: Add PUB/SUB Support - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-04-13
**Phase:** 10-add-pub-sub-support-subscribe-unsubscribe-publish-psubscribe
**Areas discussed:** Motivation & scope, Python API design, Message delivery, Cross-cutting

---

## Motivation & Scope

| Option | Description | Selected |
|--------|-------------|----------|
| Prefect now needs it | New Prefect feature or subsystem that uses pub/sub channels | |
| Broader compatibility | Making burner-redis useful beyond Prefect — other apps expect pub/sub | |
| Completeness | Rounding out the Redis feature set for drop-in replacement confidence | |
| Other | Broader compatibility with the pydocket package which Prefect uses | ✓ |

**User's choice:** Broader compatibility with the `pydocket` package which Prefect uses
**Notes:** Pub/Sub was originally out of scope but pydocket dependency drives the need

### Commands Scope

| Option | Description | Selected |
|--------|-------------|----------|
| SUBSCRIBE/UNSUBSCRIBE | Subscribe to exact channel names, unsubscribe from them | ✓ |
| PUBLISH | Publish a message to a channel | ✓ |
| PSUBSCRIBE/PUNSUBSCRIBE | Pattern-based subscription using glob-style patterns | ✓ |
| PUBSUB subcommands | Introspection: PUBSUB CHANNELS, PUBSUB NUMSUB, PUBSUB NUMPAT | ✓ |

**User's choice:** All commands are must-haves

### pydocket Patterns

| Option | Description | Selected |
|--------|-------------|----------|
| Yes, I can describe | I know the specific pub/sub patterns pydocket relies on | |
| Not sure, research it | Let the researcher agent investigate pydocket's pub/sub usage | ✓ |
| Just match redis-py | Don't worry about pydocket specifics — match redis.asyncio.PubSub behavior | |

**User's choice:** Not sure — researcher should investigate
**Notes:** None

---

## Python API Design

### API Entry Point

| Option | Description | Selected |
|--------|-------------|----------|
| Mirror redis-py | client.pubsub() returns PubSub object, monkey-patched like Pipeline/Lock | ✓ |
| Simplified API | Fewer methods — just subscribe/publish/listen | |
| You decide | Claude picks best approach | |

**User's choice:** Mirror redis-py

### Handler Callbacks

| Option | Description | Selected |
|--------|-------------|----------|
| Yes, match redis-py | subscribe('chan', handler=my_func) routes messages to callbacks | ✓ |
| No, just listen() | Consumers iterate with async for msg in listen() | |
| You decide | Claude decides based on pydocket needs | |

**User's choice:** Yes, match redis-py

### Implementation Layer

| Option | Description | Selected |
|--------|-------------|----------|
| Pure Python | PubSub class in Python, calls Rust Store methods. Consistent with Pipeline/Lock. | ✓ |
| Rust-backed | PubSub state in Rust with PyO3 bindings | |
| You decide | Claude picks based on patterns | |

**User's choice:** Pure Python

### run_in_thread() Support

| Option | Description | Selected |
|--------|-------------|----------|
| Yes, match redis-py | run_in_thread() spawns daemon thread for background message processing | ✓ |
| No, async only | Skip run_in_thread() — async-first library | |
| You decide | Claude decides based on pydocket research | |

**User's choice:** Yes, match redis-py

### ignore_subscribe_messages

| Option | Description | Selected |
|--------|-------------|----------|
| Yes | Match redis-py — filter subscription confirmations from listen() output | ✓ |
| No | Always include subscription confirmations | |
| You decide | Claude decides | |

**User's choice:** Yes

---

## Message Delivery

### Consumer Methods

| Option | Description | Selected |
|--------|-------------|----------|
| Both listen() and get_message() | Full redis-py compat — async generator and polling | ✓ |
| listen() only | Just the async generator pattern | |
| You decide | Claude decides based on pydocket needs | |

**User's choice:** Both

### Internal Fan-out Mechanism

| Option | Description | Selected |
|--------|-------------|----------|
| Tokio broadcast channel | Each subscriber gets a broadcast receiver. Natural async fit. | |
| Per-subscriber queue | Each subscriber has own message queue. More backpressure control. | |
| You decide | Claude picks best Rust-side mechanism | ✓ |

**User's choice:** You decide (Claude's Discretion)

---

## Cross-cutting

### Lua Integration

| Option | Description | Selected |
|--------|-------------|----------|
| Yes | PUBLISH available in Lua scripts via redis.call() | ✓ |
| No | Skip Lua integration for now | |
| You decide | Claude decides | |

**User's choice:** Yes

### Pipeline Integration

| Option | Description | Selected |
|--------|-------------|----------|
| Yes | PUBLISH queued in pipeline, executed with batch | ✓ |
| No | PUBLISH only standalone | |
| You decide | Claude decides | |

**User's choice:** Yes

### Persistence

| Option | Description | Selected |
|--------|-------------|----------|
| Fire-and-forget | No persistence. Matches Redis exactly. | ✓ |
| Persist subscriptions | Remember subscribed channels across restarts | |
| You decide | Claude decides | |

**User's choice:** Fire-and-forget

---

## Claude's Discretion

- Internal Rust fan-out mechanism for message dispatch (broadcast channel vs per-subscriber queue vs other)

## Deferred Ideas

None — discussion stayed within phase scope
