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
            BodyProj::EmptyResponse => std::task::Poll::Ready(None),
            BodyProj::Body { body } => body.poll_data(cx),
            BodyProj::CustomBody { body } => body.poll_data(cx).map_err(|err| match err {})
        }
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<HeaderMap>, Self::Error>> {
        match self.project().inner.project() {
            BodyProj::EmptyResponse => std::task::Poll::Ready(Ok(None)),
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
