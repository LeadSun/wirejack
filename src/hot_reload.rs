use crate::python::load_handler;
use notify_debouncer_mini::{
    DebounceEventHandler, DebouncedEvent, DebouncedEventKind, Debouncer, new_debouncer,
    notify::{self, RecommendedWatcher, RecursiveMode},
};
use pyo3::prelude::*;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use tracing::error;

/// Watch handler script for changes and reload.
pub fn watch(
    handler_tx: watch::Sender<Arc<Py<PyAny>>>,
    mut path: PathBuf,
) -> notify::Result<Debouncer<RecommendedWatcher>> {
    // Make sure path matches FS notifications.
    path = path.canonicalize().unwrap();

    let mut watcher = new_debouncer(
        Duration::from_secs(2),
        HttpHandlerHotReload {
            handler_tx,
            handler_path: path.clone(),
        },
    )?;

    // FS notifications operate on inodes but text editors may delete and create
    // files instead of modifying. We have to watch the directory instead.
    path.pop();

    watcher
        .watcher()
        .watch(&path, RecursiveMode::NonRecursive)?;
    Ok(watcher)
}

struct HttpHandlerHotReload {
    handler_tx: watch::Sender<Arc<Py<PyAny>>>,
    handler_path: PathBuf,
}

impl DebounceEventHandler for HttpHandlerHotReload {
    fn handle_event(&mut self, event: notify::Result<Vec<DebouncedEvent>>) {
        match event {
            Ok(events) => {
                if events.iter().any(|event| {
                    event.path == self.handler_path && event.kind == DebouncedEventKind::Any
                }) {
                    self.handler_tx
                        .send(Arc::new(load_handler(&self.handler_path).unwrap()))
                        .unwrap();
                }
            }

            Err(e) => {
                error!("Handler hot-reload notify error: {:?}", e)
            }
        }
    }
}
