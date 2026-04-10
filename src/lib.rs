use bytes::Bytes;
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict};
use std::collections::HashSet as StdHashSet;
use std::sync::Arc;

mod store;
mod commands;

use commands::strings::{extract_bytes, extract_expiry};
use store::{Store, StoreError};

/// Convert a StoreError into a Python exception with the Redis-compatible error message.
fn store_err_to_py(e: StoreError) -> PyErr {
    match e {
        StoreError::WrongType => {
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
        BurnerRedis {
            store: Arc::new(Store::new()),
        }
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
