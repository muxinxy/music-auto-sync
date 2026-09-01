pub mod config;
pub mod database;
pub mod login_diagnostics;
pub mod paths;

use std::sync::{Arc, RwLock};

#[derive(Clone)]
pub struct AppPaths {
    inner: Arc<RwLock<paths::DataPaths>>,
}

impl AppPaths {
    pub fn new(paths: paths::DataPaths) -> Self {
        Self {
            inner: Arc::new(RwLock::new(paths)),
        }
    }

    pub fn get(&self) -> paths::DataPaths {
        self.inner.read().expect("data path lock poisoned").clone()
    }

    pub fn set(&self, paths: paths::DataPaths) {
        *self.inner.write().expect("data path lock poisoned") = paths;
    }
}
