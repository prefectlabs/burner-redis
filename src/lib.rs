use pyo3::prelude::*;
use std::sync::Arc;

mod store;
mod commands;

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
