use bytes::Bytes;
use pyo3::prelude::*;
use pyo3::types::PyAny;
use std::sync::Arc;

mod store;
mod commands;

use commands::strings::{extract_bytes, extract_expiry};
use store::Store;

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
}

#[pymodule]
fn _burner_redis(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // CRITICAL: Initialize Tokio current-thread runtime before any future_into_py call.
    // Default is multi_thread() which competes with Python's GIL.
    let mut builder = tokio::runtime::Builder::new_current_thread();
    builder.enable_all();
    pyo3_async_runtimes::tokio::init(builder);

    m.add_class::<BurnerRedis>()?;
    Ok(())
}
