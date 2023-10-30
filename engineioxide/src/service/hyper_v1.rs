//! Implement Services for hyper 1.0
//! Only enabled with feature flag `hyper-v1`
use crate::{
    body::{
        request::IncomingBody,
        response::{Empty, ResponseBody},
    },
    handler::EngineIoHandler,
};
use bytes::Bytes;
use http::Request;
use hyper::Response;
use hyper_v1::body::Incoming;
use std::{
    convert::Infallible,
    future::{ready, Ready},
    task::{Context, Poll},
};

use super::{futures::ResponseFuture, parser::dispatch_req, EngineIoService, NotFoundService};

/// A wrapper of [`EngineIoService`] that handles engine.io requests as a middleware from hyper-v1.
pub struct EngineIoHyperService<H: EngineIoHandler, S = NotFoundService>(EngineIoService<H, S>);
impl<H, S> EngineIoHyperService<H, S>
where
    H: EngineIoHandler,
{
    pub(crate) fn new(svc: EngineIoService<H, S>) -> Self {
        EngineIoHyperService(svc)
    }
}

/// Tower Service implementation with an [`Incoming`] body and a [`http_body_v1::Body`] Body for hyper 1.0
impl<ReqBody, ResBody, S, H> tower::Service<Request<ReqBody>> for EngineIoHyperService<H, S>
where
    ResBody: http_body_v1::Body + Send + 'static,
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
        self.0.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        if req.uri().path().starts_with(&self.0.engine.config.req_path) {
            let req = req.map(IncomingBody::new);
            dispatch_req(req, self.0.engine.clone())
        } else {
            ResponseFuture::new(self.0.inner.call(req))
        }
    }
}

/// Hyper 1.0 Service implementation with an [`Incoming`] body and a [`http_body_v1::Body`] Body
impl<ResBody, S, H> hyper_v1::service::Service<Request<Incoming>> for EngineIoHyperService<H, S>
where
    ResBody: http_body_v1::Body + Send + 'static,
    S: hyper_v1::service::Service<Request<Incoming>, Response = Response<ResBody>>,
    H: EngineIoHandler,
{
    type Response = Response<ResponseBody<ResBody>>;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future, ResBody>;

    fn call(&self, req: Request<Incoming>) -> Self::Future {
        if req.uri().path().starts_with(&self.0.engine.config.req_path) {
            let req = req.map(IncomingBody::new);
            dispatch_req(req, self.0.engine.clone())
        } else {
            ResponseFuture::new(self.0.inner.call(req))
        }
    }
}

impl<H: EngineIoHandler, S: Clone> Clone for EngineIoHyperService<H, S> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<H: EngineIoHandler, S> std::fmt::Debug for EngineIoHyperService<H, S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("EngineIoHyperService")
            .field(&self.0)
            .finish()
    }
}

/// A [`Service`] that always returns a 404 response and that is compatible with [`EngineIoHyperService`].
pub struct HyperNotFoundService;

impl tower::Service<Request<Incoming>> for HyperNotFoundService {
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

impl hyper_v1::service::Service<Request<Incoming>> for HyperNotFoundService {
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
