mod app_state;
mod http;
mod python;

use crate::http::proxy::TlsConfig;
use ::http::Uri;
use app_state::{AppState, PyAppState};
use http::proxy;
use pyo3::prelude::*;
use python::load_handler;
use python::wirejack;
use rustls::crypto::CryptoProvider;
use std::net::SocketAddr;
use std::path::PathBuf;
use tokio::task::JoinSet;

pub struct HttpConfig {
    pub handler: PathBuf,
    pub bind: Vec<SocketAddr>,
    pub proxy: Option<::http::Uri>,
    pub filter: Vec<Uri>,
    pub interactive: bool,
    pub threads: usize,
}

pub fn proxy_http(config: HttpConfig) {
    pyo3::append_to_inittab!(wirejack);

    // Manually setup async python instead of using the
    // `pyo3_async_runtimes::tokio::main` macro so custom modules can be loaded
    // with `pyo3::append_to_inittab!(module)``.
    async fn main_async(config: HttpConfig) -> PyResult<()> {
        CryptoProvider::install_default(rustls::crypto::aws_lc_rs::default_provider()).unwrap();

        let forwarder = proxy::make_client(config.proxy.clone());
        let state = AppState::new(load_handler(&config.handler)?, forwarder.clone());
        let task_locals = Python::attach(pyo3_async_runtimes::tokio::get_current_locals)?;

        let mut proxies = JoinSet::new();
        let tls = TlsConfig::new(&PathBuf::from(format!(
            "{}/.local/share/wirejack/",
            std::env!("HOME")
        )));
        for bind in config.bind {
            let service =
                proxy::PythonService::new(forwarder.clone(), state.clone(), task_locals.clone());
            proxies.spawn(proxy::run(
                bind,
                service,
                tls.clone(),
                config.filter.clone(),
            ));
        }

        if config.interactive {
            tokio::task::spawn_blocking(move || python::create_interpreter(state.to_py()).unwrap());
        }

        for result in proxies.join_all().await {
            result.unwrap();
        }

        Ok(())
    }

    pyo3::Python::initialize();

    let mut builder = pyo3_async_runtimes::tokio::re_exports::runtime::Builder::new_multi_thread();
    builder.worker_threads(config.threads);
    builder.enable_all();
    pyo3_async_runtimes::tokio::init(builder);

    pyo3::Python::attach(|py| {
        pyo3_async_runtimes::tokio::run(py, main_async(config))
            .map_err(|e| {
                e.print_and_set_sys_last_vars(py);
            })
            .unwrap();
    });
}
