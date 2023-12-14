//! Eavily inspired from : [davidpdrsn/tower-hyper-http-body-compat](https://github.com/davidpdrsn/tower-hyper-http-body-compat)
use std::{
    convert::Infallible,
    str::FromStr,
    task::{Context, Poll},
};

use crate::{body::ResponseBody, handler::EngineIoHandler};

use self::futures::LegacyResponseFuture;

use super::{futures::ResponseFuture, parser::dispatch_req, EngineIoService, NotFoundService};
use ::futures::future::{self, Ready};
use http_body_legacy::Body;
use hyper_legacy::http::{Request, Response};
use tower::Service;

mod body;
mod futures;

/// A wrapper of [`EngineIoService`] that handles engine.io requests as a middleware for `hyper-v1`.
pub struct EngineIoLegacyService<H: EngineIoHandler, S = NotFoundService>(EngineIoService<H, S>);

/// Map a [`hyper_legacy::http::Request`] to a current [`http::Request`].
fn map_req<R>(req: Request<R>) -> http::Request<R> {
    let (mut parts, body) = req.into_parts();
    let mut req = http::Request::builder()
        .method(http::Method::from_str(parts.method.as_str()).unwrap())
        .uri(http::Uri::from_str(&parts.uri.to_string()).unwrap())
        .version(match parts.version {
            hyper_legacy::Version::HTTP_10 => http::Version::HTTP_10,
            hyper_legacy::Version::HTTP_11 => http::Version::HTTP_11,
            _ => unreachable!(),
        })
        .body(body)
        .unwrap();

    for (k, v) in parts.headers.iter() {
        let v = http::header::HeaderValue::from_bytes(v.as_bytes()).unwrap();
        req.headers_mut().insert(k.as_str(), v);
    }
    req
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
    type Future = LegacyResponseFuture<S::Future, ResBody>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.0.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let path = self.0.engine.config.req_path.as_ref();
        if req.uri().path().starts_with(path) {
            let req = map_req(req).map(body::IncomingBody::new);
            let res = dispatch_req(req, self.0.engine.clone());
            LegacyResponseFuture::new(res)
        } else {
            let res = self.0.inner.call(req);
            LegacyResponseFuture::new(ResponseFuture::new(res))
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
