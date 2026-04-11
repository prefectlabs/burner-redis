use bytes::Bytes;
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict};
use std::collections::HashSet as StdHashSet;
use std::sync::Arc;
use std::time::Duration;

mod store;
mod commands;

use commands::strings::{extract_bytes, extract_expiry};
use commands::sorted_sets::parse_score_bound;
use commands::streams::{format_stream_id, parse_stream_id, extract_stream_fields, StreamId};
use store::{Store, StoreError};

/// Convert a StoreError into a Python exception with the Redis-compatible error message.
fn store_err_to_py(e: StoreError) -> PyErr {
    match e {
        StoreError::WrongType => {
            pyo3::exceptions::PyException::new_err(e.to_string())
        }
        StoreError::NoGroup(_, _) => {
            pyo3::exceptions::PyException::new_err(e.to_string())
        }
        StoreError::BusyGroup => {
            pyo3::exceptions::PyException::new_err(e.to_string())
        }
        StoreError::KeyNotFound => {
            pyo3::exceptions::PyException::new_err(e.to_string())
        }
    }
}

#[pyclass]
pub struct BurnerRedis {
    store: Arc<Store>,
}

#[pymethods]
impl BurnerRedis {
    #[new]
    fn new() -> Self {
        let store = Arc::new(Store::new());

        // Spawn background sweep task for active expiration (EXP-03).
        // Uses Weak<Store> so the task stops when all BurnerRedis instances are dropped.
        let weak_store = Arc::downgrade(&store);
        pyo3_async_runtimes::tokio::get_runtime().spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(100));
            loop {
                interval.tick().await;
                match weak_store.upgrade() {
                    Some(store) => {
                        store.sweep_expired();
                    }
                    None => break, // Store dropped, stop sweeping
                }
            }
        });

        BurnerRedis { store }
    }

    /// SET command matching redis.asyncio.Redis.set() signature.
    /// Returns True on success, None when NX/XX condition fails.
    #[pyo3(signature = (name, value, ex=None, px=None, nx=false, xx=false))]
    fn set<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
        value: &Bound<'py, PyAny>,
        ex: Option<&Bound<'py, PyAny>>,
        px: Option<&Bound<'py, PyAny>>,
        nx: bool,
        xx: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        let key = extract_bytes(name)?;
        let val = extract_bytes(value)?;

        // Determine TTL: px takes precedence over ex (matches Redis behavior)
        let ttl = if let Some(px_val) = px {
            Some(extract_expiry(px_val, true)?)
        } else if let Some(ex_val) = ex {
            Some(extract_expiry(ex_val, false)?)
        } else {
            None
        };

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let success = store.set(key, val, ttl, nx, xx);
            if success {
                Ok(Some(true)) // Python: True
            } else {
                Ok(None) // Python: None (NX/XX condition failed)
            }
        })
    }

    /// GET command matching redis.asyncio.Redis.get() signature.
    /// Returns bytes or None.
    fn get<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        let key = extract_bytes(name)?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            Ok(store.get(&key).map(|b| b.to_vec()))
            // Option<Vec<u8>> -> Python bytes or None
        })
    }

    /// DELETE command matching redis.asyncio.Redis.delete() signature.
    /// Accepts variadic keys, returns count of deleted keys.
    #[pyo3(signature = (*names))]
    fn delete<'py>(
        &self,
        py: Python<'py>,
        names: &Bound<'py, pyo3::types::PyTuple>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        let keys: Vec<Bytes> = names
            .iter()
            .map(|obj| extract_bytes(&obj))
            .collect::<PyResult<Vec<_>>>()?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            Ok(store.delete(&keys))
        })
    }

    /// EXISTS command matching redis.asyncio.Redis.exists() signature.
    /// Accepts variadic keys, returns count of existing keys.
    #[pyo3(signature = (*names))]
    fn exists<'py>(
        &self,
        py: Python<'py>,
        names: &Bound<'py, pyo3::types::PyTuple>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        let keys: Vec<Bytes> = names
            .iter()
            .map(|obj| extract_bytes(&obj))
            .collect::<PyResult<Vec<_>>>()?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            Ok(store.exists(&keys))
        })
    }

    // ── Hash Commands ────────────────────────────────────────────────

    /// HSET command matching redis.asyncio.Redis.hset() signature.
    /// Sets field-value pairs in a hash. Returns count of NEW fields added.
    #[pyo3(signature = (name, key=None, value=None, mapping=None))]
    fn hset<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
        key: Option<&Bound<'py, PyAny>>,
        value: Option<&Bound<'py, PyAny>>,
        mapping: Option<&Bound<'py, PyDict>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        let name_bytes = extract_bytes(name)?;

        let mut fields: Vec<(Bytes, Bytes)> = Vec::new();

        // Single key/value pair
        if let (Some(k), Some(v)) = (key, value) {
            fields.push((extract_bytes(k)?, extract_bytes(v)?));
        }

        // Mapping (dict of field -> value)
        if let Some(dict) = mapping {
            for (k, v) in dict.iter() {
                fields.push((extract_bytes(&k)?, extract_bytes(&v)?));
            }
        }

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            store.hset(name_bytes, fields).map_err(store_err_to_py)
        })
    }

    /// HGET command matching redis.asyncio.Redis.hget() signature.
    /// Returns bytes value or None.
    fn hget<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
        key: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        let name_bytes = extract_bytes(name)?;
        let field_bytes = extract_bytes(key)?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let result = store.hget(&name_bytes, &field_bytes).map_err(store_err_to_py)?;
            Ok(result.map(|b| b.to_vec()))
        })
    }

    /// HDEL command matching redis.asyncio.Redis.hdel() signature.
    /// Accepts variadic keys, returns count of fields deleted.
    #[pyo3(signature = (name, *keys))]
    fn hdel<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
        keys: &Bound<'py, pyo3::types::PyTuple>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        let name_bytes = extract_bytes(name)?;
        let fields: Vec<Bytes> = keys
            .iter()
            .map(|obj| extract_bytes(&obj))
            .collect::<PyResult<Vec<_>>>()?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            store.hdel(&name_bytes, &fields).map_err(store_err_to_py)
        })
    }

    /// HVALS command matching redis.asyncio.Redis.hvals() signature.
    /// Returns list of all values in the hash as bytes.
    fn hvals<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        let name_bytes = extract_bytes(name)?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let vals = store.hvals(&name_bytes).map_err(store_err_to_py)?;
            Ok(vals.into_iter().map(|b| b.to_vec()).collect::<Vec<Vec<u8>>>())
        })
    }

    // ── Set Commands ─────────────────────────────────────────────────

    /// SADD command matching redis.asyncio.Redis.sadd() signature.
    /// Accepts variadic members, returns count of NEW members added.
    #[pyo3(signature = (name, *values))]
    fn sadd<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
        values: &Bound<'py, pyo3::types::PyTuple>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        let name_bytes = extract_bytes(name)?;
        let members: Vec<Bytes> = values
            .iter()
            .map(|obj| extract_bytes(&obj))
            .collect::<PyResult<Vec<_>>>()?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            store.sadd(name_bytes, members).map_err(store_err_to_py)
        })
    }

    /// SMEMBERS command matching redis.asyncio.Redis.smembers() signature.
    /// Returns a Python set of all members as bytes.
    fn smembers<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        let name_bytes = extract_bytes(name)?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let members = store.smembers(&name_bytes).map_err(store_err_to_py)?;
            let set: StdHashSet<Vec<u8>> = members.into_iter().map(|b| b.to_vec()).collect();
            Ok(set)
        })
    }

    /// SISMEMBER command matching redis.asyncio.Redis.sismember() signature.
    /// Returns bool: True if the value is a member of the set.
    fn sismember<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
        value: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        let name_bytes = extract_bytes(name)?;
        let member_bytes = extract_bytes(value)?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            store.sismember(&name_bytes, &member_bytes).map_err(store_err_to_py)
        })
    }

    /// SREM command matching redis.asyncio.Redis.srem() signature.
    /// Accepts variadic members, returns count of members removed.
    #[pyo3(signature = (name, *values))]
    fn srem<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
        values: &Bound<'py, pyo3::types::PyTuple>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        let name_bytes = extract_bytes(name)?;
        let members: Vec<Bytes> = values
            .iter()
            .map(|obj| extract_bytes(&obj))
            .collect::<PyResult<Vec<_>>>()?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            store.srem(&name_bytes, &members).map_err(store_err_to_py)
        })
    }

    // ── Sorted Set Commands ──────────────────────────────────────────

    /// ZADD command matching redis.asyncio.Redis.zadd() signature.
    /// Adds members with scores to a sorted set. Returns count of new members
    /// (or changed members if ch=True).
    #[pyo3(signature = (name, mapping, nx=false, xx=false, gt=false, lt=false, ch=false))]
    fn zadd<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
        mapping: &Bound<'py, PyDict>,
        nx: bool,
        xx: bool,
        gt: bool,
        lt: bool,
        ch: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        let name_bytes = extract_bytes(name)?;

        // Extract mapping: {member: score} dict -> Vec<(f64, Bytes)>
        let mut members: Vec<(f64, Bytes)> = Vec::new();
        for (k, v) in mapping.iter() {
            let member = extract_bytes(&k)?;
            let score: f64 = v.extract::<f64>()?;
            members.push((score, member));
        }

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            store
                .zadd(name_bytes, members, nx, xx, gt, lt, ch)
                .map_err(store_err_to_py)
        })
    }

    /// ZREM command matching redis.asyncio.Redis.zrem() signature.
    /// Removes members from a sorted set. Returns count removed.
    #[pyo3(signature = (name, *values))]
    fn zrem<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
        values: &Bound<'py, pyo3::types::PyTuple>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        let name_bytes = extract_bytes(name)?;
        let members: Vec<Bytes> = values
            .iter()
            .map(|obj| extract_bytes(&obj))
            .collect::<PyResult<Vec<_>>>()?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            store.zrem(&name_bytes, &members).map_err(store_err_to_py)
        })
    }

    /// ZRANGE command matching redis.asyncio.Redis.zrange() signature.
    /// Returns members by index range. Without withscores: list[bytes].
    /// With withscores=True: list[tuple[bytes, float]].
    #[pyo3(signature = (name, start, end, withscores=false))]
    fn zrange<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
        start: i64,
        end: i64,
        withscores: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        let name_bytes = extract_bytes(name)?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let results = store
                .zrange(&name_bytes, start, end, withscores)
                .map_err(store_err_to_py)?;
            Python::try_attach(|py| -> PyResult<Py<PyAny>> {
                if withscores {
                    let list: Vec<(Vec<u8>, f64)> = results
                        .into_iter()
                        .map(|(m, s)| (m.to_vec(), s.unwrap_or(0.0)))
                        .collect();
                    Ok(list.into_pyobject(py)?.into_any().unbind())
                } else {
                    let list: Vec<Vec<u8>> =
                        results.into_iter().map(|(m, _)| m.to_vec()).collect();
                    Ok(list.into_pyobject(py)?.into_any().unbind())
                }
            })
            .ok_or_else(|| {
                PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                    "failed to attach to Python interpreter",
                )
            })?
        })
    }

    /// ZRANGEBYSCORE command matching redis.asyncio.Redis.zrangebyscore() signature.
    /// Returns members with scores in [min, max] range.
    /// Accepts float or string ("-inf", "+inf") for min/max.
    #[pyo3(signature = (name, min, max, withscores=false))]
    fn zrangebyscore<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
        min: &Bound<'py, PyAny>,
        max: &Bound<'py, PyAny>,
        withscores: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        let name_bytes = extract_bytes(name)?;
        let min_f64 = parse_score_bound(min)?;
        let max_f64 = parse_score_bound(max)?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let results = store
                .zrangebyscore(&name_bytes, min_f64, max_f64, withscores)
                .map_err(store_err_to_py)?;
            Python::try_attach(|py| -> PyResult<Py<PyAny>> {
                if withscores {
                    let list: Vec<(Vec<u8>, f64)> = results
                        .into_iter()
                        .map(|(m, s)| (m.to_vec(), s.unwrap_or(0.0)))
                        .collect();
                    Ok(list.into_pyobject(py)?.into_any().unbind())
                } else {
                    let list: Vec<Vec<u8>> =
                        results.into_iter().map(|(m, _)| m.to_vec()).collect();
                    Ok(list.into_pyobject(py)?.into_any().unbind())
                }
            })
            .ok_or_else(|| {
                PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                    "failed to attach to Python interpreter",
                )
            })?
        })
    }

    /// ZRANGESTORE command matching redis.asyncio.Redis.zrangestore() signature.
    /// Stores score-range result from source into destination key. Returns count stored.
    #[pyo3(signature = (dest, name, start, end))]
    fn zrangestore<'py>(
        &self,
        py: Python<'py>,
        dest: &Bound<'py, PyAny>,
        name: &Bound<'py, PyAny>,
        start: &Bound<'py, PyAny>,
        end: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        let dst_bytes = extract_bytes(dest)?;
        let src_bytes = extract_bytes(name)?;
        let min_f64 = parse_score_bound(start)?;
        let max_f64 = parse_score_bound(end)?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            store
                .zrangestore(dst_bytes, &src_bytes, min_f64, max_f64)
                .map_err(store_err_to_py)
        })
    }

    /// ZREMRANGEBYSCORE command matching redis.asyncio.Redis.zremrangebyscore() signature.
    /// Removes all members with scores in [min, max] range. Returns count removed.
    #[pyo3(signature = (name, min, max))]
    fn zremrangebyscore<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
        min: &Bound<'py, PyAny>,
        max: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        let name_bytes = extract_bytes(name)?;
        let min_f64 = parse_score_bound(min)?;
        let max_f64 = parse_score_bound(max)?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            store
                .zremrangebyscore(&name_bytes, min_f64, max_f64)
                .map_err(store_err_to_py)
        })
    }

    // ── Stream Commands ──────────────────────────────────────────────

    /// XADD command matching redis.asyncio.Redis.xadd() signature.
    /// Adds an entry to a stream. Returns the entry ID as bytes (e.g., b"1234567890123-0").
    #[pyo3(signature = (name, fields, id="*"))]
    fn xadd<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
        fields: &Bound<'py, PyDict>,
        id: &str,
    ) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        let key = extract_bytes(name)?;
        let field_map = extract_stream_fields(fields)?;

        // Parse id: "*" means auto-generate, otherwise parse explicit ID
        let id_opt: Option<StreamId> = if id == "*" {
            None
        } else {
            Some(parse_stream_id(id).ok_or_else(|| {
                pyo3::exceptions::PyValueError::new_err(format!("Invalid stream ID format: {}", id))
            })?)
        };

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let stream_id = store.xadd(key, field_map, id_opt).map_err(store_err_to_py)?;
            let id_str = format_stream_id(stream_id);
            Ok(id_str.into_bytes())
        })
    }

    /// XREAD command matching redis.asyncio.Redis.xread() signature.
    /// Reads entries from one or more streams after given IDs.
    /// Returns list of [stream_name, [(id, {field: value}), ...]] or None if empty.
    #[pyo3(signature = (streams, count=None))]
    fn xread<'py>(
        &self,
        py: Python<'py>,
        streams: &Bound<'py, PyDict>,
        count: Option<usize>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();

        // Extract stream names and IDs from the dict
        let mut keys: Vec<Bytes> = Vec::new();
        let mut ids: Vec<StreamId> = Vec::new();

        for (k, v) in streams.iter() {
            let key = extract_bytes(&k)?;
            let id_str: String = v.extract::<String>().or_else(|_| {
                v.extract::<Vec<u8>>()
                    .map(|b| String::from_utf8_lossy(&b).into_owned())
            })?;

            let stream_id = if id_str == "0" || id_str == "0-0" {
                (0u64, 0u64)
            } else {
                parse_stream_id(&id_str).ok_or_else(|| {
                    pyo3::exceptions::PyValueError::new_err(format!(
                        "Invalid stream ID: {}",
                        id_str
                    ))
                })?
            };

            keys.push(key);
            ids.push(stream_id);
        }

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let results = store.xread(&keys, &ids, count).map_err(store_err_to_py)?;

            if results.is_empty() {
                return Ok(None::<pyo3::Py<pyo3::PyAny>>);
            }

            // Build the nested Python structure:
            // [[stream_name_bytes, [(id_bytes, {field_bytes: value_bytes}), ...]], ...]
            Python::try_attach(|py| -> PyResult<Option<pyo3::Py<pyo3::PyAny>>> {
                let outer = pyo3::types::PyList::empty(py);
                for (stream_name, entries) in &results {
                    let entry_list = pyo3::types::PyList::empty(py);
                    for (id, fields) in entries {
                        let id_bytes = format_stream_id(*id).into_bytes();
                        let field_dict = pyo3::types::PyDict::new(py);
                        for (fk, fv) in fields {
                            field_dict.set_item(
                                pyo3::types::PyBytes::new(py, fk.as_ref()),
                                pyo3::types::PyBytes::new(py, fv.as_ref()),
                            )?;
                        }
                        let tuple = pyo3::types::PyTuple::new(
                            py,
                            &[
                                pyo3::types::PyBytes::new(py, &id_bytes).into_any(),
                                field_dict.into_any(),
                            ],
                        )?;
                        entry_list.append(tuple)?;
                    }
                    let stream_pair = pyo3::types::PyList::new(
                        py,
                        &[
                            pyo3::types::PyBytes::new(py, stream_name.as_ref()).into_any(),
                            entry_list.into_any(),
                        ],
                    )?;
                    outer.append(stream_pair)?;
                }
                Ok(Some(outer.into_any().unbind()))
            })
            .ok_or_else(|| {
                PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                    "failed to attach to Python interpreter",
                )
            })?
        })
    }

    /// XLEN command matching redis.asyncio.Redis.xlen() signature.
    /// Returns the number of entries in a stream.
    fn xlen<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        let key = extract_bytes(name)?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let len = store.xlen(&key).map_err(store_err_to_py)?;
            Ok(len as i64)
        })
    }

    /// XTRIM command matching redis.asyncio.Redis.xtrim() signature.
    /// Trims a stream by maxlen or minid. Returns count of entries removed.
    #[pyo3(signature = (name, maxlen=None, minid=None))]
    fn xtrim<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
        maxlen: Option<usize>,
        minid: Option<&str>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        let key = extract_bytes(name)?;

        let minid_parsed: Option<StreamId> = match minid {
            Some(s) => Some(parse_stream_id(s).ok_or_else(|| {
                pyo3::exceptions::PyValueError::new_err(format!(
                    "Invalid stream ID for minid: {}",
                    s
                ))
            })?),
            None => None,
        };

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let trimmed = store
                .xtrim(&key, maxlen, minid_parsed)
                .map_err(store_err_to_py)?;
            Ok(trimmed as i64)
        })
    }
}

#[pymodule]
fn _burner_redis(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Initialize Tokio multi-thread runtime for future_into_py compatibility.
    // future_into_py spawns tasks on the Tokio runtime; a current-thread runtime
    // has no background thread to drive spawned futures, causing deadlocks.
    // The GIL is released before spawning, so multi-thread is safe here.
    let mut builder = tokio::runtime::Builder::new_multi_thread();
    builder.enable_all();
    pyo3_async_runtimes::tokio::init(builder);

    m.add_class::<BurnerRedis>()?;
    Ok(())
}
