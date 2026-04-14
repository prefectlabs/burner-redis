use bytes::Bytes;
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyBytes, PyCFunction, PyDict, PyTuple};
use std::collections::HashSet as StdHashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;

mod store;
mod commands;
mod persistence;
mod scripting;

use commands::strings::{extract_bytes, extract_expiry};
use commands::sorted_sets::parse_score_bound;
use commands::streams::{format_stream_id, parse_stream_id, extract_stream_fields, StreamId};
use scripting::RedisValue;
use store::{Store, StoreError};

/// Convert a RedisValue (from Lua script execution) into a Python object.
/// Handles recursive conversion for nested arrays.
fn redis_value_to_py(py: Python<'_>, val: RedisValue) -> PyResult<Py<PyAny>> {
    match val {
        RedisValue::BulkString(b) => Ok(PyBytes::new(py, &b).into_any().unbind()),
        RedisValue::Integer(n) => Ok(n.into_pyobject(py)?.into_any().unbind()),
        RedisValue::Nil => Ok(py.None()),
        RedisValue::Status(s) => Ok(PyBytes::new(py, s.as_bytes()).into_any().unbind()),
        RedisValue::Error(msg) => Err(make_response_error(msg)),
        RedisValue::Array(items) => {
            let py_items: PyResult<Vec<Py<PyAny>>> = items
                .into_iter()
                .map(|item| redis_value_to_py(py, item))
                .collect();
            Ok(pyo3::types::PyList::new(py, &py_items?)?.into_any().unbind())
        }
    }
}

/// Create a Redis-compatible ResponseError.
/// Uses redis.exceptions.ResponseError if available (for pydocket/redis-py compatibility),
/// falls back to Python's Exception.
fn make_response_error(msg: String) -> PyErr {
    match Python::try_attach(|py| -> PyResult<PyErr> {
        if let Ok(redis_exc) = py.import("redis.exceptions") {
            if let Ok(response_error_type) = redis_exc.getattr("ResponseError") {
                if let Ok(exc_type) = response_error_type.downcast::<pyo3::types::PyType>() {
                    return Ok(PyErr::from_type(exc_type.clone(), msg.clone()));
                }
            }
        }
        Ok(pyo3::exceptions::PyException::new_err(msg.clone()))
    }) {
        Some(Ok(err)) => err,
        _ => pyo3::exceptions::PyException::new_err(msg),
    }
}

/// Convert a StoreError into a Python exception with the Redis-compatible error message.
fn store_err_to_py(e: StoreError) -> PyErr {
    make_response_error(e.to_string())
}

/// Format XREADGROUP results into a Python list structure.
/// Used by both blocking and non-blocking XREADGROUP paths.
fn format_xreadgroup_result(
    results: Vec<(Bytes, Vec<(commands::streams::StreamId, std::collections::HashMap<Bytes, Bytes>)>)>,
) -> PyResult<pyo3::Py<pyo3::PyAny>> {
    if results.is_empty() {
        return Python::try_attach(|py| -> PyResult<pyo3::Py<pyo3::PyAny>> {
            Ok(pyo3::types::PyList::empty(py).into_any().unbind())
        })
        .ok_or_else(|| {
            PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                "failed to attach to Python interpreter",
            )
        })?;
    }

    Python::try_attach(|py| -> PyResult<pyo3::Py<pyo3::PyAny>> {
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
        Ok(outer.into_any().unbind())
    })
    .ok_or_else(|| {
        PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
            "failed to attach to Python interpreter",
        )
    })?
}

#[pyclass]
pub struct BurnerRedis {
    store: Arc<Store>,
    persistence_path: Option<String>,
}

#[pymethods]
impl BurnerRedis {
    #[new]
    #[pyo3(signature = (persistence_path=None))]
    fn new(py: Python<'_>, persistence_path: Option<String>) -> PyResult<Self> {
        let store = Arc::new(Store::new());

        // If persistence_path is set and file exists, restore data
        if let Some(ref path) = persistence_path {
            match store.load_into(path) {
                Ok(true) => {
                    // Data restored successfully
                }
                Ok(false) => {
                    // File doesn't exist -- start empty (normal for first run)
                }
                Err(e) => {
                    // Corrupt file -- warn and start empty
                    eprintln!("burner-redis: failed to load persistence file '{}': {}. Starting with empty store.", path, e);
                }
            }

            // Register atexit handler to save on graceful shutdown (T-08-06: catch exceptions silently)
            let store_for_atexit = store.clone();
            let path_for_atexit = path.clone();
            let save_fn = PyCFunction::new_closure(
                py,
                None,
                None,
                move |_args: &Bound<'_, PyTuple>, _kwargs: Option<&Bound<'_, PyDict>>| -> PyResult<()> {
                    let _ = store_for_atexit.save(&path_for_atexit);
                    Ok(())
                },
            )?;
            let atexit = py.import("atexit")?;
            atexit.call_method1("register", (save_fn,))?;
        }

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

        Ok(BurnerRedis { store, persistence_path })
    }

