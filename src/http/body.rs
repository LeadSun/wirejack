use bytes::{Bytes, BytesMut};
use http::{HeaderMap, HeaderValue};
use http_body_util::{BodyExt, Either, Full};
use hyper::body::{Frame, Incoming};
use std::ops::Deref;
use std::pin::{Pin, pin};
use std::task::{self, Poll};

// TODO: This should probably be configurable.
const MAX_LENGTH: u32 = 100_000_000;

#[derive(Debug)]
pub enum Body {
    BytesMut(BytesMut),
    Bytes(Bytes),
    TooBig(hyper::body::Incoming),
    Consumed,
}

impl Body {
    pub fn too_big(incoming: hyper::body::Incoming) -> Self {
        Self::TooBig(incoming)
    }

    pub fn empty() -> Self {
        Self::BytesMut(BytesMut::new())
    }

    pub fn full<T: Into<BytesMut>>(content: T) -> Self {
        Self::BytesMut(content.into())
    }

    pub async fn from_incoming(
        incoming: Incoming,
        headers: &HeaderMap<HeaderValue>,
    ) -> hyper::Result<Self> {
        if headers
            .get("content-length")
            .and_then(|h| h.to_str().ok())
            .and_then(|len| len.parse::<u32>().ok())
            .map(|l| l < MAX_LENGTH)
            .unwrap_or(true)
        {
            Ok(Body::BytesMut(incoming.collect().await?.to_bytes().into()))
        } else {
            Ok(Body::too_big(incoming))
        }
    }

    pub fn to_hyper(&mut self) -> Option<Either<Full<Bytes>, Incoming>> {
        let body = std::mem::take(self);
        match body {
            Self::BytesMut(bytes_mut) => {
                let bytes = bytes_mut.freeze();
                *self = Self::Bytes(bytes.clone());
                Some(Either::Left(Full::new(bytes)))
            }
            Self::Bytes(bytes) => {
                *self = Self::Bytes(bytes.clone());
                Some(Either::Left(Full::new(bytes)))
            }
            Self::TooBig(incoming) => Some(Either::Right(incoming)),
            Self::Consumed => None,
        }
    }

    pub fn take_stream(&mut self) -> Self {
        let body = std::mem::replace(self, Self::Consumed);
        match body {
            Self::BytesMut(bytes_mut) => {
                let bytes = bytes_mut.freeze();
                *self = Self::Bytes(bytes.clone());
                Self::Bytes(bytes)
            }
            Self::Bytes(bytes) => {
                *self = Self::Bytes(bytes.clone());
                Self::Bytes(bytes)
            }
            _ => body,
        }
    }

    pub fn bytes(&self) -> Option<&[u8]> {
        match self {
            Body::BytesMut(bytes_mut) => Some(&bytes_mut),
            Body::Bytes(bytes) => Some(&bytes),
            _ => None,
        }
    }

    pub fn copy_mut(&self) -> Option<BytesMut> {
        if let Self::Bytes(bytes) = self {
            Some(BytesMut::from(bytes.deref()))
        } else {
            None
        }
    }
}

impl Default for Body {
    fn default() -> Self {
        Self::Consumed
    }
}

impl<T: Into<Bytes>> From<T> for Body {
    fn from(val: T) -> Self {
        Self::Bytes(val.into())
    }
}

impl hyper::body::Body for Body {
    type Data = Bytes;
    type Error = hyper::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, <Body as hyper::body::Body>::Error>>> {
        match &mut *self {
            Self::BytesMut(_) | Self::Bytes(_) => {
                match std::mem::replace(&mut *self, Body::Consumed) {
                    Self::BytesMut(bytes_mut) => {
                        Poll::Ready(Some(Ok(Frame::data(bytes_mut.freeze()))))
                    }
                    Self::Bytes(bytes) => Poll::Ready(Some(Ok(Frame::data(bytes)))),
                    _ => unreachable!(),
                }
            }
            Self::TooBig(i) => pin!(i).poll_frame(cx),
            Self::Consumed => Poll::Ready(None),
        }
    }
}
