use crate::{
    body::ResponseBody,
    config::EngineIoConfig,
    engine::EngineIo,
    errors::{Error, Error::UnknownTransport},
    futures::ResponseFuture,
    handler::EngineIoHandler,
    sid_generator::Sid,
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
pub struct EngineIoService<H: EngineIoHandler, S = NotFoundService> {
    inner: S,
    engine: Arc<EngineIo<H>>,
}

impl<H: EngineIoHandler> EngineIoService<H, NotFoundService> {
    /// Create a new [`EngineIoService`] with a [`NotFoundService`] as the inner service.
    /// If the request is not an `EngineIo` request, it will always return a 404 response.
    pub fn new(handler: H) -> Self {
        EngineIoService::with_config(handler, EngineIoConfig::default())
    }
    /// Create a new [`EngineIoService`] with a custom config
    pub fn with_config(handler: H, config: EngineIoConfig) -> Self {
        EngineIoService::with_config_inner(NotFoundService, handler, config)
    }
}
impl<S: Clone, H: EngineIoHandler> EngineIoService<H, S> {
    /// Create a new [`EngineIoService`] with a custom inner service.
    pub fn with_inner(inner: S, handler: H) -> Self {
        EngineIoService::with_config_inner(inner, handler, EngineIoConfig::default())
    }

    /// Create a new [`EngineIoService`] with a custom inner service and a custom config.
    pub fn with_config_inner(inner: S, handler: H, config: EngineIoConfig) -> Self {
        EngineIoService {
            inner,
            engine: Arc::new(EngineIo::new(handler, config)),
        }
    }

    /// Convert this [`EngineIoService`] into a [`MakeEngineIoService`].
    /// This is useful when using [`EngineIoService`] without layers.
    pub fn into_make_service(self) -> MakeEngineIoService<H, S> {
        MakeEngineIoService::new(self)
    }
}

impl<S: Clone, H: EngineIoHandler> Clone for EngineIoService<H, S> {
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
    ReqBody: http_body::Body + Send + Unpin + 'static + Debug,
    <ReqBody as http_body::Body>::Error: Debug,
    <ReqBody as http_body::Body>::Data: Send,
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    H: EngineIoHandler,
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
                Ok(RequestInfo {
                    protocol,
                    sid: None,
                    transport: TransportType::Polling,
                    method: Method::GET,
                    #[cfg(feature = "v3")]
                    b64,
                }) => ResponseFuture::ready(engine.on_open_http_req(
                    protocol,
                    req,
                    #[cfg(feature = "v3")]
                    !b64,
                )),
                Ok(RequestInfo {
                    protocol,
                    sid: Some(sid),
                    transport: TransportType::Polling,
                    method: Method::GET,
                    ..
                }) => ResponseFuture::async_response(Box::pin(
                    engine.on_polling_http_req(protocol, sid),
                )),
                Ok(RequestInfo {
                    protocol,
                    sid: Some(sid),
                    transport: TransportType::Polling,
                    method: Method::POST,
                    ..
                }) => ResponseFuture::async_response(Box::pin(
                    engine.on_post_http_req(protocol, sid, req),
                )),
                Ok(RequestInfo {
                    protocol,
                    sid,
                    transport: TransportType::Websocket,
                    method: Method::GET,
                    ..
                }) => ResponseFuture::ready(engine.on_ws_req(protocol, sid, req)),
                Err(e) => ResponseFuture::ready(Ok(e.into())),
                _ => ResponseFuture::empty_response(400),
            }
        } else {
            ResponseFuture::new(self.inner.call(req))
        }
    }
}

impl<H: EngineIoHandler, S> Debug for EngineIoService<H, S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EngineIoService").finish()
    }
}

/// A MakeService that always returns a clone of the [`EngineIoService`] it was created with.
pub struct MakeEngineIoService<H: EngineIoHandler, S> {
    svc: EngineIoService<H, S>,
}

impl<H: EngineIoHandler, S> MakeEngineIoService<H, S> {
    /// Create a new [`MakeEngineIoService`] with a custom inner service.
    pub fn new(svc: EngineIoService<H, S>) -> Self {
        MakeEngineIoService { svc }
    }
}

impl<H: EngineIoHandler, S: Clone, T> Service<T> for MakeEngineIoService<H, S> {
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
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "websocket" => Ok(TransportType::Websocket),
            "polling" => Ok(TransportType::Polling),
            _ => Err(UnknownTransport),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum ProtocolVersion {
    V3 = 3,
    V4 = 4,
}

impl FromStr for ProtocolVersion {
    type Err = Error;

    #[cfg(all(feature = "v3", feature = "v4"))]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "3" => Ok(ProtocolVersion::V3),
            "4" => Ok(ProtocolVersion::V4),
            _ => Err(Error::UnsupportedProtocolVersion),
        }
    }

    #[cfg(feature = "v4")]
    #[cfg(not(feature = "v3"))]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "4" => Ok(ProtocolVersion::V4),
            _ => Err(Error::UnsupportedProtocolVersion),
        }
    }

    #[cfg(feature = "v3")]
    #[cfg(not(feature = "v4"))]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "3" => Ok(ProtocolVersion::V3),
            _ => Err(Error::UnsupportedProtocolVersion),
        }
    }
}

