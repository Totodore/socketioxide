use bytes::Bytes;
use http::HeaderMap;
use http_body::{Body, Full, SizeHint};
use pin_project::pin_project;
use std::pin::Pin;
use std::task::{Context, Poll};
#[pin_project]
pub struct ResponseBody<B> {
    #[pin]
    inner: ResponseBodyInner<B>,
}

impl<B> ResponseBody<B> {
    pub fn empty_response() -> Self {
        Self {
            inner: ResponseBodyInner::EmptyResponse,
        }
    }

    pub fn custom_response(body: Full<Bytes>) -> Self {
        Self {
            inner: ResponseBodyInner::CustomBody { body },
        }
    }

    pub fn new(body: B) -> Self {
        Self {
            inner: ResponseBodyInner::Body { body },
        }
    }
}

impl<B> Default for ResponseBody<B> {
    fn default() -> Self {
        Self::empty_response()
    }
}

#[pin_project(project = BodyProj)]
enum ResponseBodyInner<B> {
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
        match self.project().inner.project() {
            BodyProj::EmptyResponse => Poll::Ready(None),
            BodyProj::Body { body } => body.poll_data(cx),
            BodyProj::CustomBody { body } => body.poll_data(cx).map_err(|err| match err {}),
        }
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<HeaderMap>, Self::Error>> {
        match self.project().inner.project() {
            BodyProj::EmptyResponse => Poll::Ready(Ok(None)),
            BodyProj::Body { body } => body.poll_trailers(cx),
            BodyProj::CustomBody { body } => body.poll_trailers(cx).map_err(|err| match err {}),
        }
    }

    fn is_end_stream(&self) -> bool {
        match &self.inner {
            ResponseBodyInner::EmptyResponse => true,
            ResponseBodyInner::Body { body } => body.is_end_stream(),
            ResponseBodyInner::CustomBody { body } => body.is_end_stream(),
        }
    }

    fn size_hint(&self) -> SizeHint {
        match &self.inner {
            ResponseBodyInner::EmptyResponse => {
                let mut hint = SizeHint::default();
                hint.set_upper(0);
                hint
            }
            ResponseBodyInner::Body { body } => body.size_hint(),
            ResponseBodyInner::CustomBody { body } => body.size_hint(),
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
        match &self.inner {
            ResponseBodyInner::EmptyResponse => true,
            ResponseBodyInner::Body { body } => body.is_end_stream(),
            ResponseBodyInner::CustomBody { body } => body.is_end_stream(),
        }
    }

    fn size_hint(&self) -> http_body_v1::SizeHint {
        match &self.inner {
            ResponseBodyInner::EmptyResponse => {
                let mut hint = http_body_v1::SizeHint::default();
                hint.set_upper(0);
                hint
            }
            ResponseBodyInner::Body { body } => body.size_hint(),
            ResponseBodyInner::CustomBody { body } => {
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
        match self.project().inner.project() {
            BodyProj::EmptyResponse => Poll::Ready(None),
            BodyProj::Body { body } => body.poll_frame(cx),
            BodyProj::CustomBody { body } => {
                match body.poll_data(cx) {
                    Poll::Ready(Some(Ok(buf))) => return Poll::Ready(Some(Ok(Frame::data(buf)))),
                    Poll::Ready(Some(Err(err))) => unreachable!("unreachable error!"),
                    Poll::Ready(None) => {}
                    Poll::Pending => return Poll::Pending,
                }

                match body.poll_trailers(cx) {
                    Poll::Ready(Ok(Some(trailers))) => {
                        Poll::Ready(Some(Ok(Frame::trailers(trailers))))
                    }
                    Poll::Ready(Ok(None)) => Poll::Ready(None),
                    Poll::Ready(Err(err)) => unreachable!("unreachable error!"),
                    Poll::Pending => Poll::Pending,
                }
            }
        }
    }
}
