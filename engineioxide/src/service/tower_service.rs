//! Implement Services for tower 0.3 and hyper 0.4
//! Enabled by default and disabled with feature flag `hyper-v1` enabled
use std::{
    convert::Infallible,
    task::{Context, Poll},
};

use bytes::Bytes;
use futures::future::{self, Ready};
use http::{Request, Response};
use http_body::{Body, Empty};

use crate::{body::response::ResponseBody, handler::EngineIoHandler};

use super::{futures::ResponseFuture, EngineIoService, MakeEngineIoService, NotFoundService};

/// The service implementation for [`EngineIoService`].
impl<ReqBody, ResBody, S, H> tower::Service<Request<ReqBody>> for EngineIoService<H, S>
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

    fn poll_ready(&mut self, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        if req.uri().path().starts_with(&self.engine.config.req_path) {
            self.dispatch_req(req)
        } else {
            ResponseFuture::new(self.inner.call(req))
        }
    }
}

impl<ReqBody> tower::Service<Request<ReqBody>> for NotFoundService
where
    ReqBody: Body + Send + 'static + std::fmt::Debug,
    <ReqBody as Body>::Error: std::fmt::Debug,
    <ReqBody as Body>::Data: Send,
{
    type Response = Response<ResponseBody<Empty<Bytes>>>;
    type Error = Infallible;
    type Future = Ready<Result<Response<ResponseBody<Empty<Bytes>>>, Infallible>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _: Request<ReqBody>) -> Self::Future {
        future::ready(Ok(Response::builder()
            .status(404)
            .body(ResponseBody::empty_response())
            .unwrap()))
    }
}

impl<H: EngineIoHandler, S: Clone, T> tower::Service<T> for MakeEngineIoService<H, S> {
    type Response = EngineIoService<H, S>;

    type Error = Infallible;

    type Future = Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _req: T) -> Self::Future {
        future::ready(Ok(self.svc.clone()))
    }
}
