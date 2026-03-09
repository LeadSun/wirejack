use crate::AppState;
use crate::http::{Body, HttpRequest};
use http::{Request, Response};
use hyper::body::Incoming;
use pyo3_async_runtimes::TaskLocals;
use std::future::Future;
use std::pin::Pin;
use tower::Service;

#[derive(Clone)]
pub struct PythonService<S: Clone> {
    inner: S,
    state: AppState,
    task_locals: TaskLocals,
}

impl<S: Clone> PythonService<S> {
    pub fn new(inner: S, state: AppState, task_locals: TaskLocals) -> Self {
        Self {
            inner,
            state,
            task_locals,
        }
    }
}

impl<S> Service<Request<Incoming>> for PythonService<S>
where
    S: Service<HttpRequest, Response = Response<Incoming>> + Clone + Send + 'static,
    S::Error: Into<Box<dyn std::error::Error + Send + Sync>> + std::error::Error,
    S::Future: Send,
{
    type Response = Response<Body>;
    type Error = Box<dyn std::error::Error + Send + Sync>;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(|e| e.into())
    }

    fn call(&mut self, req: Request<Incoming>) -> Self::Future {
        let state = self.state.clone();
        let fut = async move {
            let (parts, incoming) = req.into_parts();
            let body = Body::from_incoming(incoming, &parts.headers).await?;
            let request = HttpRequest::from_parts(parts, body);
            Ok(state.call_handler(request).await.unwrap())
        };
        let fut = pyo3_async_runtimes::tokio::scope(self.task_locals.clone(), fut);
        Box::pin(fut)
    }
}
