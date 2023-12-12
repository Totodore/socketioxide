//! Response Body wrapper in order to return a custom body or the body from the inner service

use bytes::Bytes;
use http_body::{Body, Frame, SizeHint};
use http_body_util::Full;
use pin_project::pin_project;
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

/// Implementation heavily inspired from https://github.com/davidpdrsn/tower-hyper-http-body-compat
impl<B> Body for ResponseBody<B>
where
    B: Body<Data = Bytes>,
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

    fn size_hint(&self) -> SizeHint {
        match &self {
            ResponseBody::EmptyResponse => {
                let mut hint = SizeHint::default();
                hint.set_upper(0);
                hint
            }
            ResponseBody::Body { body } => body.size_hint(),
            ResponseBody::CustomBody { body } => body.size_hint(),
        }
    }

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        match self.project() {
            BodyProj::EmptyResponse => Poll::Ready(None),
            BodyProj::Body { body } => body.poll_frame(cx),
            BodyProj::CustomBody { body } => body.poll_frame(cx).map_err(|err| match err {}),
        }
    }
}

#[cfg(feature = "hyper-legacy")]
impl<B> http_body_legacy::Body for ResponseBody<B>
where
    B: Body<Data = Bytes>,
    B::Error: std::error::Error + 'static,
{
    type Data = B::Data;

    type Error = B::Error;

    /// We don't support trailers
    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        let frame = match self.project() {
            BodyProj::EmptyResponse => Poll::Ready(None),
            BodyProj::Body { body } => body.poll_frame(cx),
            BodyProj::CustomBody { body } => body.poll_frame(cx).map_err(|err| match err {}),
        };
        let frame = futures::ready!(frame);
        match frame {
            Some(Ok(frame)) => match frame.into_data() {
                Ok(data) => return Poll::Ready(Some(Ok(data))),
                Err(_) => Poll::Ready(None),
            },
            Some(Err(err)) => Poll::Ready(Some(Err(err))),
            None => Poll::Ready(None),
        }
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Result<Option<hyper_legacy::http::HeaderMap>, Self::Error>> {
        Poll::Ready(Ok(None))
    }
}
