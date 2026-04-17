use bytes::Bytes;
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyBytes, PyCFunction, PyDict, PyList, PyTuple};
use std::collections::HashSet as StdHashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
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
use store::{Store, StoreError, XInfoStreamSnapshot};

/// A pre-resolved Python awaitable. Returns the stored value immediately
/// on the first `__next__` call via `StopIteration(value)`.
///
/// This eliminates Tokio scheduling and asyncio coroutine overhead for
/// commands that execute synchronously (no I/O, no blocking).
#[pyclass]
struct ResolvedFuture {
    result: Option<Py<PyAny>>,
}

#[pymethods]
impl ResolvedFuture {
    fn __await__(slf: Py<Self>) -> Py<Self> {
        slf
    }
    fn __next__(&mut self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let val = match self.result.take() {
            Some(v) => v,
            None => py.None(),
        };
        // Wrap in a single-element tuple to prevent Python from unpacking
        // tuple values as StopIteration constructor arguments.
        // StopIteration((val,)).value == (val,) but we want .value == val,
        // so we construct StopIteration and set .value directly.
        let exc = pyo3::exceptions::PyStopIteration::new_err(());
        let err_val = exc.value(py);
        err_val.setattr("value", val.bind(py))?;
        Err(exc)
    }
    fn __iter__(slf: Py<Self>) -> Py<Self> {
        slf
    }
}

/// Wrap an already-computed PyObject into a ResolvedFuture awaitable.
fn resolved<'py>(py: Python<'py>, value: Py<PyAny>) -> PyResult<Bound<'py, PyAny>> {
    Ok(Bound::new(py, ResolvedFuture { result: Some(value) })?.into_any())
}

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

/// Convert a StoreError from a stream-group command into a Python exception
/// with the per-command NOGROUP suffix, matching Redis canonical error text.
///
/// For `StoreError::NoGroup` the suffix " in <CMD>" is appended (e.g.
/// "NOGROUP No such key 'x' or consumer group 'g' in XPENDING"). All other
/// StoreError variants pass through unchanged.
fn store_err_to_py_for_cmd(e: StoreError, cmd: &str) -> PyErr {
    let msg = match &e {
        StoreError::NoGroup(_, _) => format!("{} in {}", e, cmd),
        _ => e.to_string(),
    };
    make_response_error(msg)
}

/// Format XREADGROUP results into a Python list structure (GIL-holding version).
/// Used by the synchronous non-blocking xreadgroup path and execute_pipeline.
fn format_xreadgroup_result_with_py(
    py: Python<'_>,
    results: Vec<(Bytes, Vec<(commands::streams::StreamId, std::collections::HashMap<Bytes, Bytes>)>)>,
) -> PyResult<Py<PyAny>> {
    if results.is_empty() {
        return Ok(pyo3::types::PyList::empty(py).into_any().unbind());
    }

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
}

/// Format XREADGROUP results into a Python list structure (async version).
/// Used by the blocking xreadgroup path which runs inside a Tokio future
/// and must acquire the GIL via try_attach.
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
        format_xreadgroup_result_with_py(py, results)
    })
    .ok_or_else(|| {
        PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
            "failed to attach to Python interpreter",
        )
    })?
}

/// Build the nested Python list structure for an XREAD result (GIL-holding version).
/// Shape: [[stream_name_bytes, [(id_bytes, {field_bytes: value_bytes}), ...]], ...]
fn build_xread_pylist<'py>(
    py: Python<'py>,
    results: &[(Bytes, Vec<(commands::streams::StreamId, std::collections::HashMap<Bytes, Bytes>)>)],
) -> PyResult<Bound<'py, pyo3::types::PyList>> {
    let outer = pyo3::types::PyList::empty(py);
    for (stream_name, entries) in results {
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
    Ok(outer)
}

/// Format XREAD results for the async blocking path. Empty results -> None
/// (matching the sync xread fast path), non-empty -> the nested list shape.
fn format_xread_result(
    results: Vec<(Bytes, Vec<(commands::streams::StreamId, std::collections::HashMap<Bytes, Bytes>)>)>,
) -> PyResult<pyo3::Py<pyo3::PyAny>> {
    Python::try_attach(|py| -> PyResult<pyo3::Py<pyo3::PyAny>> {
        if results.is_empty() {
            return Ok(py.None());
        }
        Ok(build_xread_pylist(py, &results)?.into_any().unbind())
    })
    .ok_or_else(|| {
        PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
            "failed to attach to Python interpreter",
        )
    })?
}

