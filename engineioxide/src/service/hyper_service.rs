//! Implement Services for hyper 1.0
//! Only enabled with feature flag `hyper-v1`
use crate::{
    body::{request::IncomingBody, response::ResponseBody},
    handler::EngineIoHandler,
};
use bytes::Bytes;
use http::Request;
use http_body::Empty;
use hyper::Response;
use hyper_v1::body::Incoming;
use std::{
    convert::Infallible,
    future::{ready, Ready},
    task::{Context, Poll},
};

use super::{futures::ResponseFuture, EngineIoService, MakeEngineIoService, NotFoundService};

/// Tower Service implementation with an [`Incoming`] body and a [`http_body_v1::Body`] Body for hyper 1.0
impl<ReqBody, ResBody, S, H> tower::Service<Request<ReqBody>> for EngineIoService<H, S>
where
    ResBody: http_body_v1::Body + http_body::Body + Send + 'static,
    ReqBody: http_body_v1::Body + Send + Unpin + 'static + std::fmt::Debug,
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
            let req = req.map(IncomingBody::new);
            self.dispatch_req(req)
        } else {
            ResponseFuture::new(self.inner.call(req))
        }
    }
}

/// Hyper 1.0 Service implementation with an [`Incoming`] body and a [`http_body_v1::Body`] Body
impl<ResBody, S, H> hyper_v1::service::Service<Request<Incoming>> for EngineIoService<H, S>
where
    ResBody: http_body_v1::Body + http_body::Body + Send + 'static,
    S: hyper_v1::service::Service<Request<Incoming>, Response = Response<ResBody>>,
    H: EngineIoHandler,
{
    type Response = Response<ResponseBody<ResBody>>;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future, ResBody>;

    fn call(&self, req: Request<Incoming>) -> Self::Future {
        if req.uri().path().starts_with(&self.engine.config.req_path) {
            let req = req.map(IncomingBody::new);
            self.dispatch_req(req)
        } else {
            ResponseFuture::new(self.inner.call(req))
        }
    }
}

impl tower::Service<Request<Incoming>> for NotFoundService {
    type Response = Response<ResponseBody<Empty<Bytes>>>;
    type Error = Infallible;
    type Future = Ready<Result<Response<ResponseBody<Empty<Bytes>>>, Infallible>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _: Request<Incoming>) -> Self::Future {
        ready(Ok(Response::builder()
            .status(404)
            .body(ResponseBody::empty_response())
            .unwrap()))
    }
}

impl hyper_v1::service::Service<Request<Incoming>> for NotFoundService {
    type Response = Response<ResponseBody<Empty<Bytes>>>;
    type Error = Infallible;
    type Future = Ready<Result<Response<ResponseBody<Empty<Bytes>>>, Infallible>>;

    fn call(&self, _: Request<Incoming>) -> Self::Future {
        ready(Ok(Response::builder()
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
        ready(Ok(self.svc.clone()))
    }
}

impl<H: EngineIoHandler, S: Clone, T> hyper_v1::service::Service<T> for MakeEngineIoService<H, S> {
    type Response = EngineIoService<H, S>;

    type Error = Infallible;

    type Future = Ready<Result<Self::Response, Self::Error>>;

    fn call(&self, _req: T) -> Self::Future {
        ready(Ok(self.svc.clone()))
    }
}
