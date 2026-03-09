use super::Body;
use bytes::BytesMut;
use http::StatusCode;
use pyo3::{exceptions::PyValueError, prelude::*};
use std::collections::HashMap;

pub type HttpResponse = http::Response<Body>;

#[pyclass(name = "Response")]
#[derive(Debug)]
pub struct PyHttpResponse {
    inner: HttpResponse,
}

impl PyHttpResponse {
    pub fn into_inner(self) -> HttpResponse {
        self.inner
    }

    pub fn inner(&self) -> &HttpResponse {
        &self.inner
    }

    pub fn take_stream(&mut self) -> HttpResponse {
        let (parts, mut body) = std::mem::take(&mut self.inner).into_parts();
        let hyper_body = body.take_stream();
        self.inner = HttpResponse::from_parts(parts.clone(), body);
        HttpResponse::from_parts(parts, hyper_body)
    }
}

impl From<HttpResponse> for PyHttpResponse {
    fn from(inner: HttpResponse) -> Self {
        Self { inner }
    }
}

#[pymethods]
impl PyHttpResponse {
    #[new]
    fn new() -> Self {
        HttpResponse::new(Body::empty()).into()
    }

    #[getter]
    fn status(&self) -> u16 {
        self.inner.status().as_u16()
    }

    #[setter]
    fn set_status(&mut self, status: u16) -> PyResult<()> {
        *self.inner.status_mut() = StatusCode::from_u16(status)
            .map_err(|e| PyValueError::new_err(format!("Invalid HTTP status code: {e}")))?;
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