/// Build the redis-py-shaped dict for XINFO STREAM. Uses str keys (not bytes)
/// to match the xinfo_groups convention. first-entry / last-entry are either
/// None or (id_bytes, {field_bytes: value_bytes}) tuples.
fn build_xinfo_stream_dict<'py>(
    py: Python<'py>,
    snapshot: &XInfoStreamSnapshot,
) -> PyResult<Bound<'py, pyo3::types::PyDict>> {
    let dict = pyo3::types::PyDict::new(py);
    dict.set_item("length", snapshot.length as i64)?;
    // We don't use a radix tree under the hood; expose plausible integers so
    // downstream code that reads these keys doesn't KeyError.
    dict.set_item("radix-tree-keys", snapshot.length as i64)?;
    dict.set_item("radix-tree-nodes", (snapshot.length as i64) + 1)?;
    dict.set_item(
        "last-generated-id",
        PyBytes::new(py, format_stream_id(snapshot.last_id).as_bytes()),
    )?;
    dict.set_item("groups", snapshot.groups_count as i64)?;

    let format_entry = |entry: &(commands::streams::StreamId, std::collections::HashMap<Bytes, Bytes>)| -> PyResult<Bound<'py, PyAny>> {
        let (id, fields) = entry;
        let id_bytes = PyBytes::new(py, format_stream_id(*id).as_bytes());
        let field_dict = pyo3::types::PyDict::new(py);
        for (fk, fv) in fields {
            field_dict.set_item(
                PyBytes::new(py, fk.as_ref()),
                PyBytes::new(py, fv.as_ref()),
            )?;
        }
        let tuple = PyTuple::new(py, &[id_bytes.into_any(), field_dict.into_any()])?;
        Ok(tuple.into_any())
    };

    match &snapshot.first_entry {
        Some(e) => dict.set_item("first-entry", format_entry(e)?)?,
        None => dict.set_item("first-entry", py.None())?,
    }
    match &snapshot.last_entry {
        Some(e) => dict.set_item("last-entry", format_entry(e)?)?,
        None => dict.set_item("last-entry", py.None())?,
    }
    Ok(dict)
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
        let save_path = path
            .or_else(|| self.persistence_path.clone())
            .unwrap_or_else(|| "burner-redis.dat".to_string());
        self.store.save(&save_path).map_err(|e| {
            pyo3::exceptions::PyIOError::new_err(e.to_string())
        })?;
        resolved(py, pyo3::types::PyBool::new(py, true).to_owned().into_any().unbind())
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

        let success = self.store.set(key, val, ttl, nx, xx);
        let py_result = if success {
            pyo3::types::PyBool::new(py, true).to_owned().into_any().unbind()
        } else {
            py.None()
        };
        resolved(py, py_result)
    }

    /// GET command matching redis.asyncio.Redis.get() signature.
    /// Returns bytes or None.
    fn get<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let key = extract_bytes(name)?;
        let result = self.store.get(&key);
        let py_result = match result {
            Some(b) => PyBytes::new(py, &b).into_any().unbind(),
            None => py.None(),
        };
        resolved(py, py_result)
    }

    /// DELETE command matching redis.asyncio.Redis.delete() signature.
    /// Accepts variadic keys, returns count of deleted keys.
    #[pyo3(signature = (*names))]
    fn delete<'py>(
        &self,
        py: Python<'py>,
        names: &Bound<'py, pyo3::types::PyTuple>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let keys: Vec<Bytes> = names
            .iter()
            .map(|obj| extract_bytes(&obj))
            .collect::<PyResult<Vec<_>>>()?;
        let count = self.store.delete(&keys);
        resolved(py, count.into_pyobject(py)?.into_any().unbind())
    }

    /// EXISTS command matching redis.asyncio.Redis.exists() signature.
    /// Accepts variadic keys, returns count of existing keys.
    #[pyo3(signature = (*names))]
    fn exists<'py>(
        &self,
        py: Python<'py>,
        names: &Bound<'py, pyo3::types::PyTuple>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let keys: Vec<Bytes> = names
            .iter()
            .map(|obj| extract_bytes(&obj))
            .collect::<PyResult<Vec<_>>>()?;
        let count = self.store.exists(&keys);
        resolved(py, count.into_pyobject(py)?.into_any().unbind())
    }

    // -- Hash Commands ----

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

        let count = self.store.hset(name_bytes, fields).map_err(store_err_to_py)?;
        resolved(py, count.into_pyobject(py)?.into_any().unbind())
    }

    /// HGET command matching redis.asyncio.Redis.hget() signature.
    /// Returns bytes value or None.
    fn hget<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
        key: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let name_bytes = extract_bytes(name)?;
        let field_bytes = extract_bytes(key)?;
        let result = self.store.hget(&name_bytes, &field_bytes).map_err(store_err_to_py)?;
        let py_result = match result {
            Some(b) => PyBytes::new(py, &b).into_any().unbind(),
            None => py.None(),
        };
        resolved(py, py_result)
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
        let name_bytes = extract_bytes(name)?;
        let fields: Vec<Bytes> = keys
            .iter()
            .map(|obj| extract_bytes(&obj))
            .collect::<PyResult<Vec<_>>>()?;
        let count = self.store.hdel(&name_bytes, &fields).map_err(store_err_to_py)?;
        resolved(py, count.into_pyobject(py)?.into_any().unbind())
    }

    /// HVALS command matching redis.asyncio.Redis.hvals() signature.
    /// Returns list of all values in the hash as bytes.
    fn hvals<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let name_bytes = extract_bytes(name)?;
        let vals = self.store.hvals(&name_bytes).map_err(store_err_to_py)?;
        let py_list: Vec<Vec<u8>> = vals.into_iter().map(|b| b.to_vec()).collect();
        resolved(py, py_list.into_pyobject(py)?.into_any().unbind())
    }

    /// HGETALL command matching redis.asyncio.Redis.hgetall() signature.
    /// Returns dict of all field-value pairs as bytes->bytes.
    fn hgetall<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let name_bytes = extract_bytes(name)?;
        let map = self.store.hgetall(&name_bytes).map_err(store_err_to_py)?;
        let dict = PyDict::new(py);
        for (k, v) in &map {
            dict.set_item(
                PyBytes::new(py, k.as_ref()),
                PyBytes::new(py, v.as_ref()),
            )?;
        }
        resolved(py, dict.into_any().unbind())
    }

    /// HEXISTS command matching redis.asyncio.Redis.hexists() signature.
    /// Returns bool: True if the field exists in the hash.
    fn hexists<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
        key: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let name_bytes = extract_bytes(name)?;
        let field_bytes = extract_bytes(key)?;
        let exists = self.store.hexists(&name_bytes, &field_bytes).map_err(store_err_to_py)?;
        resolved(py, pyo3::types::PyBool::new(py, exists).to_owned().into_any().unbind())
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
        let name_bytes = extract_bytes(name)?;
        let field_bytes = extract_bytes(key)?;
        let new_val = self.store.hincrby(name_bytes, field_bytes, amount).map_err(store_err_to_py)?;
        resolved(py, new_val.into_pyobject(py)?.into_any().unbind())
    }

    // -- Set Commands ----

    /// SADD command matching redis.asyncio.Redis.sadd() signature.
    /// Accepts variadic members, returns count of NEW members added.
    #[pyo3(signature = (name, *values))]
    fn sadd<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
        values: &Bound<'py, pyo3::types::PyTuple>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let name_bytes = extract_bytes(name)?;
        let members: Vec<Bytes> = values
            .iter()
            .map(|obj| extract_bytes(&obj))
            .collect::<PyResult<Vec<_>>>()?;
        let count = self.store.sadd(name_bytes, members).map_err(store_err_to_py)?;
        resolved(py, count.into_pyobject(py)?.into_any().unbind())
    }

    /// SMEMBERS command matching redis.asyncio.Redis.smembers() signature.
    /// Returns a Python set of all members as bytes.
    fn smembers<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let name_bytes = extract_bytes(name)?;
        let members = self.store.smembers(&name_bytes).map_err(store_err_to_py)?;
        let set: StdHashSet<Vec<u8>> = members.into_iter().map(|b| b.to_vec()).collect();
        resolved(py, set.into_pyobject(py)?.into_any().unbind())
    }

    /// SISMEMBER command matching redis.asyncio.Redis.sismember() signature.
    /// Returns bool: True if the value is a member of the set.
    fn sismember<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
        value: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let name_bytes = extract_bytes(name)?;
        let member_bytes = extract_bytes(value)?;
        let is_member = self.store.sismember(&name_bytes, &member_bytes).map_err(store_err_to_py)?;
        resolved(py, pyo3::types::PyBool::new(py, is_member).to_owned().into_any().unbind())
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
        let name_bytes = extract_bytes(name)?;
        let members: Vec<Bytes> = values
            .iter()
            .map(|obj| extract_bytes(&obj))
            .collect::<PyResult<Vec<_>>>()?;
        let count = self.store.srem(&name_bytes, &members).map_err(store_err_to_py)?;
        resolved(py, count.into_pyobject(py)?.into_any().unbind())
    }

    // -- Sorted Set Commands ----

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
        let name_bytes = extract_bytes(name)?;

        // Extract mapping: {member: score} dict -> Vec<(f64, Bytes)>
        let mut members: Vec<(f64, Bytes)> = Vec::new();
        for (k, v) in mapping.iter() {
            let member = extract_bytes(&k)?;
            let score: f64 = v.extract::<f64>()?;
            members.push((score, member));
        }

        let count = self.store
            .zadd(name_bytes, members, nx, xx, gt, lt, ch)
            .map_err(store_err_to_py)?;
        resolved(py, count.into_pyobject(py)?.into_any().unbind())
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
        let name_bytes = extract_bytes(name)?;
        let members: Vec<Bytes> = values
            .iter()
            .map(|obj| extract_bytes(&obj))
            .collect::<PyResult<Vec<_>>>()?;
        let count = self.store.zrem(&name_bytes, &members).map_err(store_err_to_py)?;
        resolved(py, count.into_pyobject(py)?.into_any().unbind())
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
        let name_bytes = extract_bytes(name)?;
        let results = self.store
            .zrange(&name_bytes, start, end, withscores)
            .map_err(store_err_to_py)?;
        let py_result = if withscores {
            let list: Vec<(Vec<u8>, f64)> = results
                .into_iter()
                .map(|(m, s)| (m.to_vec(), s.unwrap_or(0.0)))
                .collect();
            list.into_pyobject(py)?.into_any().unbind()
        } else {
            let list: Vec<Vec<u8>> =
                results.into_iter().map(|(m, _)| m.to_vec()).collect();
            list.into_pyobject(py)?.into_any().unbind()
        };
        resolved(py, py_result)
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
        let name_bytes = extract_bytes(name)?;
        let min_f64 = parse_score_bound(min)?;
        let max_f64 = parse_score_bound(max)?;
        let results = self.store
            .zrangebyscore(&name_bytes, min_f64, max_f64, withscores)
            .map_err(store_err_to_py)?;
        let py_result = if withscores {
            let list: Vec<(Vec<u8>, f64)> = results
                .into_iter()
                .map(|(m, s)| (m.to_vec(), s.unwrap_or(0.0)))
                .collect();
            list.into_pyobject(py)?.into_any().unbind()
        } else {
            let list: Vec<Vec<u8>> =
                results.into_iter().map(|(m, _)| m.to_vec()).collect();
            list.into_pyobject(py)?.into_any().unbind()
        };
        resolved(py, py_result)
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
        let dst_bytes = extract_bytes(dest)?;
        let src_bytes = extract_bytes(name)?;
        let min_f64 = parse_score_bound(start)?;
        let max_f64 = parse_score_bound(end)?;
        let count = self.store
            .zrangestore(dst_bytes, &src_bytes, min_f64, max_f64)
            .map_err(store_err_to_py)?;
        resolved(py, count.into_pyobject(py)?.into_any().unbind())
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
        let name_bytes = extract_bytes(name)?;
        let min_f64 = parse_score_bound(min)?;
        let max_f64 = parse_score_bound(max)?;
        let count = self.store
            .zremrangebyscore(&name_bytes, min_f64, max_f64)
            .map_err(store_err_to_py)?;
        resolved(py, count.into_pyobject(py)?.into_any().unbind())
    }

    /// ZCARD command matching redis.asyncio.Redis.zcard() signature.
    /// Returns the cardinality (member count) of a sorted set.
    fn zcard<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let name_bytes = extract_bytes(name)?;
        let count = self.store.zcard(&name_bytes).map_err(store_err_to_py)?;
        resolved(py, count.into_pyobject(py)?.into_any().unbind())
    }

    /// ZSCORE command matching redis.asyncio.Redis.zscore() signature.
    /// Returns the score of a member in a sorted set, or None if not found.
    fn zscore<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
        value: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let name_bytes = extract_bytes(name)?;
        let member_bytes = extract_bytes(value)?;
        let score = self.store.zscore(&name_bytes, &member_bytes).map_err(store_err_to_py)?;
        resolved(py, score.into_pyobject(py)?.into_any().unbind())
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
        let name_bytes = extract_bytes(name)?;
        let min_f64 = parse_score_bound(min)?;
        let max_f64 = parse_score_bound(max)?;
        let count = self.store.zcount(&name_bytes, min_f64, max_f64).map_err(store_err_to_py)?;
        resolved(py, count.into_pyobject(py)?.into_any().unbind())
    }

    // -- Key Commands ----

    /// EXPIRE command matching redis.asyncio.Redis.expire() signature.
    /// Sets a timeout on an existing key in seconds. Returns True if set, False if key doesn't exist.
    /// Accepts int seconds or datetime.timedelta (extracts total_seconds).
    fn expire<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
        time: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
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

        let result = self.store.expire(&name_bytes, seconds);
        resolved(py, pyo3::types::PyBool::new(py, result).to_owned().into_any().unbind())
    }

    // -- Stream Commands ----

    /// XADD command matching redis.asyncio.Redis.xadd() signature.
    /// Adds an entry to a stream. Returns the entry ID as bytes (e.g., b"1234567890123-0").
    #[pyo3(signature = (name, fields, id="*", maxlen=None, minid=None))]
    fn xadd<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
        fields: &Bound<'py, PyDict>,
        id: &str,
        #[allow(unused_variables)]
        maxlen: Option<usize>,
        #[allow(unused_variables)]
        minid: Option<&str>,
    ) -> PyResult<Bound<'py, PyAny>> {
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

        let stream_id = self.store.xadd(key, field_map, id_opt).map_err(store_err_to_py)?;
        let id_str = format_stream_id(stream_id);
        resolved(py, PyBytes::new(py, id_str.as_bytes()).into_any().unbind())
    }

    /// XREAD command matching redis.asyncio.Redis.xread() signature.
    /// Reads entries from one or more streams after given IDs.
    /// Returns list of [stream_name, [(id, {field: value}), ...]] or None if empty.
    /// When block is specified, waits for new entries up to the given timeout in milliseconds
    /// (block=0 means block indefinitely).
    #[pyo3(signature = (streams, count=None, block=None))]
    fn xread<'py>(
        &self,
        py: Python<'py>,
        streams: &Bound<'py, PyDict>,
        count: Option<usize>,
        block: Option<u64>,
    ) -> PyResult<Bound<'py, PyAny>> {
        // Extract stream names and IDs from the dict. "$" resolves to the
        // stream's current last_id at CALL TIME (not wakeup time) so that
        // any xadd after the call began shows up as "new".
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
            } else if id_str == "$" {
                // Resolve "$" to the stream's current last_id at call time.
                // Missing stream or wrong type -> (0, 0); xread itself will
                // surface WrongType via the store call below if applicable.
                self.store.stream_last_id(&key).unwrap_or((0, 0))
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

        // Non-blocking fast path
        if block.is_none() {
            let results = self.store.xread(&keys, &ids, count).map_err(store_err_to_py)?;

            if results.is_empty() {
                return resolved(py, py.None());
            }

            let outer = build_xread_pylist(py, &results)?;
            return resolved(py, outer.into_any().unbind());
        }

        // Blocking path: mirrors xreadgroup blocking loop (see lib.rs ~1129).
        // NOTE: Keep in sync with the xreadgroup blocking loop. DRY-via-helper
        // was evaluated and rejected: the two closures return different shapes
        // (xread -> Option<PyAny> with None-on-empty; xreadgroup -> list-on-empty)
        // and the Store methods have different signatures, so a generic helper
        // would add trait gymnastics without reducing real complexity.
        let store = self.store.clone();
        let block_ms = block.unwrap();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let notify = store.stream_notify();
            let mut waiter = Box::pin(notify.notified());
            waiter.as_mut().enable();

            // First non-blocking attempt
            let results = store.xread(&keys, &ids, count).map_err(store_err_to_py)?;
            if !results.is_empty() {
                return format_xread_result(results);
            }

            // block=0 means block forever; otherwise enforce a deadline.
            let deadline_opt = if block_ms == 0 {
                None
            } else {
                Some(tokio::time::Instant::now() + Duration::from_millis(block_ms))
            };

            loop {
                // Graceful shutdown: return empty immediately so the Rust future
                // completes and pyo3-async-runtimes can deliver via
                // call_soon_threadsafe while the event loop is still alive.
                if store.is_shutdown() {
                    break format_xread_result(Vec::new());
                }

                let remaining = match deadline_opt {
                    Some(d) => {
                        let r = d.saturating_duration_since(tokio::time::Instant::now());
                        if r.is_zero() {
                            break format_xread_result(Vec::new());
                        }
                        r
                    }
                    // block=0: long slice; re-armed on wakeup. Loop forever.
                    None => Duration::from_secs(3600),
                };

                tokio::select! {
                    _ = waiter.as_mut() => {
                        waiter.set(notify.notified());
                        waiter.as_mut().enable();
                        let results = store.xread(&keys, &ids, count).map_err(store_err_to_py)?;
                        if !results.is_empty() {
                            break format_xread_result(results);
                        }
                        // Otherwise: notification was for unrelated stream; loop.
                    }
                    _ = tokio::time::sleep(remaining) => {
                        if deadline_opt.is_some() {
                            break format_xread_result(Vec::new());
                        }
                        // block=0: sleep completed without wakeup; keep looping.
                    }
                }
            }
        })
    }

    /// XLEN command matching redis.asyncio.Redis.xlen() signature.
    /// Returns the number of entries in a stream.
    fn xlen<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let key = extract_bytes(name)?;
        let len = self.store.xlen(&key).map_err(store_err_to_py)?;
        resolved(py, (len as i64).into_pyobject(py)?.into_any().unbind())
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

        let trimmed = self.store
            .xtrim(&key, maxlen, minid_parsed)
            .map_err(store_err_to_py)?;
        resolved(py, (trimmed as i64).into_pyobject(py)?.into_any().unbind())
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

        let count = self.store.xdel(&key, &stream_ids).map_err(store_err_to_py)?;
        resolved(py, count.into_pyobject(py)?.into_any().unbind())
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

        let entries = self.store.xrange(&key, min_id, max_id, count).map_err(store_err_to_py)?;

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
        resolved(py, result_list.into_any().unbind())
    }

    // -- Consumer Group Commands ----

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

        self.store
            .xgroup_create(&key, group, stream_id, mkstream)
            .map_err(|e| store_err_to_py_for_cmd(e, "XGROUP CREATE"))?;
        resolved(py, pyo3::types::PyBool::new(py, true).to_owned().into_any().unbind())
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
        let key = extract_bytes(name)?;
        let group = extract_bytes(groupname)?;
        let destroyed = self.store.xgroup_destroy(&key, &group).map_err(store_err_to_py)?;
        let val = if destroyed { 1i64 } else { 0i64 };
        resolved(py, val.into_pyobject(py)?.into_any().unbind())
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
            // XREADGROUP uses '>' for "new entries for this consumer"; '$' is
            // an XREAD-only concept and must be rejected here to match Redis.
            if id_str == "$" {
                return Err(make_response_error(
                    "ERR the $ ID meaning is only valid within XREAD".to_string(),
                ));
            }
            keys.push(key);
            id_strs.push(id_str);
        }

        // Non-blocking path: execute synchronously
        if block.is_none() {
            let results = self.store
                .xreadgroup(&group, &consumer, &keys, &id_strs, count)
                .map_err(|e| store_err_to_py_for_cmd(e, "XREADGROUP"))?;
            let py_result = format_xreadgroup_result_with_py(py, results)?;
            return resolved(py, py_result);
        }

        // Blocking path: use future_into_py with Tokio for sleep/select.
        //
        // Register interest on stream_notify BEFORE the first poll so a
        // notification fired between the first read and the select! await
        // is not lost (Notify::notified registers a permit; notify_waiters
        // called after notified() has been awaited will wake it).
        let store = self.store.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let notify = store.stream_notify();
            let mut waiter = Box::pin(notify.notified());
            // Enable the waiter (arms its permit) so a notify that fires
            // before we reach the select! is remembered.
            waiter.as_mut().enable();

            // First non-blocking attempt
            let results = store
                .xreadgroup(&group, &consumer, &keys, &id_strs, count)
                .map_err(|e| store_err_to_py_for_cmd(e, "XREADGROUP"))?;

            if !results.is_empty() {
                return format_xreadgroup_result(results);
            }

            // Blocking: wait for stream notification or timeout, retrying until
            // data is available or the deadline expires. A single notify may fire
            // because a different stream received data, or another consumer claimed
            // the entry first, so we loop until we get results or time out.
            let block_ms = block.unwrap();
            let timeout_duration = Duration::from_millis(block_ms);
            let deadline = tokio::time::Instant::now() + timeout_duration;

            loop {
                // Graceful shutdown: return empty immediately so the Rust future
                // completes while the Python event loop is still alive.
                if store.is_shutdown() {
                    break format_xreadgroup_result(Vec::new());
                }

                let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                if remaining.is_zero() {
                    break format_xreadgroup_result(Vec::new());
                }
                tokio::select! {
                    _ = waiter.as_mut() => {
                        // Re-arm the waiter for the next iteration so we don't
                        // miss a notification that fires while we're re-polling.
                        waiter.set(notify.notified());
                        waiter.as_mut().enable();
                        // New data arrived, retry read
                        let results = store
                            .xreadgroup(&group, &consumer, &keys, &id_strs, count)
                            .map_err(|e| store_err_to_py_for_cmd(e, "XREADGROUP"))?;
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

        let count = self.store
            .xack(&key, &group, &stream_ids)
            .map_err(|e| store_err_to_py_for_cmd(e, "XACK"))?;
        resolved(py, count.into_pyobject(py)?.into_any().unbind())
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

        let (next_id, claimed, deleted) = self.store
            .xautoclaim(&key, &group, consumer_bytes, min_idle_time, start, count)
            .map_err(|e| store_err_to_py_for_cmd(e, "XAUTOCLAIM"))?;

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
        resolved(py, result.into_any().unbind())
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

        let results = self.store
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
            .map_err(|e| store_err_to_py_for_cmd(e, "XCLAIM"))?;

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
        resolved(py, outer.into_any().unbind())
    }

    /// XINFO STREAM command matching redis.asyncio.Redis.xinfo_stream() signature.
    /// Returns a dict with stream metadata using str keys (matches xinfo_groups
    /// convention). Keys: length, radix-tree-keys, radix-tree-nodes,
    /// last-generated-id (bytes), groups (int), first-entry, last-entry.
    /// Missing key raises ResponseError("ERR no such key ..."); wrong type
    /// raises WRONGTYPE.
    fn xinfo_stream<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let key = extract_bytes(name)?;
        let snapshot = match self.store.xinfo_stream(&key).map_err(store_err_to_py)? {
            Some(s) => s,
            None => {
                return Err(make_response_error(format!(
                    "ERR no such key '{}'",
                    String::from_utf8_lossy(&key)
                )));
            }
        };
        let dict = build_xinfo_stream_dict(py, &snapshot)?;
        resolved(py, dict.into_any().unbind())
    }

    /// XINFO GROUPS command matching redis.asyncio.Redis.xinfo_groups() signature.
    /// Returns list of dicts with group metadata.
    fn xinfo_groups<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let key = extract_bytes(name)?;
        let groups = self.store
            .xinfo_groups(&key)
            .map_err(|e| store_err_to_py_for_cmd(e, "XINFO GROUPS"))?;

        let result_list = pyo3::types::PyList::empty(py);
        for group_info in &groups {
            let dict = pyo3::types::PyDict::new(py);
            // name -> bytes
            if let Some(name_val) = group_info.get("name") {
                dict.set_item(
                    pyo3::types::PyString::new(py, "name"),
                    pyo3::types::PyBytes::new(py, name_val.as_bytes()),
                )?;
            }
            // consumers -> int
            if let Some(consumers_val) = group_info.get("consumers") {
                let count: i64 = consumers_val.parse().unwrap_or(0);
                dict.set_item(
                    pyo3::types::PyString::new(py, "consumers"),
                    count,
                )?;
            }
            // pending -> int
            if let Some(pending_val) = group_info.get("pending") {
                let count: i64 = pending_val.parse().unwrap_or(0);
                dict.set_item(
                    pyo3::types::PyString::new(py, "pending"),
                    count,
                )?;
            }
            // last-delivered-id -> bytes
            if let Some(id_val) = group_info.get("last-delivered-id") {
                dict.set_item(
                    pyo3::types::PyString::new(py, "last-delivered-id"),
                    pyo3::types::PyBytes::new(py, id_val.as_bytes()),
                )?;
            }
            result_list.append(dict)?;
        }
        resolved(py, result_list.into_any().unbind())
    }

    /// XINFO CONSUMERS command matching redis.asyncio.Redis.xinfo_consumers() signature.
    /// Returns list of dicts with consumer metadata for a specific group.
    fn xinfo_consumers<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
        groupname: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let key = extract_bytes(name)?;
        let group = extract_bytes(groupname)?;
        let consumers = self.store
            .xinfo_consumers(&key, &group)
            .map_err(|e| store_err_to_py_for_cmd(e, "XINFO CONSUMERS"))?;

        let result_list = pyo3::types::PyList::empty(py);
        for consumer_info in &consumers {
            let dict = pyo3::types::PyDict::new(py);
            // name -> bytes
            if let Some(name_val) = consumer_info.get("name") {
                dict.set_item(
                    pyo3::types::PyString::new(py, "name"),
                    pyo3::types::PyBytes::new(py, name_val.as_bytes()),
                )?;
            }
            // pending -> int
            if let Some(pending_val) = consumer_info.get("pending") {
                let count: i64 = pending_val.parse().unwrap_or(0);
                dict.set_item(
                    pyo3::types::PyString::new(py, "pending"),
                    count,
                )?;
            }
            // idle -> int
            if let Some(idle_val) = consumer_info.get("idle") {
                let idle: i64 = idle_val.parse().unwrap_or(0);
                dict.set_item(
                    pyo3::types::PyString::new(py, "idle"),
                    idle,
                )?;
            }
            result_list.append(dict)?;
        }
        resolved(py, result_list.into_any().unbind())
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

        let entries = self.store
            .xpending_range(
                &key,
                &group,
                min_id,
                max_id,
                count,
                consumer_filter.as_ref(),
                idle,
            )
            .map_err(|e| store_err_to_py_for_cmd(e, "XPENDING"))?;

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
        resolved(py, result_list.into_any().unbind())
    }

    // -- Scripting Commands ----

    /// EVAL command matching redis.asyncio.Redis.eval() signature.
    /// Executes a Lua script with KEYS and ARGV arrays.
    #[pyo3(signature = (script, numkeys, *keys_and_args))]
    fn eval<'py>(
        &self,
        py: Python<'py>,
        script: String,
        numkeys: usize,
        keys_and_args: &Bound<'py, pyo3::types::PyTuple>,
    ) -> PyResult<Bound<'py, PyAny>> {
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

        let result = self.store.eval(&script, keys, args);
        match result {
            Ok(val) => {
                let py_val = redis_value_to_py(py, val)?;
                resolved(py, py_val)
            }
            Err(msg) => Err(make_response_error(msg)),
        }
    }

    /// EVALSHA command matching redis.asyncio.Redis.evalsha() signature.
    /// Executes a cached Lua script by its SHA1 hash.
    #[pyo3(signature = (sha, numkeys, *keys_and_args))]
    fn evalsha<'py>(
        &self,
        py: Python<'py>,
        sha: String,
        numkeys: usize,
        keys_and_args: &Bound<'py, pyo3::types::PyTuple>,
    ) -> PyResult<Bound<'py, PyAny>> {
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

        let result = self.store.evalsha(&sha, keys, args);
        match result {
            Ok(val) => {
                let py_val = redis_value_to_py(py, val)?;
                resolved(py, py_val)
            }
            Err(msg) => Err(make_response_error(msg)),
        }
    }

    /// SCRIPT LOAD command - caches a Lua script and returns its SHA1 hash.
    fn script_load<'py>(
        &self,
        py: Python<'py>,
        script: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let sha = self.store.script_load(&script);
        resolved(py, sha.into_pyobject(py)?.into_any().unbind())
    }

    /// SCRIPT EXISTS command - checks if one or more scripts are cached by SHA1 hash.
    /// Returns list of bools.
    #[pyo3(signature = (*args))]
    fn script_exists<'py>(
        &self,
        py: Python<'py>,
        args: &Bound<'py, pyo3::types::PyTuple>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let shas: Vec<String> = args
            .iter()
            .map(|obj| obj.extract::<String>())
            .collect::<PyResult<Vec<_>>>()?;
        let results = self.store.script_exists(&shas);
        resolved(py, results.into_pyobject(py)?.into_any().unbind())
    }

    // -- Pub/Sub Commands --

    /// PUBLISH: Send a message to a channel. Returns number of subscribers that received it.
    fn publish<'py>(&self, py: Python<'py>, channel: &Bound<'py, PyAny>, message: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
        let channel_bytes = extract_bytes(channel)?;
        let message_bytes = extract_bytes(message)?;
        let count = self.store.publish(
            Bytes::from(channel_bytes),
            Bytes::from(message_bytes),
        );
        resolved(py, count.into_pyobject(py)?.into_any().unbind())
    }

    /// Graceful shutdown: signal all blocking futures to complete so they
    /// finish via call_soon_threadsafe while the Python event loop is alive.
    /// Also stops all pubsub listener tasks.
    /// Python side exposes this as `aclose()` / `close()`.
    fn _aclose<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        self.store.shutdown();
        resolved(py, py.None())
    }

    /// Alias for _aclose (sync-style name).
    fn _close<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        self._aclose(py)
    }

    /// Internal: Create a new subscriber ID. Used by Python PubSub class.
    fn _new_subscriber(&self) -> PyResult<u64> {
        let (id, _rx) = self.store.new_subscriber();
        Ok(id)
    }

    /// Internal: return the last stream ID for a given key as a bytes string,
    /// or None if the key doesn't exist or isn't a stream.
    /// Used by Python-side blocking xread/xreadgroup to resolve "$" once.
    fn _stream_last_id<'py>(
        &self,
        py: Python<'py>,
        key: &Bound<'py, PyAny>,
    ) -> PyResult<Option<Py<PyAny>>> {
        let key_bytes = extract_bytes(key)?;
        match self.store.stream_last_id(&key_bytes) {
            Some(id) => {
                let id_str = format_stream_id(id);
                Ok(Some(
                    PyBytes::new(py, id_str.as_bytes()).into_any().unbind(),
                ))
            }
            None => Ok(None),
        }
    }

    /// Internal: Subscribe and start a background task that filters messages
    /// into a Python asyncio.Queue. Returns subscriber_id.
    /// Called once by Python PubSub on first subscribe.
    ///
    /// NOTE: This remains async (future_into_py) because it spawns a long-running
    /// background Tokio task that blocks on a broadcast channel. The task is
    /// kept bound to the lifetime of its owning Python PubSub object via a
    /// oneshot::Sender stored in `store.pubsub.listener_stoppers` — when the
    /// Python side calls `_stop_subscriber_listener`, the oneshot receiver
    /// resolves and the listener's `tokio::select!` breaks out of its loop.
    ///
    /// WHY a dedicated stopper (not just broadcast::RecvError::Closed): the
    /// global broadcast sender lives on the shared `Store` and is only
    /// dropped when `BurnerRedis` itself goes away. Under pytest-asyncio on
    /// Python 3.11+, each test tears down and re-creates its event loop.
    /// The captured `queue` PyObject in this listener would otherwise point
    /// at the PRIOR test's asyncio Queue. Stopping the listener from
    /// `PubSub.aclose()` removes that foot-gun entirely.
    ///
    /// **Known issue:** Message delivery currently uses
    /// `event_loop.call_soon_threadsafe(queue.put_nowait, msg)`. This
    /// writes to the ProactorEventLoop's IOCP self-pipe on Windows, and
    /// a racy call_soon_threadsafe against a closing loop can corrupt
    /// IOCP state (cpython#116773), causing a flaky hang (~10-20% of
    /// the time) in GetQueuedCompletionStatus. The stop_flag check
    /// before each call_soon_threadsafe narrows the window but does not
    /// eliminate it entirely. A future fix should either:
    ///   (a) await the listener's actual exit in _stop_subscriber_listener
    ///       so no call_soon_threadsafe fires after aclose() returns, or
    ///   (b) replace call_soon_threadsafe with a thread-safe channel
    ///       (e.g. janus queue) that doesn't touch the event loop pipe.
    fn _subscribe_listener<'py>(&self, py: Python<'py>, subscriber_id: u64, queue: Py<PyAny>) -> PyResult<Bound<'py, PyAny>> {
        let store = self.store.clone();
        // Capture the running asyncio event loop for call_soon_threadsafe.
        let asyncio = py.import("asyncio")?;
        let event_loop: Py<PyAny> = asyncio
            .getattr("get_running_loop")?
            .call0()?
            .into();

        // Atomic flag set by aclose() to prevent the listener from calling
        // call_soon_threadsafe after the PubSub is being torn down. The
        // listener checks this flag under GIL BEFORE every
        // call_soon_threadsafe invocation.
        let stop_flag = Arc::new(AtomicBool::new(false));

        // Oneshot used by `tokio::select!` to break out of rx.recv()
        // immediately on stop.
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();

        // Clone stop_flag for the spawned task; the original goes to the
        // stopper handle so aclose() can set it.
        let task_stop_flag = stop_flag.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut rx = {
                let registry = store.pubsub.read();
                registry.tx.subscribe()
            };

            // Spawn a background Tokio task that forwards matching messages
            // to the Python queue via call_soon_threadsafe.
            let join_handle = tokio::spawn(async move {
                tokio::pin!(stop_rx);
                loop {
                    tokio::select! {
                        biased;

                        _ = &mut stop_rx => {
                            break;
                        }

                        res = rx.recv() => {
                            match res {
                                Ok(msg) => {
                                    // Check stop flag BEFORE touching Python.
                                    if task_stop_flag.load(Ordering::Acquire) {
                                        break;
                                    }
                                    let delivered = Python::try_attach(|py| -> Result<(), PyErr> {
                                        // Re-check under GIL: aclose may
                                        // have set the flag while we were
                                        // waiting for GIL.
                                        if task_stop_flag.load(Ordering::Acquire) {
                                            return Ok(());
                                        }
                                        let dict = PyDict::new(py);
                                        dict.set_item("type", &msg.kind)?;
                                        match &msg.pattern {
                                            Some(p) => dict.set_item("pattern", PyBytes::new(py, p))?,
                                            None => dict.set_item("pattern", py.None())?,
                                        };
                                        dict.set_item("channel", PyBytes::new(py, &msg.channel))?;
                                        dict.set_item("data", PyBytes::new(py, &msg.data))?;
                                        let put_nowait = queue.getattr(py, "put_nowait")?;
                                        event_loop
                                            .getattr(py, "call_soon_threadsafe")?
                                            .call1(py, (put_nowait, dict))?;
                                        Ok(())
                                    });
                                    match delivered {
                                        Some(Ok(())) => {}
                                        Some(Err(_)) => {
                                            // Event loop is closed or other
                                            // error. Stop the listener.
                                            break;
                                        }
                                        None => {
                                            // Python interpreter shutting down.
                                            break;
                                        }
                                    }
                                }
                                Err(broadcast::error::RecvError::Lagged(n)) => {
                                    eprintln!("burner-redis pubsub: subscriber lagged, {} messages dropped", n);
                                    continue;
                                }
                                Err(broadcast::error::RecvError::Closed) => {
                                    break;
                                }
                            }
                        }
                    }
                }
            });

            // Register the stopper handle AFTER spawn so we have the
            // JoinHandle. aclose() will set the stop_flag, fire stop_tx,
            // then await join_handle to guarantee the listener has fully
            // exited before returning.
            store.register_listener_stopper(
                subscriber_id, stop_tx, stop_flag, join_handle,
            );

            Ok(subscriber_id)
        })
    }

    /// Internal: Signal the background listener task for `subscriber_id` to
    /// stop and remove the subscriber's channel/pattern registrations from
    /// the pub/sub registry.
    ///
    /// This is SYNCHRONOUS (resolved()) — it sets the stop_flag + fires
    /// stop_tx and returns immediately. The listener task will exit on its
    /// next loop iteration; the stop_flag prevents it from calling
    /// call_soon_threadsafe after this point.
    ///
    /// Previously this was async (future_into_py) and awaited the listener's
    /// JoinHandle to guarantee zero in-flight call_soon_threadsafe calls.
    /// But the future_into_py result delivery itself uses call_soon_threadsafe,
    /// creating the exact race it was trying to prevent (cpython#116773).
    /// Making this synchronous removes that call_soon_threadsafe entirely.
    /// The stop_flag guard in the listener loop is sufficient: after
    /// stop_flag is set, the listener will not issue any new
    /// call_soon_threadsafe calls (it checks before every delivery).
    fn _stop_subscriber_listener<'py>(&self, py: Python<'py>, subscriber_id: u64) -> PyResult<Bound<'py, PyAny>> {
        // stop_subscriber_listener sets stop_flag, fires stop_tx, removes
        // from registry — all synchronously. The returned JoinHandle is
        // intentionally NOT awaited to avoid a future_into_py round-trip.
        let _join_handle = self.store.stop_subscriber_listener(subscriber_id);
        resolved(py, py.None())
    }

    /// SUBSCRIBE: Register channels for a subscriber.
    fn subscribe_channels<'py>(&self, py: Python<'py>, subscriber_id: u64, channels: Vec<Vec<u8>>) -> PyResult<Bound<'py, PyAny>> {
        let channel_bytes: Vec<Bytes> = channels.into_iter().map(Bytes::from).collect();
        let results = self.store.subscribe(subscriber_id, channel_bytes);
        let tuples: Vec<(Vec<u8>, i64)> = results.into_iter()
            .map(|(ch, count)| (ch.to_vec(), count))
            .collect();
        resolved(py, tuples.into_pyobject(py)?.into_any().unbind())
    }

    /// UNSUBSCRIBE: Remove channels from a subscriber.
    fn unsubscribe_channels<'py>(&self, py: Python<'py>, subscriber_id: u64, channels: Vec<Vec<u8>>) -> PyResult<Bound<'py, PyAny>> {
        let channel_bytes: Vec<Bytes> = channels.into_iter().map(Bytes::from).collect();
        let results = self.store.unsubscribe(subscriber_id, channel_bytes);
        let tuples: Vec<(Vec<u8>, i64)> = results.into_iter()
            .map(|(ch, count)| (ch.to_vec(), count))
            .collect();
        resolved(py, tuples.into_pyobject(py)?.into_any().unbind())
    }

    /// PSUBSCRIBE: Register glob patterns for a subscriber.
    fn psubscribe_patterns<'py>(&self, py: Python<'py>, subscriber_id: u64, patterns: Vec<Vec<u8>>) -> PyResult<Bound<'py, PyAny>> {
        let pattern_bytes: Vec<Bytes> = patterns.into_iter().map(Bytes::from).collect();
        let results = self.store.psubscribe(subscriber_id, pattern_bytes);
        let tuples: Vec<(Vec<u8>, i64)> = results.into_iter()
            .map(|(pat, count)| (pat.to_vec(), count))
            .collect();
        resolved(py, tuples.into_pyobject(py)?.into_any().unbind())
    }

    /// PUNSUBSCRIBE: Remove glob patterns from a subscriber.
    fn punsubscribe_patterns<'py>(&self, py: Python<'py>, subscriber_id: u64, patterns: Vec<Vec<u8>>) -> PyResult<Bound<'py, PyAny>> {
        let pattern_bytes: Vec<Bytes> = patterns.into_iter().map(Bytes::from).collect();
        let results = self.store.punsubscribe(subscriber_id, pattern_bytes);
        let tuples: Vec<(Vec<u8>, i64)> = results.into_iter()
            .map(|(pat, count)| (pat.to_vec(), count))
            .collect();
        resolved(py, tuples.into_pyobject(py)?.into_any().unbind())
    }

    /// PUBSUB CHANNELS: Return active channels matching optional glob pattern.
    #[pyo3(signature = (pattern=None))]
    fn pubsub_channels<'py>(&self, py: Python<'py>, pattern: Option<Vec<u8>>) -> PyResult<Bound<'py, PyAny>> {
        let pat_bytes = pattern.map(Bytes::from);
        let channels = self.store.pubsub_channels(pat_bytes.as_ref());
        let result: Vec<Vec<u8>> = channels.into_iter().map(|ch| ch.to_vec()).collect();
        resolved(py, result.into_pyobject(py)?.into_any().unbind())
    }

    /// PUBSUB NUMSUB: Return (channel, count) for requested channels.
    fn pubsub_numsub<'py>(&self, py: Python<'py>, channels: Vec<Vec<u8>>) -> PyResult<Bound<'py, PyAny>> {
        let channel_bytes: Vec<Bytes> = channels.into_iter().map(Bytes::from).collect();
        let results = self.store.pubsub_numsub(channel_bytes);
        let tuples: Vec<(Vec<u8>, i64)> = results.into_iter()
            .map(|(ch, count)| (ch.to_vec(), count))
            .collect();
        resolved(py, tuples.into_pyobject(py)?.into_any().unbind())
    }

    /// PUBSUB NUMPAT: Return total number of active pattern subscriptions.
    fn pubsub_numpat<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let count = self.store.pubsub_numpat();
        resolved(py, count.into_pyobject(py)?.into_any().unbind())
    }

    // -- Key Enumeration & Multi-Key Commands ----

    /// KEYS command matching redis.asyncio.Redis.keys() signature.
    /// Returns list of keys matching the glob pattern.
    #[pyo3(signature = (pattern="*"))]
    fn keys<'py>(
        &self,
        py: Python<'py>,
        pattern: &str,
    ) -> PyResult<Bound<'py, PyAny>> {
        let pat = Bytes::from(pattern.to_owned().into_bytes());
        let keys = self.store.keys(pat.as_ref());
        let result: Vec<Vec<u8>> = keys.into_iter().map(|k| k.to_vec()).collect();
        resolved(py, result.into_pyobject(py)?.into_any().unbind())
    }

    /// TTL command matching redis.asyncio.Redis.ttl() signature.
    /// Returns seconds remaining, -1 for no TTL, -2 for missing key.
    fn ttl<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let key = extract_bytes(name)?;
        let ttl_val = self.store.ttl(&key);
        resolved(py, ttl_val.into_pyobject(py)?.into_any().unbind())
    }

    /// MGET command matching redis.asyncio.Redis.mget() signature.
    /// Returns list of values (or None) for each key.
    #[pyo3(signature = (*keys))]
    fn mget<'py>(
        &self,
        py: Python<'py>,
        keys: &Bound<'py, PyTuple>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let key_list: Vec<Bytes> = keys
            .iter()
            .map(|k| extract_bytes(&k))
            .collect::<PyResult<Vec<Bytes>>>()?;
        let results = self.store.mget(&key_list);
        let py_results: Vec<Option<Vec<u8>>> = results
            .into_iter()
            .map(|opt| opt.map(|b| b.to_vec()))
            .collect();
        resolved(py, py_results.into_pyobject(py)?.into_any().unbind())
    }

    /// XPENDING summary command matching redis.asyncio.Redis.xpending() signature.
    /// Returns dict with pending count, min/max IDs, and per-consumer counts.
    fn xpending<'py>(
        &self,
        py: Python<'py>,
        name: &Bound<'py, PyAny>,
        groupname: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let key = extract_bytes(name)?;
        let group = extract_bytes(groupname)?;
        let (total, min_id, max_id, consumers) = self.store
            .xpending_summary(&key, &group)
            .map_err(|e| store_err_to_py_for_cmd(e, "XPENDING"))?;

        let dict = PyDict::new(py);
        dict.set_item("pending", total)?;
        match min_id {
            Some(id) => dict.set_item("min", format_stream_id(id))?,
            None => dict.set_item("min", py.None())?,
        }
        match max_id {
            Some(id) => dict.set_item("max", format_stream_id(id))?,
            None => dict.set_item("max", py.None())?,
        }
        let consumer_list = pyo3::types::PyList::empty(py);
        for (cname, count) in consumers {
            let cdict = PyDict::new(py);
            cdict.set_item("name", PyBytes::new(py, &cname))?;
            cdict.set_item("pending", count)?;
            consumer_list.append(cdict)?;
        }
        dict.set_item("consumers", consumer_list)?;
        resolved(py, dict.into_any().unbind())
    }

    /// Execute a batch of pipeline commands in a single Rust call.
    /// Accepts a list of (method_name, args_tuple, kwargs_dict) tuples.
    /// Returns a list of results (with errors as exception objects at the failed position).
    fn execute_pipeline<'py>(&self, py: Python<'py>, commands: &Bound<'py, PyList>) -> PyResult<Bound<'py, PyAny>> {
        let results = pyo3::types::PyList::empty(py);
        for item in commands.iter() {
            let tuple = item.downcast::<PyTuple>()?;
            let method_name: String = tuple.get_item(0)?.extract()?;
            let args = tuple.get_item(1)?.downcast::<PyTuple>()?.clone();
            let kwargs = tuple.get_item(2)?.downcast::<PyDict>()?.clone();

            let result = self.dispatch_pipeline_command(py, &method_name, &args, &kwargs);
            match result {
                Ok(val) => results.append(val)?,
                Err(e) => results.append(e.value(py))?,
            }
        }
        resolved(py, results.into_any().unbind())
    }
}

