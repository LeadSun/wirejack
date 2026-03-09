use crate::http::{Body, PyHttpRequest, PyHttpResponse, proxy::Forwarder};
use pyo3::{exceptions::PyRuntimeError, prelude::*};
use tower::Service;
use tracing::warn;

#[pyclass(frozen)]
#[derive(Clone)]
pub struct HttpContext {
    forwarder: Forwarder,
}

impl HttpContext {
    pub fn new(forwarder: Forwarder) -> Self {
        Self { forwarder }
    }
}

#[pymethods]
impl HttpContext {
    fn forward<'a>(
        &self,
        py: Python<'a>,
        request: &mut PyHttpRequest,
    ) -> PyResult<Bound<'a, PyAny>> {
        let mut forwarder = self.forwarder.clone();
        let req = request.take_stream();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            match forwarder.call(req).await {
                Ok(response) => {
                    let (parts, incoming) = response.into_parts();
                    let body = Body::from_incoming(incoming, &parts.headers)
                        .await
                        .map_err(|e| {
                            PyRuntimeError::new_err(format!(
                                "Failed to collect incoming HTTP response: {e}"
                            ))
                        })?;
                    Ok(PyHttpResponse::from(http::Response::from_parts(
                        parts, body,
                    )))
                }

                Err(e) => {
                    warn!("Failed to forward HTTP request: {e}");
                    Ok(http::Response::builder()
                        .status(502)
                        .body("Wirejack failed to reach the target.".into())
                        .unwrap()
                        .into())
                }
            }
        })
    }
}
