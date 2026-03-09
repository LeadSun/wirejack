use super::Body;
use bytes::BytesMut;
use pyo3::{exceptions::PyValueError, prelude::*};
use std::collections::HashMap;

pub type HttpRequest = http::Request<Body>;

#[pyclass(name = "Request")]
#[derive(Debug)]
pub struct PyHttpRequest {
    inner: HttpRequest,
}

impl PyHttpRequest {
    pub fn inner(&self) -> &HttpRequest {
        &self.inner
    }

    pub fn take_stream(&mut self) -> HttpRequest {
        let (parts, mut body) = std::mem::take(&mut self.inner).into_parts();
        let hyper_body = body.take_stream();
        self.inner = HttpRequest::from_parts(parts.clone(), body);
        HttpRequest::from_parts(parts, hyper_body)
    }
}

impl From<HttpRequest> for PyHttpRequest {
    fn from(inner: HttpRequest) -> Self {
        Self { inner }
    }
}

#[pymethods]
impl PyHttpRequest {
    #[new]
    fn new() -> Self {
        HttpRequest::new(Body::empty()).into()
    }

    #[getter]
    fn uri(&self) -> String {
        self.inner.uri().to_string()
    }

    #[setter]
    fn set_uri(&mut self, uri: &str) -> PyResult<()> {
        *self.inner.uri_mut() = uri
            .parse()
            .map_err(|e| PyValueError::new_err(format!("Invalid URI: {e}")))?;
        Ok(())
    }

    #[getter]
    fn headers(&self) -> HashMap<String, Vec<u8>> {
        super::headers_to_hashmap(self.inner.headers())
    }

    #[setter]
    fn set_headers(&mut self, headers: HashMap<String, Vec<u8>>) -> PyResult<()> {
        *self.inner.headers_mut() = super::headers_from_hashmap(headers)?;
        Ok(())
    }

    #[getter]
    fn body(&self) -> Option<&[u8]> {
        self.inner.body().bytes()
    }

    #[setter]
    fn set_body(&mut self, val: &[u8]) {
        *self.inner.body_mut() = BytesMut::from(val).into();
    }
}
