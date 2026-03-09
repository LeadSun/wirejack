use http::Uri;
use hyper_util::client::legacy::connect::{
    HttpConnector,
    proxy::{SocksV4, SocksV5, Tunnel},
};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use tower::Service;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Debug, Clone)]
pub enum UpstreamProxyConnector {
    NoProxy(HttpConnector),
    SocksV4(SocksV4<HttpConnector>),
    SocksV5(SocksV5<HttpConnector>),
    Tunnel(Tunnel<HttpConnector>),
}

impl UpstreamProxyConnector {
    pub fn no_proxy() -> Self {
        let mut http = HttpConnector::new();
        http.enforce_http(false);
        Self::NoProxy(http)
    }

    pub fn from_uri(uri: Uri) -> Option<Self> {
        let mut http = HttpConnector::new();
        http.enforce_http(false);
        match uri.scheme_str() {
            Some("socks4") => Some(Self::SocksV4(SocksV4::new(uri, http))),
            Some("socks5") | Some("socks") => Some(Self::SocksV5(SocksV5::new(uri, http))),
            Some("http") => Some(Self::Tunnel(Tunnel::new(uri, http))),
            _ => None,
        }
    }
}

impl Service<Uri> for UpstreamProxyConnector {
    type Response = <HttpConnector as Service<Uri>>::Response;
    type Error = BoxError;
    type Future = Pin<
        Box<
            dyn Future<Output = Result<<HttpConnector as Service<Uri>>::Response, BoxError>> + Send,
        >,
    >;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self {
            Self::NoProxy(http) => http.poll_ready(cx).map_err(BoxError::from),
            Self::SocksV4(socks) => socks.poll_ready(cx).map_err(BoxError::from),
            Self::SocksV5(socks) => socks.poll_ready(cx).map_err(BoxError::from),
            Self::Tunnel(tunnel) => tunnel.poll_ready(cx).map_err(BoxError::from),
        }
    }

    fn call(&mut self, req: Uri) -> Self::Future {
        // HACK: This is pretty filthy.
        let socks = match self {
            Self::SocksV4(socks) => Some(socks.call(req.clone())),
            Self::SocksV5(socks) => Some(socks.call(req.clone())),
            _ => None,
        };
        let tunnel = if let Self::Tunnel(tunnel) = self {
            Some(tunnel.call(req.clone()))
        } else {
            None
        };

        let http = if let Self::NoProxy(http) = self {
            Some(http.call(req))
        } else {
            None
        };

        let fut = async move {
            if let Some(socks) = socks {
                socks.await.map_err(BoxError::from)
            } else if let Some(tunnel) = tunnel {
                tunnel.await.map_err(BoxError::from)
            } else if let Some(http) = http {
                http.await.map_err(BoxError::from)
            } else {
                unreachable!();
            }
        };
        Box::pin(fut)
    }
}
