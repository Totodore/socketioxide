use crate::{
    body::ResponseBody, config::EngineIoConfig, engine::EngineIo, futures::ResponseFuture,
    handler::EngineIoHandler,
};
use bytes::Bytes;
use futures::future::{ready, Ready};
use http::{Method, Request};
use http_body::{Body, Empty};
use hyper::{service::Service, Response};
use std::{
    convert::Infallible,
    fmt::Debug,
    str::FromStr,
    sync::Arc,
    task::{Context, Poll},
};

/// A [`Service`] that handles `EngineIo` requests as a middleware.
/// If the request is not an `EngineIo` request, it forwards it to the inner service.
/// It is agnostic to the [`TransportType`](crate::service::TransportType).
///
/// By default, it uses a [`NotFoundService`] as the inner service so it can be used as a standalone [`Service`].
pub struct EngineIoService<H, S = NotFoundService>
where
    H: EngineIoHandler + ?Sized,
{
    inner: S,
    engine: Arc<EngineIo<H>>,
}
impl<H> EngineIoService<H, NotFoundService>
where
    H: EngineIoHandler + ?Sized,
{
    /// Create a new [`EngineIoService`] with a [`NotFoundService`] as the inner service.
    /// If the request is not an `EngineIo` request, it will always return a 404 response.
    pub fn new(handler: Arc<H>) -> Self {
        EngineIoService {
            inner: NotFoundService,
            engine: Arc::new(EngineIo::new(handler)),
        }
    }
    /// Create a new [`EngineIoService`] with a custom config
    pub fn with_config(handler: Arc<H>, config: EngineIoConfig) -> Self {
        EngineIoService {
            inner: NotFoundService,
            engine: Arc::new(EngineIo::from_config(handler, config)),
        }
    }
}
impl<S, H> EngineIoService<H, S>
where
    H: EngineIoHandler + ?Sized,
    S: Clone,
{
    /// Create a new [`EngineIoService`] with a custom inner service and a custom config.
    pub fn with_config_inner(inner: S, handler: Arc<H>, config: EngineIoConfig) -> Self {
        EngineIoService {
            inner,
            engine: Arc::new(EngineIo::from_config(handler, config)),
        }
    }

    /// Convert this [`EngineIoService`] into a [`MakeEngineIoService`].
    /// This is useful when using [`EngineIoService`] without layers.
    pub fn into_make_service(self) -> MakeEngineIoService<H, S> {
        MakeEngineIoService::new(self)
    }
}

impl<S, H> Clone for EngineIoService<H, S>
where
    H: EngineIoHandler + ?Sized,
    S: Clone,
{
    fn clone(&self) -> Self {
        EngineIoService {
            inner: self.inner.clone(),
            engine: self.engine.clone(),
        }
    }
}

/// The service implementation for [`EngineIoService`].
impl<ReqBody, ResBody, S, H> Service<Request<ReqBody>> for EngineIoService<H, S>
where
    ResBody: Body + Send + 'static,
    ReqBody: http_body::Body + Send + 'static + Debug,
    <ReqBody as http_body::Body>::Error: Debug,
    <ReqBody as http_body::Body>::Data: Send,
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    H: EngineIoHandler + ?Sized,
{
    type Response = Response<ResponseBody<ResBody>>;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future, ResBody>;

    fn poll_ready(&mut self, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    /// Handle the request.
    /// Each request is parsed to extract the [`TransportType`](crate::service::TransportType) and the socket id.
    /// If the request is an `EngineIo` request, it is handled by the `EngineIo` engine.
    /// Otherwise, it is forwarded to the inner service.
    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        if req.uri().path().starts_with(&self.engine.config.req_path) {
            let engine = self.engine.clone();
            match RequestInfo::parse(&req) {
                Some(RequestInfo {
                    sid: None,
                    transport: TransportType::Polling,
                    method: Method::GET,
                }) => ResponseFuture::ready(engine.on_open_http_req(req)),
                Some(RequestInfo {
                    sid: Some(sid),
                    transport: TransportType::Polling,
                    method: Method::GET,
                }) => ResponseFuture::async_response(Box::pin(engine.on_polling_http_req(sid))),
                Some(RequestInfo {
                    sid: Some(sid),
                    transport: TransportType::Polling,
                    method: Method::POST,
                }) => ResponseFuture::async_response(Box::pin(engine.on_post_http_req(sid, req))),
                Some(RequestInfo {
                    sid,
                    transport: TransportType::Websocket,
                    method: Method::GET,
                }) => ResponseFuture::ready(engine.on_ws_req(sid, req)),
                _ => ResponseFuture::empty_response(400),
            }
        } else {
            ResponseFuture::new(self.inner.call(req))
        }
    }
}

