//! Response Body wrapper in order to return a custom body or the body from the inner service

use bytes::Bytes;
use http::HeaderMap;
use http_body::{Body, Full, SizeHint};
use pin_project::pin_project;
use std::convert::Infallible;
use std::pin::Pin;
use std::task::{Context, Poll};

#[pin_project(project = BodyProj)]
pub enum ResponseBody<B> {
    EmptyResponse,
    CustomBody {
        #[pin]
        body: Full<Bytes>,
    },
    Body {
        #[pin]
        body: B,
    },
}
impl<B> Default for ResponseBody<B> {
    fn default() -> Self {
        Self::empty_response()
    }
}
impl<B> ResponseBody<B> {
    pub fn empty_response() -> Self {
        ResponseBody::EmptyResponse
    }

    pub fn custom_response(body: Full<Bytes>) -> Self {
        ResponseBody::CustomBody { body }
    }

    pub fn new(body: B) -> Self {
        ResponseBody::Body { body }
    }
}
impl<B> Body for ResponseBody<B>
where
    B: Body<Data = Bytes>,
    B::Error: std::error::Error + 'static,
{
    type Data = Bytes;
    type Error = B::Error;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        match self.project() {
            BodyProj::EmptyResponse => Poll::Ready(None),
            BodyProj::Body { body } => body.poll_data(cx),
            BodyProj::CustomBody { body } => body.poll_data(cx).map_err(|err| match err {}),
        }
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<HeaderMap>, Self::Error>> {
        match self.project() {
            BodyProj::EmptyResponse => Poll::Ready(Ok(None)),
            BodyProj::Body { body } => body.poll_trailers(cx),
            BodyProj::CustomBody { body } => body.poll_trailers(cx).map_err(|err| match err {}),
        }
    }

    fn is_end_stream(&self) -> bool {
        match self {
            ResponseBody::EmptyResponse => true,
            ResponseBody::Body { body } => body.is_end_stream(),
            ResponseBody::CustomBody { body } => body.is_end_stream(),
        }
    }

    fn size_hint(&self) -> SizeHint {
        match self {
            ResponseBody::EmptyResponse => {
                let mut hint = SizeHint::default();
                hint.set_upper(0);
                hint
            }
            ResponseBody::Body { body } => body.size_hint(),
            ResponseBody::CustomBody { body } => body.size_hint(),
        }
    }
}

/// Implementation heavily inspired from https://github.com/davidpdrsn/tower-hyper-http-body-compat
#[cfg(feature = "hyper-v1")]
impl<B> http_body_v1::Body for ResponseBody<B>
where
    B: http_body_v1::Body<Data = Bytes>,
    B::Error: std::error::Error + 'static,
{
    type Data = B::Data;

    type Error = B::Error;

    fn is_end_stream(&self) -> bool {
        match &self {
            ResponseBody::EmptyResponse => true,
            ResponseBody::Body { body } => body.is_end_stream(),
            ResponseBody::CustomBody { body } => body.is_end_stream(),
        }
    }

    fn size_hint(&self) -> http_body_v1::SizeHint {
        match &self {
            ResponseBody::EmptyResponse => {
                let mut hint = http_body_v1::SizeHint::default();
                hint.set_upper(0);
                hint
            }
            ResponseBody::Body { body } => body.size_hint(),
            ResponseBody::CustomBody { body } => {
                let size_hint = body.size_hint();
                let mut out = http_body_v1::SizeHint::new();
                out.set_lower(size_hint.lower());
                if let Some(upper) = size_hint.upper() {
                    out.set_upper(upper);
                }
                out
            }
        }
    }

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body_v1::Frame<Self::Data>, Self::Error>>> {
        match self.project() {
            BodyProj::EmptyResponse => Poll::Ready(None),
            BodyProj::Body { body } => body.poll_frame(cx),
            BodyProj::CustomBody { mut body } => {
                match body.as_mut().poll_data(cx) {
                    Poll::Ready(Some(Ok(buf))) => {
                        return Poll::Ready(Some(Ok(http_body_v1::Frame::data(buf))))
                    }
                    Poll::Ready(Some(Err(_))) => unreachable!("unreachable error!"),
                    Poll::Ready(None) => {}
                    Poll::Pending => return Poll::Pending,
                }

                match body.as_mut().poll_trailers(cx) {
                    Poll::Ready(Ok(Some(trailers))) => {
                        Poll::Ready(Some(Ok(http_body_v1::Frame::trailers(trailers))))
                    }
                    Poll::Ready(Ok(None)) => Poll::Ready(None),
                    Poll::Ready(Err(_)) => unreachable!("unreachable error!"),
                    Poll::Pending => Poll::Pending,
                }
            }
        }
    }
}

/// A body that is always empty.
#[cfg(feature = "hyper-v1")]
pub struct Empty<D> {
    _marker: std::marker::PhantomData<fn() -> D>,
}

impl<D> Empty<D> {
    /// Create a new `Empty`.
    pub fn new() -> Self {
        Self::default()
    }
}

impl<D: bytes::Buf> http_body_v1::Body for Empty<D> {
    type Data = D;
    type Error = Infallible;

    #[inline]
    fn poll_frame(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body_v1::Frame<Self::Data>, Self::Error>>> {
        Poll::Ready(None)
    }

    fn is_end_stream(&self) -> bool {
        true
    }

    fn size_hint(&self) -> http_body_v1::SizeHint {
        http_body_v1::SizeHint::with_exact(0)
    }
}

impl<D> std::fmt::Debug for Empty<D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Empty").finish()
    }
}

impl<D> Default for Empty<D> {
    fn default() -> Self {
        Self {
            _marker: std::marker::PhantomData,
        }
    }
}
