---
status: complete
phase: 15-close-v0.1.6-wiring-and-coverage-gaps
source: [15-01-SUMMARY.md]
started: 2026-04-27T16:25:00Z
updated: 2026-04-27T16:30:00Z
---

## Current Test

[testing complete]

## Tests

### 1. EVALSHA on unknown SHA raises NoScriptError
expected: |
  When you call `await client.evalsha("deadbeef" * 5, 0)` with a SHA that was never loaded,
  the call raises `burner_redis.NoScriptError` (which is also a `redis.exceptions.NoScriptError`
  subclass when `redis` is installed). Code that does `except NoScriptError:` from either
  `burner_redis` or `redis.exceptions` catches the error. The error message starts with
  "NOSCRIPT".
result: pass
verified_by: |
  Live script run against installed burner_redis 0.1.5:
  - `except NoScriptError` (from burner_redis) caught the exception with message
    `'NOSCRIPT No matching script. Use EVAL.'`
  - `except redis.exceptions.NoScriptError` ALSO caught the exception (subclass relationship)
  - `issubclass(burner_redis.NoScriptError, redis.exceptions.NoScriptError)` is True
  - MRO: `(burner_redis.NoScriptError, redis.exceptions.NoScriptError, ResponseError, RedisError, Exception, BaseException, object)`

### 2. Pipeline.zrangestore() executes without "Unknown pipeline command"
expected: |
  Building a pipeline with `pipe.zrangestore("dest", "src", 0, 10)` and awaiting
  `pipe.execute()` returns the integer count of stored members (no `Unknown pipeline command`
  error). The resulting destination key contains the stored sorted-set members in score order
  when read back via `await client.zrange("dest", 0, -1)`.
result: pass
verified_by: |
  Live script: seeded `src` with {a:1.0, b:2.0, c:3.0}, ran
  `pipe.zrangestore("dest", "src", 0, 10)` + `pipe.zrange("dest", 0, -1)` → results = `[3, [b"a", b"b", b"c"]]`.
  Standalone `client.zrangestore("dest2", "src", 0, 10)` returned 3 with members `[b"a", b"b", b"c"]` —
  pipeline matches standalone exactly.

### 3. Pipeline.zcount() executes without "Unknown pipeline command"
expected: |
  Building a pipeline with `pipe.zcount("zset", 2.0, 3.0)` and `pipe.zcount("zset", "-inf", "+inf")`
  and awaiting `pipe.execute()` returns the correct integer counts (no `Unknown pipeline command`
  error). The pipeline result matches what the standalone `await client.zcount(...)` pymethod
  returns for the same inputs.
result: pass
verified_by: |
  Live script: seeded `zset` with {a:1.0, b:2.0, c:3.0, d:4.0}, ran
  `pipe.zcount("zset", 2.0, 3.0)` + `pipe.zcount("zset", "-inf", "+inf")` → results = `[2, 4]`.
  Standalone calls returned bounded=2 (b, c in [2.0, 3.0]) and unbounded=4 — pipeline matches.

### 4. List data persists across BurnerRedis instances
expected: |
  Creating `client1 = BurnerRedis(persistence_path=tmp_path)`, calling
  `await client1.rpush("list1", "a", "b", "c")` then `await client1.save()`, then constructing
  `client2 = BurnerRedis(persistence_path=tmp_path)` lets `client2` read the saved list:
  `await client2.lrange("list1", 0, -1)` returns `[b"a", b"b", b"c"]` (order preserved) and
  `await client2.llen("list1")` returns `3`.
result: pass
verified_by: |
  Live script:
  - client1.rpush + save() wrote a 25-byte persistence file.
  - client2 with same persistence_path: `lrange("list1", 0, -1)` = `[b"a", b"b", b"c"]`, `llen` = 3.
  - Bonus: binary-safe round-trip on bytes containing `\x00\x01\x02` and `\xff\xfe` also succeeded.

## Summary

total: 4
passed: 4
issues: 0
pending: 0
skipped: 0

## Gaps

[none — all tests passed]
