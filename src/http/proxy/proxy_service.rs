use super::TlsConfig;
use crate::http::{Body as HttpBody, uri_matches};
use http::uri::{Authority, PathAndQuery, Scheme};
use http::{Method, Response, Uri};
use hyper::upgrade::Upgraded;
use hyper::{
    Request,
    body::{Body, Incoming},
};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::service::TowerToHyperService;
use rustls::server::Acceptor;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Poll;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio_rustls::LazyConfigAcceptor;
use tower::Service;
use tracing::*;

type ServerBuilder = hyper_util::server::conn::auto::Builder<TokioExecutor>;

#[derive(Clone)]
pub struct Proxy<S> {
    inner: S,
    tls: TlsConfig,
    filter: Arc<Vec<Uri>>,
}

impl<S> Proxy<S> {
    pub fn new(tls: TlsConfig, inner: S, filter: Arc<Vec<Uri>>) -> Self {
        Self { inner, tls, filter }
    }
}

type Req = Request<Incoming>;
impl<S, E> Service<Request<Incoming>> for Proxy<S>
where
    S: Service<Request<Incoming>, Response = Response<HttpBody>> + Clone + Send + 'static,
    S::Future: Future<Output = Result<Response<HttpBody>, E>> + Send + 'static,
    S::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
    E: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    type Response = Response<HttpBody>;
    type Error = Box<dyn std::error::Error + Send + Sync>;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
        //self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Req) -> Self::Future {
        debug!("Proxy request: {} {}", req.method(), req.uri());
        if req.method() == Method::CONNECT {
            if let Some(addr) = host_addr(req.uri()) {
                trace!("Proxy CONNECT upgrade target: {addr}");
                let service = self.inner.clone();
                let tls = self.tls.clone();
                let filter = self.filter.clone();
                tokio::task::spawn(async move {
                    let uri = req.uri().clone();
                    match hyper::upgrade::on(req).await {
                        Ok(upgraded) => {
                            if let Err(e) = tunnel(upgraded, uri, tls, service, filter).await {
                                error!("Server io error: {}", e);
                            };
                        }
                        Err(e) => error!("Upgrade error: {}", e),
                    }
                });

                let fut = async { Ok(Response::new(HttpBody::empty())) };
                Box::pin(fut)
            } else {
                error!("CONNECT host is not socket addr: {:?}", req.uri());
                let mut resp = Response::new(HttpBody::full("CONNECT must be to a socket address"));
                *resp.status_mut() = http::StatusCode::BAD_REQUEST;

                let fut = async { Ok(resp) };
                Box::pin(fut)
            }
        } else {
            // Proxy HTTP
            trace!("HTTP direct proxy");
            let mut service = self.inner.clone();
            let fut = async move { service.call(req).await.map_err(|e| e.into()) };
            Box::pin(fut)
        }
    }
}

/// Adds a fixed scheme and authority to requests to support hyper clients.
#[derive(Debug, Clone)]
struct UriFixer<S> {
    inner: S,
    scheme: Scheme,
    authority: Authority,
}

impl<S> UriFixer<S> {
    fn new(inner: S, scheme: Scheme, authority: Authority) -> Self {
        Self {
            inner,
            scheme,
            authority,
        }
    }
}

impl<S: Service<Req>> Service<Req> for UriFixer<S>
where
    S: Service<Req> + Clone,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Req) -> Self::Future {
        let mut parts = req.uri().clone().into_parts();
        parts.scheme = Some(self.scheme.clone());
        parts.authority = Some(self.authority.clone());
        if parts.path_and_query.is_none() {
            parts.path_and_query = Some(PathAndQuery::from_static("/"));
        }
        *req.uri_mut() = Uri::from_parts(parts).unwrap();

        self.inner.call(req)
    }
}

fn host_addr(uri: &http::Uri) -> Option<String> {
    uri.authority().map(|auth| auth.to_string())
}

// Creates a tunnel between the client and the target server, optionally
// intercepting the traffic if the filter matches.
async fn tunnel<S, B>(
    upgraded: Upgraded,
    uri: Uri,
    tls: TlsConfig,
    service: S,
    filter: Arc<Vec<Uri>>,
) -> std::io::Result<()>
where
    S: Service<Request<Incoming>, Response = Response<B>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
    B: Body + Send + 'static,
    B::Data: Send,
    B::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    trace!("Opening tunnel to: {uri}");

    if filter.is_empty() || uri_matches(&filter, &uri) {
        // MITM the TLS connection.
        let acceptor = LazyConfigAcceptor::new(Acceptor::default(), TokioIo::new(upgraded));
        tokio::pin!(acceptor);
        match acceptor.as_mut().await {
            Ok(start) => {
                let config = tls.server_config(start.client_hello()).await;
                let stream = start.into_stream(config).await?;

                let service =
                    UriFixer::new(service, Scheme::HTTPS, uri.authority().unwrap().clone());
                let service = TowerToHyperService::new(service);
                if let Err(e) = ServerBuilder::new(TokioExecutor::new())
                    .serve_connection(TokioIo::new(stream), service)
                    .await
                {
                    error!("Error serving connection: {}", e);
                }
            }
            Err(err) => {
                if let Some(mut stream) = acceptor.take_io() {
                    stream
                        .write_all(
                            format!("HTTP/1.1 400 Invalid Input\r\n\r\n\r\n{:?}\n", err).as_bytes(),
                        )
                        .await
                        .unwrap();
                }
            }
        }
    } else {
        // Proxy traffic without inspecting.
        let addr = host_addr(&uri).unwrap(); // Should never fail.

        let mut server = TcpStream::connect(addr).await?;
        let mut upgraded = TokioIo::new(upgraded);

        let (from_client, from_server) =
            tokio::io::copy_bidirectional(&mut upgraded, &mut server).await?;

        debug!(
            "Tunnel client wrote {} bytes and received {} bytes",
            from_client, from_server
        );
    }

    Ok(())
}
