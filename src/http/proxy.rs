mod proxy_service;
mod python_service;
pub use python_service::PythonService;
mod tls;
pub use tls::TlsConfig;
mod upstream;

use http::Uri;
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use hyper_util::service::TowerToHyperService;
use rustls::ClientConfig;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tower::ServiceBuilder;
use tracing::*;
use upstream::UpstreamProxyConnector;
use crate::http::Body;

pub type Forwarder = hyper_util::client::legacy::Client<
    hyper_rustls::HttpsConnector<upstream::UpstreamProxyConnector>,
    Body,
>;
type ServerBuilder = hyper_util::server::conn::auto::Builder<TokioExecutor>;

pub async fn run(
    bind: SocketAddr,
    service: PythonService<Forwarder>,
    tls: TlsConfig,
    filter: Vec<Uri>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let listener = TcpListener::bind(bind).await?;
    info!("Listening on http://{}", bind);
    let filter = Arc::new(filter);

    loop {
        let (stream, _) = listener.accept().await?;
        let io = TokioIo::new(stream);

        let service = ServiceBuilder::new().service(proxy_service::Proxy::new(
            tls.clone(),
            service.clone(),
            filter.clone(),
        ));
        let service = TowerToHyperService::new(service);
        tokio::task::spawn(async move {
            if let Err(err) = ServerBuilder::new(TokioExecutor::new())
                //.preserve_header_case(true)
                //.title_case_headers(true)
                .serve_connection_with_upgrades(io, service)
                .await
            {
                error!("Failed to serve connection: {:?}", err);
            }
        });
    }
}

pub fn make_client<B>(
    proxy_uri: Option<Uri>,
) -> hyper_util::client::legacy::Client<
    hyper_rustls::HttpsConnector<upstream::UpstreamProxyConnector>,
    B,
>
where
    B: hyper::body::Body + Send + 'static + Unpin,
    B::Data: Send,
    B::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    let root_store =
        rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let tls = ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();
    let upstream = if let Some(proxy_uri) = proxy_uri {
        UpstreamProxyConnector::from_uri(proxy_uri).unwrap()
    } else {
        UpstreamProxyConnector::no_proxy()
    };
    let https = hyper_rustls::HttpsConnectorBuilder::new()
        .with_tls_config(tls)
        .https_or_http()
        .enable_all_versions()
        .wrap_connector(upstream);
    hyper_util::client::legacy::Client::builder(TokioExecutor::new())
        .pool_timer(TokioTimer::new())
        .pool_idle_timeout(Duration::from_secs(30))
        .set_host(false) // Don't add Host: header if it's missing.
        .build(https)
}