impl BurnerRedis {
    /// Dispatch a single pipeline command by name, executing the store operation
    /// directly and returning a PyObject. This avoids ResolvedFuture wrapping overhead.
    fn dispatch_pipeline_command<'py>(
        &self,
        py: Python<'py>,
        method: &str,
        args: &Bound<'py, PyTuple>,
        kwargs: &Bound<'py, PyDict>,
    ) -> PyResult<Py<PyAny>> {
        match method {
            "set" => {
                let name = &args.get_item(0)?;
                let value = &args.get_item(1)?;
                let ex: Option<Bound<'py, PyAny>> = kwargs.get_item("ex")?.and_then(|v| if v.is_none() { None } else { Some(v) });
                let px: Option<Bound<'py, PyAny>> = kwargs.get_item("px")?.and_then(|v| if v.is_none() { None } else { Some(v) });
                let nx: bool = kwargs.get_item("nx")?.map(|v| v.extract()).transpose()?.unwrap_or(false);
                let xx: bool = kwargs.get_item("xx")?.map(|v| v.extract()).transpose()?.unwrap_or(false);
                let key = extract_bytes(name)?;
                let val = extract_bytes(value)?;
                let ttl = if let Some(ref px_val) = px {
                    Some(extract_expiry(px_val, true)?)
                } else if let Some(ref ex_val) = ex {
                    Some(extract_expiry(ex_val, false)?)
                } else {
                    None
                };
                let success = self.store.set(key, val, ttl, nx, xx);
                if success {
                    Ok(pyo3::types::PyBool::new(py, true).to_owned().into_any().unbind())
                } else {
                    Ok(py.None())
                }
            }
            "get" => {
                let name = &args.get_item(0)?;
                let key = extract_bytes(name)?;
                match self.store.get(&key) {
                    Some(b) => Ok(PyBytes::new(py, &b).into_any().unbind()),
                    None => Ok(py.None()),
                }
            }
            "delete" => {
                let keys: Vec<Bytes> = args.iter().map(|obj| extract_bytes(&obj)).collect::<PyResult<Vec<_>>>()?;
                let count = self.store.delete(&keys);
                Ok(count.into_pyobject(py)?.into_any().unbind())
            }
            "exists" => {
                let keys: Vec<Bytes> = args.iter().map(|obj| extract_bytes(&obj)).collect::<PyResult<Vec<_>>>()?;
                let count = self.store.exists(&keys);
                Ok(count.into_pyobject(py)?.into_any().unbind())
            }
            "hset" => {
                let name = &args.get_item(0)?;
                let name_bytes = extract_bytes(name)?;
                let key_opt: Option<Bound<'py, PyAny>> = kwargs.get_item("key")?.and_then(|v| if v.is_none() { None } else { Some(v) });
                let value_opt: Option<Bound<'py, PyAny>> = kwargs.get_item("value")?.and_then(|v| if v.is_none() { None } else { Some(v) });
                let mapping: Option<Bound<'py, PyDict>> = kwargs.get_item("mapping")?.and_then(|v| if v.is_none() { None } else { v.downcast::<PyDict>().ok().map(|d| d.clone()) });
                let mut fields: Vec<(Bytes, Bytes)> = Vec::new();
                if let (Some(k), Some(v)) = (key_opt.as_ref(), value_opt.as_ref()) {
                    fields.push((extract_bytes(k)?, extract_bytes(v)?));
                }
                if let Some(ref dict) = mapping {
                    for (k, v) in dict.iter() {
                        fields.push((extract_bytes(&k)?, extract_bytes(&v)?));
                    }
                }
                let count = self.store.hset(name_bytes, fields).map_err(store_err_to_py)?;
                Ok(count.into_pyobject(py)?.into_any().unbind())
            }
            "hget" => {
                let name = &args.get_item(0)?;
                let key = &args.get_item(1)?;
                let name_bytes = extract_bytes(name)?;
                let field_bytes = extract_bytes(key)?;
                let result = self.store.hget(&name_bytes, &field_bytes).map_err(store_err_to_py)?;
                match result {
                    Some(b) => Ok(PyBytes::new(py, &b).into_any().unbind()),
                    None => Ok(py.None()),
                }
            }
            "hdel" => {
                let name = &args.get_item(0)?;
                let name_bytes = extract_bytes(name)?;
                let fields: Vec<Bytes> = args.iter().skip(1).map(|obj| extract_bytes(&obj)).collect::<PyResult<Vec<_>>>()?;
                let count = self.store.hdel(&name_bytes, &fields).map_err(store_err_to_py)?;
                Ok(count.into_pyobject(py)?.into_any().unbind())
            }
            "hvals" => {
                let name = &args.get_item(0)?;
                let name_bytes = extract_bytes(name)?;
                let vals = self.store.hvals(&name_bytes).map_err(store_err_to_py)?;
                let py_list: Vec<Vec<u8>> = vals.into_iter().map(|b| b.to_vec()).collect();
                Ok(py_list.into_pyobject(py)?.into_any().unbind())
            }
            "hgetall" => {
                let name = &args.get_item(0)?;
                let name_bytes = extract_bytes(name)?;
                let map = self.store.hgetall(&name_bytes).map_err(store_err_to_py)?;
                let dict = PyDict::new(py);
                for (k, v) in &map {
                    dict.set_item(PyBytes::new(py, k.as_ref()), PyBytes::new(py, v.as_ref()))?;
                }
                Ok(dict.into_any().unbind())
            }
            "hexists" => {
                let name = &args.get_item(0)?;
                let key = &args.get_item(1)?;
                let name_bytes = extract_bytes(name)?;
                let field_bytes = extract_bytes(key)?;
                let exists = self.store.hexists(&name_bytes, &field_bytes).map_err(store_err_to_py)?;
                Ok(pyo3::types::PyBool::new(py, exists).to_owned().into_any().unbind())
            }
            "hincrby" => {
                let name = &args.get_item(0)?;
                let key = &args.get_item(1)?;
                let name_bytes = extract_bytes(name)?;
                let field_bytes = extract_bytes(key)?;
                let amount: i64 = kwargs.get_item("amount")?.map(|v| v.extract()).transpose()?.unwrap_or(1);
                let new_val = self.store.hincrby(name_bytes, field_bytes, amount).map_err(store_err_to_py)?;
                Ok(new_val.into_pyobject(py)?.into_any().unbind())
            }
            "sadd" => {
                let name = &args.get_item(0)?;
                let name_bytes = extract_bytes(name)?;
                let members: Vec<Bytes> = args.iter().skip(1).map(|obj| extract_bytes(&obj)).collect::<PyResult<Vec<_>>>()?;
                let count = self.store.sadd(name_bytes, members).map_err(store_err_to_py)?;
                Ok(count.into_pyobject(py)?.into_any().unbind())
            }
            "smembers" => {
                let name = &args.get_item(0)?;
                let name_bytes = extract_bytes(name)?;
                let members = self.store.smembers(&name_bytes).map_err(store_err_to_py)?;
                let set: StdHashSet<Vec<u8>> = members.into_iter().map(|b| b.to_vec()).collect();
                Ok(set.into_pyobject(py)?.into_any().unbind())
            }
            "sismember" => {
                let name = &args.get_item(0)?;
                let value = &args.get_item(1)?;
                let name_bytes = extract_bytes(name)?;
                let member_bytes = extract_bytes(value)?;
                let is_member = self.store.sismember(&name_bytes, &member_bytes).map_err(store_err_to_py)?;
                Ok(pyo3::types::PyBool::new(py, is_member).to_owned().into_any().unbind())
            }
            "srem" => {
                let name = &args.get_item(0)?;
                let name_bytes = extract_bytes(name)?;
                let members: Vec<Bytes> = args.iter().skip(1).map(|obj| extract_bytes(&obj)).collect::<PyResult<Vec<_>>>()?;
                let count = self.store.srem(&name_bytes, &members).map_err(store_err_to_py)?;
                Ok(count.into_pyobject(py)?.into_any().unbind())
            }
            "zadd" => {
                let name = &args.get_item(0)?;
                let mapping = args.get_item(1)?.downcast::<PyDict>()?.clone();
                let name_bytes = extract_bytes(name)?;
                let nx: bool = kwargs.get_item("nx")?.map(|v| v.extract()).transpose()?.unwrap_or(false);
                let xx: bool = kwargs.get_item("xx")?.map(|v| v.extract()).transpose()?.unwrap_or(false);
                let gt: bool = kwargs.get_item("gt")?.map(|v| v.extract()).transpose()?.unwrap_or(false);
                let lt: bool = kwargs.get_item("lt")?.map(|v| v.extract()).transpose()?.unwrap_or(false);
                let ch: bool = kwargs.get_item("ch")?.map(|v| v.extract()).transpose()?.unwrap_or(false);
                let mut members: Vec<(f64, Bytes)> = Vec::new();
                for (k, v) in mapping.iter() {
                    let member = extract_bytes(&k)?;
                    let score: f64 = v.extract::<f64>()?;
                    members.push((score, member));
                }
                let count = self.store.zadd(name_bytes, members, nx, xx, gt, lt, ch).map_err(store_err_to_py)?;
                Ok(count.into_pyobject(py)?.into_any().unbind())
            }
            "zrem" => {
                let name = &args.get_item(0)?;
                let name_bytes = extract_bytes(name)?;
                let members: Vec<Bytes> = args.iter().skip(1).map(|obj| extract_bytes(&obj)).collect::<PyResult<Vec<_>>>()?;
                let count = self.store.zrem(&name_bytes, &members).map_err(store_err_to_py)?;
                Ok(count.into_pyobject(py)?.into_any().unbind())
            }
            "zrange" => {
                let name = &args.get_item(0)?;
                let start: i64 = args.get_item(1)?.extract()?;
                let end: i64 = args.get_item(2)?.extract()?;
                let name_bytes = extract_bytes(name)?;
                let withscores: bool = kwargs.get_item("withscores")?.map(|v| v.extract()).transpose()?.unwrap_or(false);
                let results = self.store.zrange(&name_bytes, start, end, withscores).map_err(store_err_to_py)?;
                if withscores {
                    let list: Vec<(Vec<u8>, f64)> = results.into_iter().map(|(m, s)| (m.to_vec(), s.unwrap_or(0.0))).collect();
                    Ok(list.into_pyobject(py)?.into_any().unbind())
                } else {
                    let list: Vec<Vec<u8>> = results.into_iter().map(|(m, _)| m.to_vec()).collect();
                    Ok(list.into_pyobject(py)?.into_any().unbind())
                }
            }
            "zrangebyscore" => {
                let name = &args.get_item(0)?;
                let min = &args.get_item(1)?;
                let max = &args.get_item(2)?;
                let name_bytes = extract_bytes(name)?;
                let min_f64 = parse_score_bound(min)?;
                let max_f64 = parse_score_bound(max)?;
                let withscores: bool = kwargs.get_item("withscores")?.map(|v| v.extract()).transpose()?.unwrap_or(false);
                let results = self.store.zrangebyscore(&name_bytes, min_f64, max_f64, withscores).map_err(store_err_to_py)?;
                if withscores {
                    let list: Vec<(Vec<u8>, f64)> = results.into_iter().map(|(m, s)| (m.to_vec(), s.unwrap_or(0.0))).collect();
                    Ok(list.into_pyobject(py)?.into_any().unbind())
                } else {
                    let list: Vec<Vec<u8>> = results.into_iter().map(|(m, _)| m.to_vec()).collect();
                    Ok(list.into_pyobject(py)?.into_any().unbind())
                }
            }
            "zrangestore" => {
                let dest = &args.get_item(0)?;
                let name = &args.get_item(1)?;
                let start = &args.get_item(2)?;
                let end = &args.get_item(3)?;
                let dst_bytes = extract_bytes(dest)?;
                let src_bytes = extract_bytes(name)?;
                let min_f64 = parse_score_bound(start)?;
                let max_f64 = parse_score_bound(end)?;
                let count = self.store.zrangestore(dst_bytes, &src_bytes, min_f64, max_f64).map_err(store_err_to_py)?;
                Ok(count.into_pyobject(py)?.into_any().unbind())
            }
            "zremrangebyscore" => {
                let name = &args.get_item(0)?;
                let min = &args.get_item(1)?;
                let max = &args.get_item(2)?;
                let name_bytes = extract_bytes(name)?;
                let min_f64 = parse_score_bound(min)?;
                let max_f64 = parse_score_bound(max)?;
                let count = self.store.zremrangebyscore(&name_bytes, min_f64, max_f64).map_err(store_err_to_py)?;
                Ok(count.into_pyobject(py)?.into_any().unbind())
            }
            "zcard" => {
                let name = &args.get_item(0)?;
                let name_bytes = extract_bytes(name)?;
                let count = self.store.zcard(&name_bytes).map_err(store_err_to_py)?;
                Ok(count.into_pyobject(py)?.into_any().unbind())
            }
            "zscore" => {
                let name = &args.get_item(0)?;
                let value = &args.get_item(1)?;
                let name_bytes = extract_bytes(name)?;
                let member_bytes = extract_bytes(value)?;
                let score = self.store.zscore(&name_bytes, &member_bytes).map_err(store_err_to_py)?;
                Ok(score.into_pyobject(py)?.into_any().unbind())
            }
            "zcount" => {
                let name = &args.get_item(0)?;
                let min = &args.get_item(1)?;
                let max = &args.get_item(2)?;
                let name_bytes = extract_bytes(name)?;
                let min_f64 = parse_score_bound(min)?;
                let max_f64 = parse_score_bound(max)?;
                let count = self.store.zcount(&name_bytes, min_f64, max_f64).map_err(store_err_to_py)?;
                Ok(count.into_pyobject(py)?.into_any().unbind())
            }
            "expire" => {
                let name = &args.get_item(0)?;
                let time = &args.get_item(1)?;
                let name_bytes = extract_bytes(name)?;
                let seconds: u64 = if let Ok(secs) = time.extract::<u64>() {
                    secs
                } else if let Ok(secs_f64) = time.call_method0("total_seconds")?.extract::<f64>() {
                    secs_f64.max(0.0) as u64
                } else {
                    return Err(pyo3::exceptions::PyTypeError::new_err("expire time must be int (seconds) or timedelta"));
                };
                let result = self.store.expire(&name_bytes, seconds);
                Ok(pyo3::types::PyBool::new(py, result).to_owned().into_any().unbind())
            }
            "xadd" => {
                let name = &args.get_item(0)?;
                let fields = args.get_item(1)?.downcast::<PyDict>()?.clone();
                let key = extract_bytes(name)?;
                let field_map = extract_stream_fields(&fields)?;
                let id_str: String = kwargs.get_item("id")?.map(|v| v.extract()).transpose()?.unwrap_or_else(|| "*".to_string());
                let id_opt: Option<StreamId> = if id_str == "*" {
                    None
                } else {
                    Some(parse_stream_id(&id_str).ok_or_else(|| {
                        pyo3::exceptions::PyValueError::new_err(format!("Invalid stream ID format: {}", id_str))
                    })?)
                };
                let stream_id = self.store.xadd(key, field_map, id_opt).map_err(store_err_to_py)?;
                let result = format_stream_id(stream_id);
                Ok(PyBytes::new(py, result.as_bytes()).into_any().unbind())
            }
            "xread" => {
                let streams_dict = args.get_item(0)?.downcast::<PyDict>()?.clone();
                let count: Option<usize> = kwargs.get_item("count")?.and_then(|v| if v.is_none() { None } else { v.extract().ok() });
                let mut keys: Vec<Bytes> = Vec::new();
                let mut ids: Vec<StreamId> = Vec::new();
                for (k, v) in streams_dict.iter() {
                    let key = extract_bytes(&k)?;
                    let id_str: String = v.extract::<String>().or_else(|_| v.extract::<Vec<u8>>().map(|b| String::from_utf8_lossy(&b).into_owned()))?;
                    let stream_id = if id_str == "0" || id_str == "0-0" {
                        (0u64, 0u64)
                    } else if id_str == "$" {
                        // Resolve '$' to the stream's current last_id at pipeline
                        // execution time; missing stream -> (0, 0).
                        self.store.stream_last_id(&key).unwrap_or((0, 0))
                    } else {
                        parse_stream_id(&id_str).ok_or_else(|| pyo3::exceptions::PyValueError::new_err(format!("Invalid stream ID: {}", id_str)))?
                    };
                    keys.push(key);
                    ids.push(stream_id);
                }
                let results = self.store.xread(&keys, &ids, count).map_err(store_err_to_py)?;
                if results.is_empty() {
                    return Ok(py.None());
                }
                let outer = pyo3::types::PyList::empty(py);
                for (stream_name, entries) in &results {
                    let entry_list = pyo3::types::PyList::empty(py);
                    for (id, fields) in entries {
                        let id_bytes = format_stream_id(*id).into_bytes();
                        let field_dict = PyDict::new(py);
                        for (fk, fv) in fields {
                            field_dict.set_item(PyBytes::new(py, fk.as_ref()), PyBytes::new(py, fv.as_ref()))?;
                        }
                        let tuple = PyTuple::new(py, &[PyBytes::new(py, &id_bytes).into_any(), field_dict.into_any()])?;
                        entry_list.append(tuple)?;
                    }
                    let stream_pair = pyo3::types::PyList::new(py, &[PyBytes::new(py, stream_name.as_ref()).into_any(), entry_list.into_any()])?;
                    outer.append(stream_pair)?;
                }
                Ok(outer.into_any().unbind())
            }
            "xlen" => {
                let name = &args.get_item(0)?;
                let key = extract_bytes(name)?;
                let len = self.store.xlen(&key).map_err(store_err_to_py)?;
                Ok((len as i64).into_pyobject(py)?.into_any().unbind())
            }
            "xtrim" => {
                let name = &args.get_item(0)?;
                let key = extract_bytes(name)?;
                let maxlen: Option<usize> = kwargs.get_item("maxlen")?.and_then(|v| if v.is_none() { None } else { v.extract().ok() });
                let minid: Option<String> = kwargs.get_item("minid")?.and_then(|v| if v.is_none() { None } else { v.extract().ok() });
                let minid_parsed: Option<StreamId> = match minid {
                    Some(ref s) => Some(parse_stream_id(s).ok_or_else(|| pyo3::exceptions::PyValueError::new_err(format!("Invalid stream ID for minid: {}", s)))?),
                    None => None,
                };
                let trimmed = self.store.xtrim(&key, maxlen, minid_parsed).map_err(store_err_to_py)?;
                Ok((trimmed as i64).into_pyobject(py)?.into_any().unbind())
            }
            "xdel" => {
                let name = &args.get_item(0)?;
                let key = extract_bytes(name)?;
                let mut stream_ids: Vec<StreamId> = Vec::new();
                for id_obj in args.iter().skip(1) {
                    let id_str: String = id_obj.extract::<String>().or_else(|_| id_obj.extract::<Vec<u8>>().map(|b| String::from_utf8_lossy(&b).into_owned()))?;
                    let stream_id = parse_stream_id(&id_str).ok_or_else(|| pyo3::exceptions::PyValueError::new_err(format!("Invalid stream ID format: {}", id_str)))?;
                    stream_ids.push(stream_id);
                }
                let count = self.store.xdel(&key, &stream_ids).map_err(store_err_to_py)?;
                Ok(count.into_pyobject(py)?.into_any().unbind())
            }
            "xrange" => {
                let name = &args.get_item(0)?;
                let key = extract_bytes(name)?;
                let min_str: String = kwargs.get_item("min")?.map(|v| v.extract()).transpose()?.unwrap_or_else(|| "-".to_string());
                let max_str: String = kwargs.get_item("max")?.map(|v| v.extract()).transpose()?.unwrap_or_else(|| "+".to_string());
                let count: Option<usize> = kwargs.get_item("count")?.and_then(|v| if v.is_none() { None } else { v.extract().ok() });
                let min_id: StreamId = if min_str == "-" { (0, 0) } else {
                    parse_stream_id(&min_str).ok_or_else(|| pyo3::exceptions::PyValueError::new_err(format!("Invalid stream ID format: {}", min_str)))?
                };
                let max_id: StreamId = if max_str == "+" { (u64::MAX, u64::MAX) } else {
                    parse_stream_id(&max_str).ok_or_else(|| pyo3::exceptions::PyValueError::new_err(format!("Invalid stream ID format: {}", max_str)))?
                };
                let entries = self.store.xrange(&key, min_id, max_id, count).map_err(store_err_to_py)?;
                let result_list = pyo3::types::PyList::empty(py);
                for (id, fields) in &entries {
                    let id_bytes = format_stream_id(*id).into_bytes();
                    let field_dict = PyDict::new(py);
                    for (fk, fv) in fields {
                        field_dict.set_item(PyBytes::new(py, fk.as_ref()), PyBytes::new(py, fv.as_ref()))?;
                    }
                    let tuple = PyTuple::new(py, &[PyBytes::new(py, &id_bytes).into_any(), field_dict.into_any()])?;
                    result_list.append(tuple)?;
                }
                Ok(result_list.into_any().unbind())
            }
            "xgroup_create" => {
                let name = &args.get_item(0)?;
                let groupname = &args.get_item(1)?;
                let key = extract_bytes(name)?;
                let group = extract_bytes(groupname)?;
                let id_str: String = kwargs.get_item("id")?.map(|v| v.extract()).transpose()?.unwrap_or_else(|| "$".to_string());
                let mkstream: bool = kwargs.get_item("mkstream")?.map(|v| v.extract()).transpose()?.unwrap_or(false);
                let stream_id: StreamId = if id_str == "$" {
                    (u64::MAX, u64::MAX)
                } else if id_str == "0" || id_str == "0-0" {
                    (0, 0)
                } else {
                    parse_stream_id(&id_str).ok_or_else(|| pyo3::exceptions::PyValueError::new_err(format!("Invalid stream ID format: {}", id_str)))?
                };
                self.store.xgroup_create(&key, group, stream_id, mkstream)
                    .map_err(|e| store_err_to_py_for_cmd(e, "XGROUP CREATE"))?;
                Ok(pyo3::types::PyBool::new(py, true).to_owned().into_any().unbind())
            }
            "xgroup_destroy" => {
                let name = &args.get_item(0)?;
                let groupname = &args.get_item(1)?;
                let key = extract_bytes(name)?;
                let group = extract_bytes(groupname)?;
                let destroyed = self.store.xgroup_destroy(&key, &group).map_err(store_err_to_py)?;
                let val = if destroyed { 1i64 } else { 0i64 };
                Ok(val.into_pyobject(py)?.into_any().unbind())
            }
            "xreadgroup" => {
                let groupname = &args.get_item(0)?;
                let consumername = &args.get_item(1)?;
                let streams_dict = args.get_item(2)?.downcast::<PyDict>()?.clone();
                let group = extract_bytes(groupname)?;
                let consumer = extract_bytes(consumername)?;
                let count: Option<usize> = kwargs.get_item("count")?.and_then(|v| if v.is_none() { None } else { v.extract().ok() });
                let mut keys: Vec<Bytes> = Vec::new();
                let mut id_strs: Vec<String> = Vec::new();
                for (k, v) in streams_dict.iter() {
                    let key = extract_bytes(&k)?;
                    let id_str: String = v.extract::<String>().or_else(|_| v.extract::<Vec<u8>>().map(|b| String::from_utf8_lossy(&b).into_owned()))?;
                    keys.push(key);
                    id_strs.push(id_str);
                }
                let results = self.store.xreadgroup(&group, &consumer, &keys, &id_strs, count)
                    .map_err(|e| store_err_to_py_for_cmd(e, "XREADGROUP"))?;
                format_xreadgroup_result_with_py(py, results)
            }
            "xack" => {
                let name = &args.get_item(0)?;
                let groupname = &args.get_item(1)?;
                let key = extract_bytes(name)?;
                let group = extract_bytes(groupname)?;
                let mut stream_ids: Vec<StreamId> = Vec::new();
                for id_obj in args.iter().skip(2) {
                    let id_str: String = id_obj.extract::<String>().or_else(|_| id_obj.extract::<Vec<u8>>().map(|b| String::from_utf8_lossy(&b).into_owned()))?;
                    let stream_id = parse_stream_id(&id_str).ok_or_else(|| pyo3::exceptions::PyValueError::new_err(format!("Invalid stream ID format: {}", id_str)))?;
                    stream_ids.push(stream_id);
                }
                let count = self.store.xack(&key, &group, &stream_ids)
                    .map_err(|e| store_err_to_py_for_cmd(e, "XACK"))?;
                Ok(count.into_pyobject(py)?.into_any().unbind())
            }
            "xautoclaim" => {
                let name = &args.get_item(0)?;
                let groupname = &args.get_item(1)?;
                let consumername = &args.get_item(2)?;
                let min_idle_time: u64 = args.get_item(3)?.extract()?;
                let key = extract_bytes(name)?;
                let group = extract_bytes(groupname)?;
                let consumer_bytes = extract_bytes(consumername)?;
                let start_id_str: String = kwargs.get_item("start_id")?.map(|v| v.extract()).transpose()?.unwrap_or_else(|| "0-0".to_string());
                let count: Option<usize> = kwargs.get_item("count")?.and_then(|v| if v.is_none() { None } else { v.extract().ok() });
                let start: StreamId = if start_id_str == "0" || start_id_str == "0-0" { (0, 0) } else {
                    parse_stream_id(&start_id_str).ok_or_else(|| pyo3::exceptions::PyValueError::new_err(format!("Invalid stream ID format: {}", start_id_str)))?
                };
                let (next_id, claimed, deleted) = self.store.xautoclaim(&key, &group, consumer_bytes, min_idle_time, start, count)
                    .map_err(|e| store_err_to_py_for_cmd(e, "XAUTOCLAIM"))?;
                let next_id_bytes = pyo3::types::PyBytes::new(py, format_stream_id(next_id).as_bytes());
                let claimed_list = pyo3::types::PyList::empty(py);
                for (id, fields) in &claimed {
                    let id_bytes = pyo3::types::PyBytes::new(py, format_stream_id(*id).as_bytes());
                    let field_dict = PyDict::new(py);
                    for (fk, fv) in fields {
                        field_dict.set_item(PyBytes::new(py, fk.as_ref()), PyBytes::new(py, fv.as_ref()))?;
                    }
                    let tuple = PyTuple::new(py, &[id_bytes.into_any(), field_dict.into_any()])?;
                    claimed_list.append(tuple)?;
                }
                let deleted_list = pyo3::types::PyList::empty(py);
                for id in &deleted {
                    let id_bytes = pyo3::types::PyBytes::new(py, format_stream_id(*id).as_bytes());
                    deleted_list.append(id_bytes)?;
                }
                let result = PyTuple::new(py, &[next_id_bytes.into_any(), claimed_list.into_any(), deleted_list.into_any()])?;
                Ok(result.into_any().unbind())
            }
            "xclaim" => {
                let name = &args.get_item(0)?;
                let groupname = &args.get_item(1)?;
                let consumername = &args.get_item(2)?;
                let min_idle_time: u64 = args.get_item(3)?.extract()?;
                let message_ids = &args.get_item(4)?;
                let key = extract_bytes(name)?;
                let group = extract_bytes(groupname)?;
                let consumer = extract_bytes(consumername)?;
                let idle: Option<u64> = kwargs.get_item("idle")?.and_then(|v| if v.is_none() { None } else { v.extract().ok() });
                let time: Option<u64> = kwargs.get_item("time")?.and_then(|v| if v.is_none() { None } else { v.extract().ok() });
                let retrycount: Option<u64> = kwargs.get_item("retrycount")?.and_then(|v| if v.is_none() { None } else { v.extract().ok() });
                let force: bool = kwargs.get_item("force")?.map(|v| v.extract()).transpose()?.unwrap_or(false);
                let justid: bool = kwargs.get_item("justid")?.map(|v| v.extract()).transpose()?.unwrap_or(false);
                let ids_list: Vec<Py<PyAny>> = message_ids.extract()?;
                let mut ids: Vec<StreamId> = Vec::new();
                for id_obj in &ids_list {
                    let id_str: String = id_obj.bind(py).extract::<String>().or_else(|_| id_obj.bind(py).extract::<Vec<u8>>().map(|b| String::from_utf8_lossy(&b).into_owned()))?;
                    ids.push(parse_stream_id(&id_str).ok_or_else(|| PyErr::new::<pyo3::exceptions::PyValueError, _>(format!("Invalid stream ID: {}", id_str)))?);
                }
                let results = self.store.xclaim(&key, &group, consumer, min_idle_time, &ids, idle, time, retrycount, force, justid)
                    .map_err(|e| store_err_to_py_for_cmd(e, "XCLAIM"))?;
                let outer = pyo3::types::PyList::empty(py);
                for (id, fields_opt) in &results {
                    if justid {
                        let id_bytes = format_stream_id(*id).into_bytes();
                        outer.append(PyBytes::new(py, &id_bytes))?;
                    } else if let Some(fields) = fields_opt {
                        let id_bytes = format_stream_id(*id).into_bytes();
                        let field_dict = PyDict::new(py);
                        for (fk, fv) in fields {
                            field_dict.set_item(PyBytes::new(py, fk.as_ref()), PyBytes::new(py, fv.as_ref()))?;
                        }
                        let tuple = PyTuple::new(py, &[PyBytes::new(py, &id_bytes).into_any(), field_dict.into_any()])?;
                        outer.append(tuple)?;
                    }
                }
                Ok(outer.into_any().unbind())
            }
            "xinfo_groups" => {
                let name = &args.get_item(0)?;
                let key = extract_bytes(name)?;
                let groups = self.store.xinfo_groups(&key)
                    .map_err(|e| store_err_to_py_for_cmd(e, "XINFO GROUPS"))?;
                let result_list = pyo3::types::PyList::empty(py);
                for group_info in &groups {
                    let dict = PyDict::new(py);
                    if let Some(name_val) = group_info.get("name") {
                        dict.set_item(pyo3::types::PyString::new(py, "name"), PyBytes::new(py, name_val.as_bytes()))?;
                    }
                    if let Some(consumers_val) = group_info.get("consumers") {
                        let count: i64 = consumers_val.parse().unwrap_or(0);
                        dict.set_item(pyo3::types::PyString::new(py, "consumers"), count)?;
                    }
                    if let Some(pending_val) = group_info.get("pending") {
                        let count: i64 = pending_val.parse().unwrap_or(0);
                        dict.set_item(pyo3::types::PyString::new(py, "pending"), count)?;
                    }
                    if let Some(id_val) = group_info.get("last-delivered-id") {
                        dict.set_item(pyo3::types::PyString::new(py, "last-delivered-id"), PyBytes::new(py, id_val.as_bytes()))?;
                    }
                    result_list.append(dict)?;
                }
                Ok(result_list.into_any().unbind())
            }
            "xinfo_consumers" => {
                let name = &args.get_item(0)?;
                let groupname = &args.get_item(1)?;
                let key = extract_bytes(name)?;
                let group = extract_bytes(groupname)?;
                let consumers = self.store.xinfo_consumers(&key, &group)
                    .map_err(|e| store_err_to_py_for_cmd(e, "XINFO CONSUMERS"))?;
                let result_list = pyo3::types::PyList::empty(py);
                for consumer_info in &consumers {
                    let dict = PyDict::new(py);
                    if let Some(name_val) = consumer_info.get("name") {
                        dict.set_item(pyo3::types::PyString::new(py, "name"), PyBytes::new(py, name_val.as_bytes()))?;
                    }
                    if let Some(pending_val) = consumer_info.get("pending") {
                        let count: i64 = pending_val.parse().unwrap_or(0);
                        dict.set_item(pyo3::types::PyString::new(py, "pending"), count)?;
                    }
                    if let Some(idle_val) = consumer_info.get("idle") {
                        let idle: i64 = idle_val.parse().unwrap_or(0);
                        dict.set_item(pyo3::types::PyString::new(py, "idle"), idle)?;
                    }
                    result_list.append(dict)?;
                }
                Ok(result_list.into_any().unbind())
            }
            "xpending_range" => {
                let name = &args.get_item(0)?;
                let groupname = &args.get_item(1)?;
                let min_str: String = args.get_item(2)?.extract()?;
                let max_str: String = args.get_item(3)?.extract()?;
                let count: usize = args.get_item(4)?.extract()?;
                let key = extract_bytes(name)?;
                let group = extract_bytes(groupname)?;
                let consumer_filter: Option<Bytes> = kwargs.get_item("consumername")?.and_then(|v| if v.is_none() { None } else { extract_bytes(&v).ok() });
                let idle: Option<u64> = kwargs.get_item("idle")?.and_then(|v| if v.is_none() { None } else { v.extract().ok() });
                let min_id: StreamId = if min_str == "-" { (0, 0) } else {
                    parse_stream_id(&min_str).ok_or_else(|| pyo3::exceptions::PyValueError::new_err(format!("Invalid stream ID format: {}", min_str)))?
                };
                let max_id: StreamId = if max_str == "+" { (u64::MAX, u64::MAX) } else {
                    parse_stream_id(&max_str).ok_or_else(|| pyo3::exceptions::PyValueError::new_err(format!("Invalid stream ID format: {}", max_str)))?
                };
                let entries = self.store.xpending_range(&key, &group, min_id, max_id, count, consumer_filter.as_ref(), idle)
                    .map_err(|e| store_err_to_py_for_cmd(e, "XPENDING"))?;
                let result_list = pyo3::types::PyList::empty(py);
                for (entry_id, consumer_name, idle_ms, delivery_count) in &entries {
                    let dict = PyDict::new(py);
                    let id_str = format_stream_id(*entry_id).into_bytes();
                    dict.set_item(PyBytes::new(py, b"message_id"), PyBytes::new(py, &id_str))?;
                    dict.set_item(PyBytes::new(py, b"consumer"), PyBytes::new(py, consumer_name.as_ref()))?;
                    dict.set_item(PyBytes::new(py, b"time_since_delivered"), *idle_ms as i64)?;
                    dict.set_item(PyBytes::new(py, b"times_delivered"), *delivery_count as i64)?;
                    result_list.append(dict)?;
                }
                Ok(result_list.into_any().unbind())
            }
            "eval" => {
                let script: String = args.get_item(0)?.extract()?;
                let numkeys: usize = args.get_item(1)?.extract()?;
                let mut keys: Vec<Bytes> = Vec::new();
                let mut eval_args: Vec<Bytes> = Vec::new();
                for (i, obj) in args.iter().skip(2).enumerate() {
                    let b = extract_bytes(&obj)?;
                    if i < numkeys { keys.push(b); } else { eval_args.push(b); }
                }
                let result = self.store.eval(&script, keys, eval_args);
                match result {
                    Ok(val) => redis_value_to_py(py, val),
                    Err(msg) => Err(make_response_error(msg)),
                }
            }
            "evalsha" => {
                let sha: String = args.get_item(0)?.extract()?;
                let numkeys: usize = args.get_item(1)?.extract()?;
                let mut keys: Vec<Bytes> = Vec::new();
                let mut eval_args: Vec<Bytes> = Vec::new();
                for (i, obj) in args.iter().skip(2).enumerate() {
                    let b = extract_bytes(&obj)?;
                    if i < numkeys { keys.push(b); } else { eval_args.push(b); }
                }
                let result = self.store.evalsha(&sha, keys, eval_args);
                match result {
                    Ok(val) => redis_value_to_py(py, val),
                    Err(msg) => Err(make_response_error(msg)),
                }
            }
            "script_load" => {
                let script: String = args.get_item(0)?.extract()?;
                let sha = self.store.script_load(&script);
                Ok(sha.into_pyobject(py)?.into_any().unbind())
            }
            "script_exists" => {
                let shas: Vec<String> = args.iter().map(|obj| obj.extract::<String>()).collect::<PyResult<Vec<_>>>()?;
                let results = self.store.script_exists(&shas);
                Ok(results.into_pyobject(py)?.into_any().unbind())
            }
            "publish" => {
                let channel = &args.get_item(0)?;
                let message = &args.get_item(1)?;
                let channel_bytes = extract_bytes(channel)?;
                let message_bytes = extract_bytes(message)?;
                let count = self.store.publish(Bytes::from(channel_bytes), Bytes::from(message_bytes));
                Ok(count.into_pyobject(py)?.into_any().unbind())
            }
            "keys" => {
                let pattern: String = args.get_item(0)?.extract()?;
                let pat = Bytes::from(pattern.into_bytes());
                let keys = self.store.keys(pat.as_ref());
                let result: Vec<Vec<u8>> = keys.into_iter().map(|k| k.to_vec()).collect();
                Ok(result.into_pyobject(py)?.into_any().unbind())
            }
            "ttl" => {
                let name = &args.get_item(0)?;
                let key = extract_bytes(name)?;
                let ttl_val = self.store.ttl(&key);
                Ok(ttl_val.into_pyobject(py)?.into_any().unbind())
            }
            "setex" => {
                // setex(name, time, value) -> set(name, value, ex=time)
                let name = &args.get_item(0)?;
                let time = &args.get_item(1)?;
                let value = &args.get_item(2)?;
                let key = extract_bytes(name)?;
                let val = extract_bytes(value)?;
                let ttl = Some(extract_expiry(time, false)?);
                let success = self.store.set(key, val, ttl, false, false);
                if success {
                    Ok(pyo3::types::PyBool::new(py, true).to_owned().into_any().unbind())
                } else {
                    Ok(py.None())
                }
            }
            "mget" => {
                let key_list: Vec<Bytes> = args.iter().map(|k| extract_bytes(&k)).collect::<PyResult<Vec<Bytes>>>()?;
                let results = self.store.mget(&key_list);
                let py_results: Vec<Option<Vec<u8>>> = results.into_iter().map(|opt| opt.map(|b| b.to_vec())).collect();
                Ok(py_results.into_pyobject(py)?.into_any().unbind())
            }
            "xpending" => {
                let name = &args.get_item(0)?;
                let groupname = &args.get_item(1)?;
                let key = extract_bytes(name)?;
                let group = extract_bytes(groupname)?;
                let (total, min_id, max_id, consumers) = self.store.xpending_summary(&key, &group)
                    .map_err(|e| store_err_to_py_for_cmd(e, "XPENDING"))?;
                let dict = PyDict::new(py);
                dict.set_item("pending", total)?;
                match min_id {
                    Some(id) => dict.set_item("min", format_stream_id(id))?,
                    None => dict.set_item("min", py.None())?,
                }
                match max_id {
                    Some(id) => dict.set_item("max", format_stream_id(id))?,
                    None => dict.set_item("max", py.None())?,
                }
                let consumer_list = pyo3::types::PyList::empty(py);
                for (cname, count) in consumers {
                    let cdict = PyDict::new(py);
                    cdict.set_item("name", PyBytes::new(py, &cname))?;
                    cdict.set_item("pending", count)?;
                    consumer_list.append(cdict)?;
                }
                dict.set_item("consumers", consumer_list)?;
                Ok(dict.into_any().unbind())
            }
            _ => {
                Err(pyo3::exceptions::PyException::new_err(format!("Unknown pipeline command: {}", method)))
            }
        }
    }
}

#[pymodule]
fn _burner_redis(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Initialize Tokio multi-thread runtime for future_into_py compatibility.
    // Still needed for blocking xreadgroup and _subscribe_listener.
    let mut builder = tokio::runtime::Builder::new_multi_thread();
    builder.enable_all();
    pyo3_async_runtimes::tokio::init(builder);

    m.add_class::<BurnerRedis>()?;
    m.add_class::<ResolvedFuture>()?;
    Ok(())
}
