# burner-redis

An embedded, in-process Redis-compatible database written in Rust with Python bindings.

Provides a drop-in replacement for `redis.asyncio.Redis` that runs inside the host process with no external server needed. The primary use case is backing a self-hosted Prefect server without requiring a separate Redis deployment.

## Installation

```bash
pip install burner-redis
```

## Usage

```python
from burner_redis import BurnerRedis

db = BurnerRedis()

# Use like redis.asyncio.Redis
await db.set("key", "value")
value = await db.get("key")
```

## Features

- Drop-in compatible with `redis.asyncio.Redis` API surface used by Prefect
- String, Hash, Set, Sorted Set, and Stream commands
- Lua scripting (EVAL/EVALSHA)
- Pipeline support
- Key expiration (TTL)
- Optional persistence (flush to disk / reload on startup)
- No external Redis server required

## License

MIT