/// A MakeService that always returns a clone of the [`EngineIoService`] it was created with.
pub struct MakeEngineIoService<H, S>
where
    H: EngineIoHandler + ?Sized,
{
    svc: EngineIoService<H, S>,
}

impl<H, S> MakeEngineIoService<H, S>
where
    H: EngineIoHandler + ?Sized,
{
    /// Create a new [`MakeEngineIoService`] with a custom inner service.
    pub fn new(svc: EngineIoService<H, S>) -> Self {
        MakeEngineIoService { svc }
    }
}

impl<H, S, T> Service<T> for MakeEngineIoService<H, S>
where
    H: EngineIoHandler,
    S: Clone,
{
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

/// A [`Service`] that always returns a 404 response and that is compatible with [`EngineIoService`].
#[derive(Debug, Clone)]
pub struct NotFoundService;
impl<ReqBody> Service<Request<ReqBody>> for NotFoundService
where
    ReqBody: http_body::Body + Send + 'static + Debug,
    <ReqBody as http_body::Body>::Error: Debug,
    <ReqBody as http_body::Body>::Data: Send,
{
    type Response = Response<ResponseBody<Empty<Bytes>>>;
    type Error = Infallible;
    type Future = Ready<Result<Response<ResponseBody<Empty<Bytes>>>, Infallible>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _: Request<ReqBody>) -> Self::Future {
        ready(Ok(Response::builder()
            .status(404)
            .body(ResponseBody::empty_response())
            .unwrap()))
    }
}

/// The type of the transport used by the client.
#[derive(Debug, PartialEq)]
pub enum TransportType {
    Websocket,
    Polling,
}

impl FromStr for TransportType {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "websocket" => Ok(TransportType::Websocket),
            "polling" => Ok(TransportType::Polling),
            _ => Err(()),
        }
    }
}

/// The request information extracted from the request URI.
struct RequestInfo {
    /// The socket id if present in the request.
    sid: Option<i64>,
    /// The transport type used by the client.
    transport: TransportType,
    /// The request method.
    method: Method,
}

impl RequestInfo {
    /// Parse the request URI to extract the [`TransportType`](crate::service::TransportType) and the socket id.
    fn parse<B>(req: &Request<B>) -> Option<Self> {
        let query = req.uri().query()?;
        if !query.contains("EIO=4") {
            return None;
        }

        let sid = query
            .split('&')
            .find(|s| s.starts_with("sid="))
            .and_then(|s| s.split('=').nth(1).map(|s1| s1.parse().ok()))
            .flatten();

        let transport: TransportType = query
            .split('&')
            .find(|s| s.starts_with("transport="))?
            .split('=')
            .nth(1)?
            .parse()
            .ok()?;

        Some(RequestInfo {
            sid,
            transport,
            method: req.method().clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_request(path: &str) -> Request<()> {
        Request::get(path).body(()).unwrap()
    }

    #[test]
    fn request_info_polling() {
        let req = build_request("http://localhost:3000/socket.io/?EIO=4&transport=polling");
        let info = RequestInfo::parse(&req).unwrap();
        assert_eq!(info.sid, None);
        assert_eq!(info.transport, TransportType::Polling);
        assert_eq!(info.method, Method::GET);
    }

    #[test]
    fn request_info_websocket() {
        let req = build_request("http://localhost:3000/socket.io/?EIO=4&transport=websocket");
        let info = RequestInfo::parse(&req).unwrap();
        assert_eq!(info.sid, None);
        assert_eq!(info.transport, TransportType::Websocket);
        assert_eq!(info.method, Method::GET);
    }

    #[test]
    fn request_info_polling_with_sid() {
        let req = build_request("http://localhost:3000/socket.io/?EIO=4&transport=polling&sid=123");
        let info = RequestInfo::parse(&req).unwrap();
        assert_eq!(info.sid, Some(123));
        assert_eq!(info.transport, TransportType::Polling);
        assert_eq!(info.method, Method::GET);
    }

    #[test]
    fn request_info_websocket_with_sid() {
        let req =
            build_request("http://localhost:3000/socket.io/?EIO=4&transport=websocket&sid=123");
        let info = RequestInfo::parse(&req).unwrap();
        assert_eq!(info.sid, Some(123));
        assert_eq!(info.transport, TransportType::Websocket);
        assert_eq!(info.method, Method::GET);
    }
}
