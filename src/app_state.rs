use crate::http::HttpResponse;
use crate::http::{HttpContext, HttpRequest, PyHttpRequest, PyHttpResponse, proxy::Forwarder};
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use std::sync::Arc;
use tokio::sync::watch;

#[derive(Clone)]
pub struct AppState {
    handler_tx: watch::Sender<Arc<Py<PyAny>>>,
    handler_rx: watch::Receiver<Arc<Py<PyAny>>>,
    forwarder: Forwarder,
}

impl AppState {
    pub fn new(handler: Py<PyAny>, forwarder: Forwarder) -> Self {
        let (handler_tx, handler_rx) = watch::channel(Arc::new(handler));
        Self {
            handler_tx,
            handler_rx,
            forwarder,
        }
    }

    pub fn http_context(&self) -> HttpContext {
        HttpContext::new(self.forwarder.clone())
    }

    pub fn to_py(&self) -> PyAppState {
        PyAppState {
            handler_tx: self.handler_tx.clone(),
        }
    }

    pub fn handler_tx(&self) -> watch::Sender<Arc<Py<PyAny>>> {
        self.handler_tx.clone()
    }

    pub async fn call_handler(&self, request: HttpRequest) -> PyResult<HttpResponse> {
        let handler = self.handler_rx.borrow().clone();
        let result = Python::attach(|py| {
            pyo3_async_runtimes::tokio::into_future(
                handler
                    .call_method1(
                        py,
                        "handle_http",
                        (
                            Py::new(py, self.http_context())?,
                            Py::new(py, PyHttpRequest::from(request))?,
                        ),
                    )?
                    .into_bound(py),
            )
        })?
        .await?;
        Python::attach(|py| {
            Ok(result
                .into_bound(py)
                .cast::<PyHttpResponse>()?
                .borrow_mut()
                .take_stream())
        })
    }
}

#[pyclass(frozen)]
#[derive(Clone)]
pub struct PyAppState {
    handler_tx: watch::Sender<Arc<Py<PyAny>>>,
}

#[pymethods]
impl PyAppState {
    fn set_handler<'a>(&self, new_handler: Py<PyAny>) -> PyResult<()> {
        self.handler_tx
            .send(Arc::new(new_handler))
            .map_err(|e| PyRuntimeError::new_err(format!("Set handler error: {e}")))
    }
}
