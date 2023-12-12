//! Eavily inspired from : [davidpdrsn/tower-hyper-http-body-compat](https://github.com/davidpdrsn/tower-hyper-http-body-compat)
use std::{
    convert::Infallible,
    pin::Pin,
    task::{Context, Poll},
};

use crate::{body::ResponseBody, handler::EngineIoHandler};

use super::{futures::ResponseFuture, parser::dispatch_req, EngineIoService, NotFoundService};
use futures::future::{self, Ready};
use http_body_legacy::Body;
use hyper_legacy::http::{Request, Response};
use pin_project::pin_project;
use tower::Service;

/// A wrapper of [`EngineIoService`] that handles engine.io requests as a middleware for `hyper-v1`.
pub struct EngineIoLegacyService<H: EngineIoHandler, S = NotFoundService>(EngineIoService<H, S>);

#[pin_project]
struct IncomingBody<B>(#[pin] B);
impl<B> http_body::Body for IncomingBody<B>
where
    B: Body,
{
    type Data = B::Data;

    type Error = B::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        match self.as_mut().project().0.poll_data(cx) {
            Poll::Ready(Some(Ok(buf))) => Poll::Ready(Some(Ok(http_body::Frame::data(buf)))),
            Poll::Ready(Some(Err(err))) => Poll::Ready(Some(Err(err))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }

    fn size_hint(&self) -> http_body::SizeHint {
        let size_hint = self.0.size_hint();
        let mut out = http_body::SizeHint::new();
        out.set_lower(size_hint.lower());
        if let Some(upper) = size_hint.upper() {
            out.set_upper(upper);
        }
        out
    }

    #[inline]
    fn is_end_stream(&self) -> bool {
        self.0.is_end_stream()
    }
}

impl<H: EngineIoHandler, S> EngineIoLegacyService<H, S> {
    pub(crate) fn new(svc: EngineIoService<H, S>) -> Self {
        EngineIoLegacyService(svc)
    }

    /// Convert this [`EngineIoLegacyService`] into a [`MakeEngineIoLegacyService`].
    /// This is useful when using [`EngineIoLegacyService`] without layers.
    pub fn into_make_service(self) -> MakeEngineIoLegacyService<H, S> {
        MakeEngineIoLegacyService::new(self)
    }
}

impl<H: EngineIoHandler, S: Clone> Clone for EngineIoLegacyService<H, S> {
    fn clone(&self) -> Self {
        EngineIoLegacyService(self.0.clone())
    }
}
/// Tower [`Service`] implementation for [`EngineIoService`].
impl<ReqBody, ResBody, S, H> Service<Request<ReqBody>> for EngineIoLegacyService<H, S>
where
    ResBody: Body + Send + 'static,
    ReqBody: Body + Send + Unpin + 'static + std::fmt::Debug,
    ReqBody::Error: std::fmt::Debug,
    ReqBody::Data: Send,
    S: tower::Service<Request<ReqBody>, Response = Response<ResBody>>,
    H: EngineIoHandler,
{
    type Response = Response<ResponseBody<ResBody>>;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future, ResBody>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.0.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let path = self.0.engine.config.req_path.as_ref();
        if req.uri().path().starts_with(path) {
            let req = req.map(IncomingBody);
            dispatch_req(req, self.0.engine.clone())
        } else {
            ResponseFuture::new(self.0.inner.call(req))
        }
    }
}

/// A MakeService that always returns a clone of the [`EngineIoService`] it was created with.
pub struct MakeEngineIoLegacyService<H: EngineIoHandler, S> {
    svc: EngineIoLegacyService<H, S>,
}

impl<H: EngineIoHandler, S> MakeEngineIoLegacyService<H, S> {
    /// Create a new [`MakeEngineIoService`] with a custom inner service.
    pub fn new(svc: EngineIoLegacyService<H, S>) -> Self {
        MakeEngineIoLegacyService { svc }
    }
}

impl<H: EngineIoHandler, S: Clone, T> Service<T> for MakeEngineIoLegacyService<H, S> {
    type Response = EngineIoLegacyService<H, S>;

    type Error = Infallible;

    type Future = Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _req: T) -> Self::Future {
        future::ready(Ok(self.svc.clone()))
    }
}