/// The request information extracted from the request URI.
#[derive(Debug)]
struct RequestInfo {
    /// The protocol version used by the client.
    protocol: ProtocolVersion,
    /// The socket id if present in the request.
    sid: Option<Sid>,
    /// The transport type used by the client.
    transport: TransportType,
    /// The request method.
    method: Method,
    /// If the client asked for base64 encoding only.
    #[cfg(feature = "v3")]
    b64: bool,
}

impl RequestInfo {
    /// Parse the request URI to extract the [`TransportType`](crate::service::TransportType) and the socket id.
    fn parse<B>(req: &Request<B>) -> Result<Self, Error> {
        let query = req.uri().query().ok_or(UnknownTransport)?;

        let protocol: ProtocolVersion = query
            .split('&')
            .find(|s| s.starts_with("EIO="))
            .and_then(|s| s.split('=').nth(1))
            .ok_or(UnknownTransport)
            .and_then(|t| t.parse())?;

        let sid = query
            .split('&')
            .find(|s| s.starts_with("sid="))
            .and_then(|s| s.split('=').nth(1).map(|s1| s1.parse().ok()))
            .flatten();

        let transport: TransportType = query
            .split('&')
            .find(|s| s.starts_with("transport="))
            .and_then(|s| s.split('=').nth(1))
            .ok_or(UnknownTransport)
            .and_then(|t| t.parse())?;

        #[cfg(feature = "v3")]
        let b64: bool = query
            .split('&')
            .find(|s| s.starts_with("b64="))
            .map(|_| true)
            .unwrap_or_default();

        let method = req.method().clone();
        if !matches!(method, Method::GET) && sid.is_none() {
            Err(Error::BadHandshakeMethod)
        } else {
            Ok(RequestInfo {
                protocol,
                sid,
                transport,
                method,
                #[cfg(feature = "v3")]
                b64,
            })
        }
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
        assert_eq!(info.protocol, ProtocolVersion::V4);
        assert_eq!(info.method, Method::GET);
    }

    #[test]
    fn request_info_websocket() {
        let req = build_request("http://localhost:3000/socket.io/?EIO=4&transport=websocket");
        let info = RequestInfo::parse(&req).unwrap();
        assert_eq!(info.sid, None);
        assert_eq!(info.transport, TransportType::Websocket);
        assert_eq!(info.protocol, ProtocolVersion::V4);
        assert_eq!(info.method, Method::GET);
    }

    #[test]
    fn request_info_polling_with_sid() {
        let req = build_request(
            "http://localhost:3000/socket.io/?EIO=3&transport=polling&sid=AAAAAAAAAHs",
        );
        let info = RequestInfo::parse(&req).unwrap();
        assert_eq!(info.sid, Some(123i64.into()));
        assert_eq!(info.transport, TransportType::Polling);
        assert_eq!(info.protocol, ProtocolVersion::V3);
        assert_eq!(info.method, Method::GET);
    }

    #[test]
    fn request_info_websocket_with_sid() {
        let req = build_request(
            "http://localhost:3000/socket.io/?EIO=3&transport=websocket&sid=AAAAAAAAAHs",
        );
        let info = RequestInfo::parse(&req).unwrap();
        assert_eq!(info.sid, Some(123i64.into()));
        assert_eq!(info.transport, TransportType::Websocket);
        assert_eq!(info.protocol, ProtocolVersion::V3);
        assert_eq!(info.method, Method::GET);
    }

    #[test]
    #[cfg(feature = "v3")]
    fn request_info_polling_with_bin_by_default() {
        let req = build_request("http://localhost:3000/socket.io/?EIO=3&transport=polling");
        let req = RequestInfo::parse(&req).unwrap();
        assert!(!req.b64);
    }

    #[test]
    #[cfg(feature = "v3")]
    fn request_info_polling_withb64() {
        assert!(cfg!(feature = "v3"));

        let req = build_request("http://localhost:3000/socket.io/?EIO=3&transport=polling&b64=1");
        let req = RequestInfo::parse(&req).unwrap();
        assert!(req.b64);
    }

    #[test]
    fn transport_unknown_err() {
        let req = build_request("http://localhost:3000/socket.io/?EIO=4&transport=grpc");
        let err = RequestInfo::parse(&req).unwrap_err();
        assert!(matches!(err, Error::UnknownTransport));
    }
    #[test]
    fn unsupported_protocol_version() {
        let req = build_request("http://localhost:3000/socket.io/?EIO=2&transport=polling");
        let err = RequestInfo::parse(&req).unwrap_err();
        assert!(matches!(err, Error::UnsupportedProtocolVersion));
    }
    #[test]
    fn bad_handshake_method() {
        let req = Request::post("http://localhost:3000/socket.io/?EIO=4&transport=polling")
            .body(())
            .unwrap();
        let err = RequestInfo::parse(&req).unwrap_err();
        assert!(matches!(err, Error::BadHandshakeMethod));
    }
}
