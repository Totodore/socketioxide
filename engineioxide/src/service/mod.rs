use std::sync::Arc;

use ::futures::Future;
use http::{Method, Request};
use http_body::Body;

use crate::{
    config::EngineIoConfig,
    engine::EngineIo,
    handler::EngineIoHandler,
    transport::{polling, ws},
};

#[cfg(feature = "hyper-v1")]
mod hyper_service;

#[cfg(not(feature = "hyper-v1"))]
mod tower_service;

mod futures;
mod parser;

pub use self::parser::{ProtocolVersion, TransportType};
use self::{futures::ResponseFuture, parser::RequestInfo};

/// A [`Service`] that handles engine.io requests as a middleware.
/// If the request is not an engine.io request, it forwards it to the inner service.
/// If it is an engine.io request it will forward it to the appropriate [`transport`](crate::transport).
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

impl<S, H: EngineIoHandler> EngineIoService<H, S> {
    /// Dispatch a request according to the [`RequestInfo`] to the appropriate [`transport`](crate::transport).
    fn dispatch_req<F, ReqBody, ResBody>(&self, req: Request<ReqBody>) -> ResponseFuture<F, ResBody>
    where
        ReqBody: Body + Send + Unpin + 'static,
        ReqBody::Data: Send,
        ReqBody::Error: std::fmt::Debug,
        ResBody: Body + Send + 'static,
        F: Future,
    {
        let engine = self.engine.clone();
        match RequestInfo::parse(&req, &self.engine.config) {
            Ok(RequestInfo {
                protocol,
                sid: None,
                transport: TransportType::Polling,
                method: Method::GET,
                #[cfg(feature = "v3")]
                b64,
            }) => ResponseFuture::ready(polling::open_req(
                engine,
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
            }) => ResponseFuture::async_response(Box::pin(polling::polling_req(
                engine, protocol, sid,
            ))),
            Ok(RequestInfo {
                protocol,
                sid: Some(sid),
                transport: TransportType::Polling,
                method: Method::POST,
                ..
            }) => ResponseFuture::async_response(Box::pin(polling::post_req(
                engine, protocol, sid, req,
            ))),
            Ok(RequestInfo {
                protocol,
                sid,
                transport: TransportType::Websocket,
                method: Method::GET,
                ..
            }) => ResponseFuture::ready(ws::new_req(engine, protocol, sid, req)),
            Err(e) => {
                #[cfg(feature = "tracing")]
                tracing::debug!("error parsing request: {:?}", e);
                ResponseFuture::ready(Ok(e.into()))
            }
            _req => {
                #[cfg(feature = "tracing")]
                tracing::debug!("invalid request: {:?}", _req);
                ResponseFuture::empty_response(400)
            }
        }
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
impl<H: EngineIoHandler, S> std::fmt::Debug for EngineIoService<H, S> {
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

/// A [`Service`] that always returns a 404 response and that is compatible with [`EngineIoService`].
#[derive(Debug, Clone)]
pub struct NotFoundService;
