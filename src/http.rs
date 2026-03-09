mod body;
pub use body::Body;
mod http_context;
pub use http_context::HttpContext;
pub mod proxy;
mod request;
pub use request::{HttpRequest, PyHttpRequest};
mod response;
pub use response::{HttpResponse, PyHttpResponse};

use http::{HeaderMap, HeaderValue, Uri};
use pyo3::{exceptions::PyValueError, prelude::*};
use std::collections::HashMap;

fn headers_to_hashmap(headers: &HeaderMap<HeaderValue>) -> HashMap<String, Vec<u8>> {
    headers
        .iter()
        .map(|(k, v)| (k.as_str().to_string(), v.as_bytes().to_vec()))
        .collect()
}

fn headers_from_hashmap(headers: HashMap<String, Vec<u8>>) -> PyResult<HeaderMap<HeaderValue>> {
    headers
        .into_iter()
        .map(|(k, v)| Ok((k.try_into()?, v.try_into()?)))
        .collect::<http::Result<_>>()
        .map_err(|e| PyValueError::new_err(format!("Invalid headers: {e}")))
}

fn uri_matches(patterns: &[Uri], target: &Uri) -> bool {
    let Some(target_host) = target.host() else {
        return false;
    };
    let target_host_parts: Vec<&str> = target_host.split(".").collect();
    let target_path_parts: Vec<&str> = target.path().split("/").collect();

    patterns.iter().any(|pattern| {
        (pattern.scheme().is_none()
            || pattern.scheme_str() == Some("*")
            || pattern.scheme() == target.scheme())
            && (pattern.port().is_none() || pattern.port() == target.port())
            && pattern
                .host()
                .map(|ph| {
                    ph.is_empty()
                        || parts_match(&ph.split(".").collect::<Vec<_>>(), &target_host_parts)
                })
                .unwrap_or(true)
            && (pattern.path().is_empty()
                || parts_match(
                    &pattern.path().split("/").collect::<Vec<_>>(),
                    &target_path_parts,
                ))
    })
}

fn parts_match(pattern: &[&str], target: &[&str]) -> bool {
    pattern.len() == target.len()
        && pattern
            .iter()
            .zip(target.iter())
            .all(|(p, t)| *p == "*" || p == t)
}