    /// Save the store to disk. Uses persistence_path if set, otherwise defaults to "burner-redis.dat".
    /// An explicit path argument overrides both.
    #[pyo3(signature = (path=None))]
    fn save<'py>(&self, py: Python<'py>, path: Option<String>) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        let save_path = path
            .or_else(|| self.persistence_path.clone())
            .unwrap_or_else(|| "burner-redis.dat".to_string());
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            store.save(&save_path).map_err(|e| {
                pyo3::exceptions::PyIOError::new_err(e.to_string())
            })?;
            Ok(true)
        })
    }

    /// Synchronous save for atexit handler. Saves to persistence_path or "burner-redis.dat".
    fn _save_sync(&self) -> PyResult<bool> {
        let path = self.persistence_path.as_deref()
            .unwrap_or("burner-redis.dat");
        self.store.save(path).map_err(|e| {
            pyo3::exceptions::PyIOError::new_err(e.to_string())
        })?;
        Ok(true)
    }

    /// Read-only property: the persistence path configured at construction, or None.
    #[getter]
    fn persistence_path(&self) -> Option<String> {
        self.persistence_path.clone()
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

    /// HGETALL command matching redis.asyncio.Redis.hgetall() signature.
    /// Returns dict of all field-value pairs as bytes->bytes.
    fn hgetall<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        let name_bytes = extract_bytes(name)?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let map = store.hgetall(&name_bytes).map_err(store_err_to_py)?;
            Python::try_attach(|py| -> PyResult<Py<PyAny>> {
                let dict = PyDict::new(py);
                for (k, v) in &map {
                    dict.set_item(
                        PyBytes::new(py, k.as_ref()),
                        PyBytes::new(py, v.as_ref()),
                    )?;
                }
                Ok(dict.into_any().unbind())
            })
            .ok_or_else(|| {
                PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                    "failed to attach to Python interpreter",
                )
            })?
        })
    }

    /// HEXISTS command matching redis.asyncio.Redis.hexists() signature.
    /// Returns bool: True if the field exists in the hash.
    fn hexists<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
        key: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        let name_bytes = extract_bytes(name)?;
        let field_bytes = extract_bytes(key)?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            store.hexists(&name_bytes, &field_bytes).map_err(store_err_to_py)
        })
    }

    /// HINCRBY command matching redis.asyncio.Redis.hincrby() signature.
    /// Increments the integer value of a hash field. Returns new value.
    #[pyo3(signature = (name, key, amount=1))]
    fn hincrby<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
        key: &Bound<'py, PyAny>,
        amount: i64,
    ) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        let name_bytes = extract_bytes(name)?;
        let field_bytes = extract_bytes(key)?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            store.hincrby(name_bytes, field_bytes, amount).map_err(store_err_to_py)
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

    /// ZCARD command matching redis.asyncio.Redis.zcard() signature.
    /// Returns the cardinality (member count) of a sorted set.
    fn zcard<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        let name_bytes = extract_bytes(name)?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            store.zcard(&name_bytes).map_err(store_err_to_py)
        })
    }

    /// ZSCORE command matching redis.asyncio.Redis.zscore() signature.
    /// Returns the score of a member in a sorted set, or None if not found.
    fn zscore<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
        value: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        let name_bytes = extract_bytes(name)?;
        let member_bytes = extract_bytes(value)?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            store.zscore(&name_bytes, &member_bytes).map_err(store_err_to_py)
        })
    }

    /// ZCOUNT command matching redis.asyncio.Redis.zcount() signature.
    /// Returns count of members with scores in [min, max] range.
    #[pyo3(signature = (name, min, max))]
    fn zcount<'py>(
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
            store.zcount(&name_bytes, min_f64, max_f64).map_err(store_err_to_py)
        })
    }

    // ── Key Commands ────────────────────────────────────────────────

    /// EXPIRE command matching redis.asyncio.Redis.expire() signature.
    /// Sets a timeout on an existing key in seconds. Returns True if set, False if key doesn't exist.
    /// Accepts int seconds or datetime.timedelta (extracts total_seconds).
    fn expire<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
        time: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        let name_bytes = extract_bytes(name)?;

        // Accept int or timedelta
        let seconds: u64 = if let Ok(secs) = time.extract::<u64>() {
            secs
        } else if let Ok(secs_f64) = time.call_method0("total_seconds")?.extract::<f64>() {
            secs_f64.max(0.0) as u64
        } else {
            return Err(pyo3::exceptions::PyTypeError::new_err(
                "expire time must be int (seconds) or timedelta",
            ));
        };

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            Ok(store.expire(&name_bytes, seconds))
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
    /// The block parameter is accepted for API compatibility but ignored (in-process DB).
    #[pyo3(signature = (streams, count=None, block=None))]
    fn xread<'py>(
        &self,
        py: Python<'py>,
        streams: &Bound<'py, PyDict>,
        count: Option<usize>,
        #[allow(unused_variables)]
        block: Option<u64>,  // Accepted for API compatibility, ignored (in-process DB)
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
    #[pyo3(signature = (name, maxlen=None, minid=None, approximate=true))]
    fn xtrim<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
        maxlen: Option<usize>,
        minid: Option<&str>,
        #[allow(unused_variables)]
        approximate: bool,
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

    /// XDEL command matching redis.asyncio.Redis.xdel() signature.
    /// Deletes specific entries from a stream by ID. Returns count of entries deleted.
    #[pyo3(signature = (name, *ids))]
    fn xdel<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
        ids: &Bound<'py, pyo3::types::PyTuple>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        let key = extract_bytes(name)?;

        // Parse each ID string into a StreamId
        let mut stream_ids: Vec<StreamId> = Vec::new();
        for id_obj in ids.iter() {
            let id_str: String = id_obj.extract::<String>().or_else(|_| {
                id_obj
                    .extract::<Vec<u8>>()
                    .map(|b| String::from_utf8_lossy(&b).into_owned())
            })?;
            let stream_id = parse_stream_id(&id_str).ok_or_else(|| {
                pyo3::exceptions::PyValueError::new_err(format!(
                    "Invalid stream ID format: {}",
                    id_str
                ))
            })?;
            stream_ids.push(stream_id);
        }

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            store.xdel(&key, &stream_ids).map_err(store_err_to_py)
        })
    }

    /// XRANGE command matching redis.asyncio.Redis.xrange() signature.
    /// Returns stream entries in ID range. Supports "-" as min and "+" as max.
    /// Returns list of (id_bytes, {field_bytes: value_bytes}) tuples.
    #[pyo3(signature = (name, min="-", max="+", count=None))]
    fn xrange<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
        min: &str,
        max: &str,
        count: Option<usize>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        let key = extract_bytes(name)?;

        // Parse min: "-" means (0, 0)
        let min_id: StreamId = if min == "-" {
            (0, 0)
        } else {
            parse_stream_id(min).ok_or_else(|| {
                pyo3::exceptions::PyValueError::new_err(format!(
                    "Invalid stream ID format: {}",
                    min
                ))
            })?
        };

        // Parse max: "+" means (u64::MAX, u64::MAX)
        let max_id: StreamId = if max == "+" {
            (u64::MAX, u64::MAX)
        } else {
            parse_stream_id(max).ok_or_else(|| {
                pyo3::exceptions::PyValueError::new_err(format!(
                    "Invalid stream ID format: {}",
                    max
                ))
            })?
        };

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let entries = store.xrange(&key, min_id, max_id, count).map_err(store_err_to_py)?;

            Python::try_attach(|py| -> PyResult<Py<PyAny>> {
                let result_list = pyo3::types::PyList::empty(py);
                for (id, fields) in &entries {
                    let id_bytes = format_stream_id(*id).into_bytes();
                    let field_dict = PyDict::new(py);
                    for (fk, fv) in fields {
                        field_dict.set_item(
                            PyBytes::new(py, fk.as_ref()),
                            PyBytes::new(py, fv.as_ref()),
                        )?;
                    }
                    let tuple = pyo3::types::PyTuple::new(
                        py,
                        &[
                            PyBytes::new(py, &id_bytes).into_any(),
                            field_dict.into_any(),
                        ],
                    )?;
                    result_list.append(tuple)?;
                }
                Ok(result_list.into_any().unbind())
            })
            .ok_or_else(|| {
                PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                    "failed to attach to Python interpreter",
                )
            })?
        })
    }

    // ── Consumer Group Commands ───────────────────────────────────────

    /// XGROUP CREATE command matching redis.asyncio.Redis.xgroup_create() signature.
    /// Creates a consumer group on a stream. Returns True on success.
    #[pyo3(signature = (name, groupname, id="$", mkstream=false))]
    fn xgroup_create<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
        groupname: &Bound<'py, PyAny>,
        id: &str,
        mkstream: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        let key = extract_bytes(name)?;
        let group = extract_bytes(groupname)?;

        // Parse id: "$" means latest (sentinel u64::MAX, u64::MAX), "0" or "0-0" means beginning
        let stream_id: StreamId = if id == "$" {
            (u64::MAX, u64::MAX) // sentinel for "use stream's last_id"
        } else if id == "0" || id == "0-0" {
            (0, 0)
        } else {
            parse_stream_id(id).ok_or_else(|| {
                pyo3::exceptions::PyValueError::new_err(format!(
                    "Invalid stream ID format: {}",
                    id
                ))
            })?
        };

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            store
                .xgroup_create(&key, group, stream_id, mkstream)
                .map_err(store_err_to_py)?;
            Ok(true)
        })
    }

    /// XGROUP DESTROY command matching redis.asyncio.Redis.xgroup_destroy() signature.
    /// Removes a consumer group. Returns 1 if destroyed, 0 if not found.
    #[pyo3(signature = (name, groupname))]
    fn xgroup_destroy<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
        groupname: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        let key = extract_bytes(name)?;
        let group = extract_bytes(groupname)?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let destroyed = store.xgroup_destroy(&key, &group).map_err(store_err_to_py)?;
            Ok(if destroyed { 1i64 } else { 0i64 })
        })
    }

    /// XREADGROUP command matching redis.asyncio.Redis.xreadgroup() signature.
    /// Reads entries from streams as a consumer in a group.
    /// Returns list of [stream_name, [(id, {field: value}), ...]] or empty list.
    /// When block is specified, waits for new entries up to the given timeout in milliseconds.
    #[pyo3(signature = (groupname, consumername, streams, count=None, block=None, noack=false))]
    fn xreadgroup<'py>(
        &self,
        py: Python<'py>,
        groupname: &Bound<'py, PyAny>,
        consumername: &Bound<'py, PyAny>,
        streams: &Bound<'py, PyDict>,
        count: Option<usize>,
        block: Option<u64>,
        #[allow(unused_variables)]
        noack: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        let group = extract_bytes(groupname)?;
        let consumer = extract_bytes(consumername)?;

        // Extract streams dict: keys are stream names, values are ID strings
        let mut keys: Vec<Bytes> = Vec::new();
        let mut id_strs: Vec<String> = Vec::new();

        for (k, v) in streams.iter() {
            let key = extract_bytes(&k)?;
            let id_str: String = v.extract::<String>().or_else(|_| {
                v.extract::<Vec<u8>>()
                    .map(|b| String::from_utf8_lossy(&b).into_owned())
            })?;
            keys.push(key);
            id_strs.push(id_str);
        }

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            // First non-blocking attempt
            let results = store
                .xreadgroup(&group, &consumer, &keys, &id_strs, count)
                .map_err(store_err_to_py)?;

            if !results.is_empty() || block.is_none() {
                return format_xreadgroup_result(results);
            }

            // Blocking: wait for stream notification or timeout, retrying until
            // data is available or the deadline expires. A single notify may fire
            // because a different stream received data, or another consumer claimed
            // the entry first, so we loop until we get results or time out.
            let block_ms = block.unwrap();
            let notify = store.stream_notify();
            let timeout_duration = Duration::from_millis(block_ms);
            let deadline = tokio::time::Instant::now() + timeout_duration;

            loop {
                let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                if remaining.is_zero() {
                    break format_xreadgroup_result(Vec::new());
                }
                tokio::select! {
                    _ = notify.notified() => {
                        // New data arrived, retry read
                        let results = store
                            .xreadgroup(&group, &consumer, &keys, &id_strs, count)
                            .map_err(store_err_to_py)?;
                        if !results.is_empty() {
                            break format_xreadgroup_result(results);
                        }
                        // No data for this consumer yet; loop and wait again
                    }
                    _ = tokio::time::sleep(remaining) => {
                        // Deadline reached, return empty
                        break format_xreadgroup_result(Vec::new());
                    }
                }
            }
        })
    }

    /// XACK command matching redis.asyncio.Redis.xack() signature.
    /// Acknowledges messages in a consumer group. Returns count acknowledged.
    #[pyo3(signature = (name, groupname, *ids))]
    fn xack<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
        groupname: &Bound<'py, PyAny>,
        ids: &Bound<'py, pyo3::types::PyTuple>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        let key = extract_bytes(name)?;
        let group = extract_bytes(groupname)?;

        // Parse each ID string into a StreamId
        let mut stream_ids: Vec<StreamId> = Vec::new();
        for id_obj in ids.iter() {
            let id_str: String = id_obj.extract::<String>().or_else(|_| {
                id_obj
                    .extract::<Vec<u8>>()
                    .map(|b| String::from_utf8_lossy(&b).into_owned())
            })?;
            let stream_id = parse_stream_id(&id_str).ok_or_else(|| {
                pyo3::exceptions::PyValueError::new_err(format!(
                    "Invalid stream ID format: {}",
                    id_str
                ))
            })?;
            stream_ids.push(stream_id);
        }

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let count = store.xack(&key, &group, &stream_ids).map_err(store_err_to_py)?;
            Ok(count)
        })
    }

    /// XAUTOCLAIM command matching redis.asyncio.Redis.xautoclaim() signature.
    /// Reclaims idle pending messages from other consumers. Returns tuple:
    /// (next_start_id_bytes, [(id_bytes, {field: value}), ...], [deleted_id_bytes, ...])
    #[pyo3(signature = (name, groupname, consumername, min_idle_time, start_id="0-0", count=None))]
    fn xautoclaim<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
        groupname: &Bound<'py, PyAny>,
        consumername: &Bound<'py, PyAny>,
        min_idle_time: u64,
        start_id: &str,
        count: Option<usize>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        let key = extract_bytes(name)?;
        let group = extract_bytes(groupname)?;
        let consumer_bytes = extract_bytes(consumername)?;

        // Parse start_id; treat "0" as (0,0)
        let start: StreamId = if start_id == "0" || start_id == "0-0" {
            (0, 0)
        } else {
            parse_stream_id(start_id).ok_or_else(|| {
                pyo3::exceptions::PyValueError::new_err(format!(
                    "Invalid stream ID format: {}",
                    start_id
                ))
            })?
        };

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let (next_id, claimed, deleted) = store
                .xautoclaim(&key, &group, consumer_bytes, min_idle_time, start, count)
                .map_err(store_err_to_py)?;

            Python::try_attach(|py| -> PyResult<pyo3::Py<pyo3::PyAny>> {
                // Build next_start_id as bytes
                let next_id_bytes =
                    pyo3::types::PyBytes::new(py, format_stream_id(next_id).as_bytes());

                // Build claimed entries list: [(id_bytes, {field: value}), ...]
                let claimed_list = pyo3::types::PyList::empty(py);
                for (id, fields) in &claimed {
                    let id_bytes =
                        pyo3::types::PyBytes::new(py, format_stream_id(*id).as_bytes());
                    let field_dict = pyo3::types::PyDict::new(py);
                    for (fk, fv) in fields {
                        field_dict.set_item(
                            pyo3::types::PyBytes::new(py, fk.as_ref()),
                            pyo3::types::PyBytes::new(py, fv.as_ref()),
                        )?;
                    }
                    let tuple = pyo3::types::PyTuple::new(
                        py,
                        &[id_bytes.into_any(), field_dict.into_any()],
                    )?;
                    claimed_list.append(tuple)?;
                }

                // Build deleted IDs list
                let deleted_list = pyo3::types::PyList::empty(py);
                for id in &deleted {
                    let id_bytes =
                        pyo3::types::PyBytes::new(py, format_stream_id(*id).as_bytes());
                    deleted_list.append(id_bytes)?;
                }

                // Return as tuple: (next_id, claimed, deleted)
                let result = pyo3::types::PyTuple::new(
                    py,
                    &[
                        next_id_bytes.into_any(),
                        claimed_list.into_any(),
                        deleted_list.into_any(),
                    ],
                )?;
                Ok(result.into_any().unbind())
            })
            .ok_or_else(|| {
                PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                    "failed to attach to Python interpreter",
                )
            })?
        })
    }

    /// XCLAIM command matching redis.asyncio.Redis.xclaim() signature.
    /// Transfers ownership of pending stream entries to a different consumer.
    #[pyo3(signature = (name, groupname, consumername, min_idle_time, message_ids, idle=None, time=None, retrycount=None, force=false, justid=false))]
    fn xclaim<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
        groupname: &Bound<'py, PyAny>,
        consumername: &Bound<'py, PyAny>,
        min_idle_time: u64,
        message_ids: &Bound<'py, PyAny>,
        idle: Option<u64>,
        time: Option<u64>,
        retrycount: Option<u64>,
        force: bool,
        justid: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        let key = extract_bytes(name)?;
        let group = extract_bytes(groupname)?;
        let consumer = extract_bytes(consumername)?;

        // Parse message_ids from Python list/tuple of bytes/str
        let ids_list: Vec<Py<PyAny>> = message_ids.extract()?;
        let mut ids: Vec<StreamId> = Vec::new();
        for id_obj in &ids_list {
            let id_str: String = id_obj.bind(py).extract::<String>().or_else(|_| {
                id_obj
                    .bind(py)
                    .extract::<Vec<u8>>()
                    .map(|b| String::from_utf8_lossy(&b).into_owned())
            })?;
            ids.push(parse_stream_id(&id_str).ok_or_else(|| {
                PyErr::new::<pyo3::exceptions::PyValueError, _>(format!(
                    "Invalid stream ID: {}",
                    id_str
                ))
            })?);
        }

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let results = store
                .xclaim(
                    &key,
                    &group,
                    consumer,
                    min_idle_time,
                    &ids,
                    idle,
                    time,
                    retrycount,
                    force,
                    justid,
                )
                .map_err(store_err_to_py)?;

            Python::try_attach(|py| -> PyResult<Py<PyAny>> {
                let outer = pyo3::types::PyList::empty(py);
                for (id, fields_opt) in &results {
                    if justid {
                        let id_bytes = format_stream_id(*id).into_bytes();
                        outer.append(PyBytes::new(py, &id_bytes))?;
                    } else if let Some(fields) = fields_opt {
                        let id_bytes = format_stream_id(*id).into_bytes();
                        let field_dict = PyDict::new(py);
                        for (fk, fv) in fields {
                            field_dict.set_item(
                                PyBytes::new(py, fk.as_ref()),
                                PyBytes::new(py, fv.as_ref()),
                            )?;
                        }
                        let tuple = PyTuple::new(
                            py,
                            &[
                                PyBytes::new(py, &id_bytes).into_any(),
                                field_dict.into_any(),
                            ],
                        )?;
                        outer.append(tuple)?;
                    }
                }
                Ok(outer.into_any().unbind())
            })
            .ok_or_else(|| {
                PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                    "failed to attach to Python interpreter",
                )
            })?
        })
    }

    /// XINFO GROUPS command matching redis.asyncio.Redis.xinfo_groups() signature.
    /// Returns list of dicts with group metadata.
    fn xinfo_groups<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        let key = extract_bytes(name)?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let groups = store.xinfo_groups(&key).map_err(store_err_to_py)?;

            Python::try_attach(|py| -> PyResult<pyo3::Py<pyo3::PyAny>> {
                let result_list = pyo3::types::PyList::empty(py);
                for group_info in &groups {
                    let dict = pyo3::types::PyDict::new(py);
                    // name -> bytes
                    if let Some(name_val) = group_info.get("name") {
                        dict.set_item(
                            pyo3::types::PyBytes::new(py, b"name"),
                            pyo3::types::PyBytes::new(py, name_val.as_bytes()),
                        )?;
                    }
                    // consumers -> int
                    if let Some(consumers_val) = group_info.get("consumers") {
                        let count: i64 = consumers_val.parse().unwrap_or(0);
                        dict.set_item(
                            pyo3::types::PyBytes::new(py, b"consumers"),
                            count,
                        )?;
                    }
                    // pending -> int
                    if let Some(pending_val) = group_info.get("pending") {
                        let count: i64 = pending_val.parse().unwrap_or(0);
                        dict.set_item(
                            pyo3::types::PyBytes::new(py, b"pending"),
                            count,
                        )?;
                    }
                    // last-delivered-id -> bytes
                    if let Some(id_val) = group_info.get("last-delivered-id") {
                        dict.set_item(
                            pyo3::types::PyBytes::new(py, b"last-delivered-id"),
                            pyo3::types::PyBytes::new(py, id_val.as_bytes()),
                        )?;
                    }
                    result_list.append(dict)?;
                }
                Ok(result_list.into_any().unbind())
            })
            .ok_or_else(|| {
                PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                    "failed to attach to Python interpreter",
                )
            })?
        })
    }

    /// XINFO CONSUMERS command matching redis.asyncio.Redis.xinfo_consumers() signature.
    /// Returns list of dicts with consumer metadata for a specific group.
    fn xinfo_consumers<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
        groupname: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        let key = extract_bytes(name)?;
        let group = extract_bytes(groupname)?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let consumers = store
                .xinfo_consumers(&key, &group)
                .map_err(store_err_to_py)?;

            Python::try_attach(|py| -> PyResult<pyo3::Py<pyo3::PyAny>> {
                let result_list = pyo3::types::PyList::empty(py);
                for consumer_info in &consumers {
                    let dict = pyo3::types::PyDict::new(py);
                    // name -> bytes
                    if let Some(name_val) = consumer_info.get("name") {
                        dict.set_item(
                            pyo3::types::PyBytes::new(py, b"name"),
                            pyo3::types::PyBytes::new(py, name_val.as_bytes()),
                        )?;
                    }
                    // pending -> int
                    if let Some(pending_val) = consumer_info.get("pending") {
                        let count: i64 = pending_val.parse().unwrap_or(0);
                        dict.set_item(
                            pyo3::types::PyBytes::new(py, b"pending"),
                            count,
                        )?;
                    }
                    // idle -> int
                    if let Some(idle_val) = consumer_info.get("idle") {
                        let idle: i64 = idle_val.parse().unwrap_or(0);
                        dict.set_item(
                            pyo3::types::PyBytes::new(py, b"idle"),
                            idle,
                        )?;
                    }
                    result_list.append(dict)?;
                }
                Ok(result_list.into_any().unbind())
            })
            .ok_or_else(|| {
                PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                    "failed to attach to Python interpreter",
                )
            })?
        })
    }

    /// XPENDING RANGE command matching redis.asyncio.Redis.xpending_range() signature.
    /// Returns list of dicts with message_id, consumer, time_since_delivered, times_delivered.
    #[pyo3(signature = (name, groupname, min="-", max="+", count=100, consumername=None, idle=None))]
    fn xpending_range<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
        groupname: &Bound<'py, PyAny>,
        min: &str,
        max: &str,
        count: usize,
        consumername: Option<&Bound<'py, PyAny>>,
        idle: Option<u64>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        let key = extract_bytes(name)?;
        let group = extract_bytes(groupname)?;
        let consumer_filter = match consumername {
            Some(c) => Some(extract_bytes(c)?),
            None => None,
        };

        // Parse min: "-" means (0, 0)
        let min_id: StreamId = if min == "-" {
            (0, 0)
        } else {
            parse_stream_id(min).ok_or_else(|| {
                pyo3::exceptions::PyValueError::new_err(format!(
                    "Invalid stream ID format: {}",
                    min
                ))
            })?
        };

        // Parse max: "+" means (u64::MAX, u64::MAX)
        let max_id: StreamId = if max == "+" {
            (u64::MAX, u64::MAX)
        } else {
            parse_stream_id(max).ok_or_else(|| {
                pyo3::exceptions::PyValueError::new_err(format!(
                    "Invalid stream ID format: {}",
                    max
                ))
            })?
        };

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let entries = store
                .xpending_range(
                    &key,
                    &group,
                    min_id,
                    max_id,
                    count,
                    consumer_filter.as_ref(),
                    idle,
                )
                .map_err(store_err_to_py)?;

            Python::try_attach(|py| -> PyResult<pyo3::Py<pyo3::PyAny>> {
                let result_list = pyo3::types::PyList::empty(py);
                for (entry_id, consumer_name, idle_ms, delivery_count) in &entries {
                    let dict = pyo3::types::PyDict::new(py);
                    // message_id -> bytes
                    let id_str = format_stream_id(*entry_id).into_bytes();
                    dict.set_item(
                        pyo3::types::PyBytes::new(py, b"message_id"),
                        pyo3::types::PyBytes::new(py, &id_str),
                    )?;
                    // consumer -> bytes
                    dict.set_item(
                        pyo3::types::PyBytes::new(py, b"consumer"),
                        pyo3::types::PyBytes::new(py, consumer_name.as_ref()),
                    )?;
                    // time_since_delivered -> int (milliseconds)
                    dict.set_item(
                        pyo3::types::PyBytes::new(py, b"time_since_delivered"),
                        *idle_ms as i64,
                    )?;
                    // times_delivered -> int
                    dict.set_item(
                        pyo3::types::PyBytes::new(py, b"times_delivered"),
                        *delivery_count as i64,
                    )?;
                    result_list.append(dict)?;
                }
                Ok(result_list.into_any().unbind())
            })
            .ok_or_else(|| {
                PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                    "failed to attach to Python interpreter",
                )
            })?
        })
    }

    // ── Scripting Commands ────────────────────────────────────────────

    /// EVAL command matching redis.asyncio.Redis.eval() signature.
    /// Executes a Lua script with KEYS and ARGV arrays.
    /// The first `numkeys` args after numkeys are KEYS, the rest are ARGV.
    #[pyo3(signature = (script, numkeys, *keys_and_args))]
    fn eval<'py>(
        &self,
        py: Python<'py>,
        script: String,
        numkeys: usize,
        keys_and_args: &Bound<'py, pyo3::types::PyTuple>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();

        // Split keys_and_args: first numkeys are KEYS, rest are ARGV
        let mut keys: Vec<Bytes> = Vec::new();
        let mut args: Vec<Bytes> = Vec::new();
        for (i, obj) in keys_and_args.iter().enumerate() {
            let b = extract_bytes(&obj)?;
            if i < numkeys {
                keys.push(b);
            } else {
                args.push(b);
            }
        }

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let result = store.eval(&script, keys, args);
            match result {
                Ok(val) => {
                    Python::try_attach(|py| redis_value_to_py(py, val))
                        .ok_or_else(|| {
                            PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                                "failed to attach to Python interpreter",
                            )
                        })?
                }
                Err(msg) => Err(make_response_error(msg)),
            }
        })
    }

    /// EVALSHA command matching redis.asyncio.Redis.evalsha() signature.
    /// Executes a cached Lua script by its SHA1 hash.
    /// Returns NOSCRIPT error if the SHA is not in the cache.
    #[pyo3(signature = (sha, numkeys, *keys_and_args))]
    fn evalsha<'py>(
        &self,
        py: Python<'py>,
        sha: String,
        numkeys: usize,
        keys_and_args: &Bound<'py, pyo3::types::PyTuple>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();

        // Split keys_and_args: first numkeys are KEYS, rest are ARGV
        let mut keys: Vec<Bytes> = Vec::new();
        let mut args: Vec<Bytes> = Vec::new();
        for (i, obj) in keys_and_args.iter().enumerate() {
            let b = extract_bytes(&obj)?;
            if i < numkeys {
                keys.push(b);
            } else {
                args.push(b);
            }
        }

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let result = store.evalsha(&sha, keys, args);
            match result {
                Ok(val) => {
                    Python::try_attach(|py| redis_value_to_py(py, val))
                        .ok_or_else(|| {
                            PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                                "failed to attach to Python interpreter",
                            )
                        })?
                }
                Err(msg) => Err(make_response_error(msg)),
            }
        })
    }

    /// SCRIPT LOAD command - caches a Lua script and returns its SHA1 hash.
    fn script_load<'py>(
        &self,
        py: Python<'py>,
        script: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let sha = store.script_load(&script);
            Ok(sha)
        })
    }

    /// SCRIPT EXISTS command - checks if one or more scripts are cached by SHA1 hash.
    /// Returns list of bools.
    #[pyo3(signature = (*args))]
    fn script_exists<'py>(
        &self,
        py: Python<'py>,
        args: &Bound<'py, pyo3::types::PyTuple>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();

        let shas: Vec<String> = args
            .iter()
            .map(|obj| obj.extract::<String>())
            .collect::<PyResult<Vec<_>>>()?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let results = store.script_exists(&shas);
            Ok(results)
        })
    }

    // -- Pub/Sub Commands --

    /// PUBLISH: Send a message to a channel. Returns number of subscribers that received it.
    fn publish<'py>(&self, py: Python<'py>, channel: &Bound<'py, PyAny>, message: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
        let channel_bytes = extract_bytes(channel)?;
        let message_bytes = extract_bytes(message)?;
        let store = self.store.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let count = store.publish(
                Bytes::from(channel_bytes),
                Bytes::from(message_bytes),
            );
            Ok(count)
        })
    }

    /// Internal: Create a new subscriber ID. Used by Python PubSub class.
    /// Returns the subscriber_id as an integer. The Python side manages the
    /// broadcast receiver via a background Tokio task.
    fn _new_subscriber(&self) -> PyResult<u64> {
        let (id, _rx) = self.store.new_subscriber();
        // Note: _rx is dropped here. The Python PubSub class will set up its own
        // message consumption via _subscribe_listener.
        Ok(id)
    }

    /// Internal: Subscribe and start a background task that filters messages
    /// into a Python asyncio.Queue. Returns subscriber_id.
    /// Called once by Python PubSub on first subscribe.
    fn _subscribe_listener<'py>(&self, py: Python<'py>, subscriber_id: u64, queue: Py<PyAny>) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let (_id, mut rx) = {
                // Get a receiver from the broadcast channel
                // NOTE: store.pubsub is pub(crate), set in Task 1 specifically for this access
                let registry = store.pubsub.read();
                (subscriber_id, registry.tx.subscribe())
            };

            // Spawn a background Tokio task that forwards matching messages to the Python queue
            tokio::spawn(async move {
                loop {
                    match rx.recv().await {
                        Ok(msg) => {
                            // Push message to Python asyncio.Queue
                            // Use try_attach (PyO3 0.28.3) to acquire the GIL from a background task
                            let delivered = Python::try_attach(|py| -> Result<(), PyErr> {
                                let dict = PyDict::new(py);
                                dict.set_item("type", &msg.kind)?;
                                match &msg.pattern {
                                    Some(p) => dict.set_item("pattern", PyBytes::new(py, p))?,
                                    None => dict.set_item("pattern", py.None())?,
                                };
                                dict.set_item("channel", PyBytes::new(py, &msg.channel))?;
                                dict.set_item("data", PyBytes::new(py, &msg.data))?;
                                let put_nowait = queue.getattr(py, "put_nowait")?;
                                put_nowait.call1(py, (dict,))?;
                                Ok(())
                            });
                            match delivered {
                                Some(Ok(())) => {
                                    // Message delivered successfully
                                }
                                Some(Err(e)) => {
                                    // Python error (e.g. QueueFull) -- log and continue
                                    eprintln!("burner-redis pubsub: delivery error: {}", e);
                                }
                                None => {
                                    // GIL not available -- transient, continue
                                }
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            // Messages were lost due to slow consumer -- continue
                            eprintln!("burner-redis pubsub: subscriber lagged, {} messages dropped", n);
                            continue;
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            // Broadcast channel closed (Store dropped) -- stop
                            break;
                        }
                    }
                }
            });

            Ok(subscriber_id)
        })
    }

    /// SUBSCRIBE: Register channels for a subscriber.
    /// Returns list of (channel_bytes, subscription_count) tuples.
    fn subscribe_channels<'py>(&self, py: Python<'py>, subscriber_id: u64, channels: Vec<Vec<u8>>) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let channel_bytes: Vec<Bytes> = channels.into_iter().map(Bytes::from).collect();
            let results = store.subscribe(subscriber_id, channel_bytes);
            let tuples: Vec<(Vec<u8>, i64)> = results.into_iter()
                .map(|(ch, count)| (ch.to_vec(), count))
                .collect();
            Ok(tuples)
        })
    }

    /// UNSUBSCRIBE: Remove channels from a subscriber.
    /// Returns list of (channel_bytes, remaining_subscription_count) tuples.
    fn unsubscribe_channels<'py>(&self, py: Python<'py>, subscriber_id: u64, channels: Vec<Vec<u8>>) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let channel_bytes: Vec<Bytes> = channels.into_iter().map(Bytes::from).collect();
            let results = store.unsubscribe(subscriber_id, channel_bytes);
            let tuples: Vec<(Vec<u8>, i64)> = results.into_iter()
                .map(|(ch, count)| (ch.to_vec(), count))
                .collect();
            Ok(tuples)
        })
    }

    /// PSUBSCRIBE: Register glob patterns for a subscriber.
    /// Returns list of (pattern_bytes, subscription_count) tuples.
    fn psubscribe_patterns<'py>(&self, py: Python<'py>, subscriber_id: u64, patterns: Vec<Vec<u8>>) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let pattern_bytes: Vec<Bytes> = patterns.into_iter().map(Bytes::from).collect();
            let results = store.psubscribe(subscriber_id, pattern_bytes);
            let tuples: Vec<(Vec<u8>, i64)> = results.into_iter()
                .map(|(pat, count)| (pat.to_vec(), count))
                .collect();
            Ok(tuples)
        })
    }

    /// PUNSUBSCRIBE: Remove glob patterns from a subscriber.
    /// Returns list of (pattern_bytes, remaining_subscription_count) tuples.
    fn punsubscribe_patterns<'py>(&self, py: Python<'py>, subscriber_id: u64, patterns: Vec<Vec<u8>>) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let pattern_bytes: Vec<Bytes> = patterns.into_iter().map(Bytes::from).collect();
            let results = store.punsubscribe(subscriber_id, pattern_bytes);
            let tuples: Vec<(Vec<u8>, i64)> = results.into_iter()
                .map(|(pat, count)| (pat.to_vec(), count))
                .collect();
            Ok(tuples)
        })
    }

    /// PUBSUB CHANNELS: Return active channels matching optional glob pattern.
    #[pyo3(signature = (pattern=None))]
    fn pubsub_channels<'py>(&self, py: Python<'py>, pattern: Option<Vec<u8>>) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let pat_bytes = pattern.map(Bytes::from);
            let channels = store.pubsub_channels(pat_bytes.as_ref());
            let result: Vec<Vec<u8>> = channels.into_iter().map(|ch| ch.to_vec()).collect();
            Ok(result)
        })
    }

    /// PUBSUB NUMSUB: Return (channel, count) for requested channels.
    fn pubsub_numsub<'py>(&self, py: Python<'py>, channels: Vec<Vec<u8>>) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let channel_bytes: Vec<Bytes> = channels.into_iter().map(Bytes::from).collect();
            let results = store.pubsub_numsub(channel_bytes);
            let tuples: Vec<(Vec<u8>, i64)> = results.into_iter()
                .map(|(ch, count)| (ch.to_vec(), count))
                .collect();
            Ok(tuples)
        })
    }

    /// PUBSUB NUMPAT: Return total number of active pattern subscriptions.
    fn pubsub_numpat<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            Ok(store.pubsub_numpat())
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
