use crate::{
    body::ResponseBody,
    engine::{EngineIo, EngineIoConfig},
    futures::ResponseFuture, layer::EngineIoHandler,
};
use http::{Method, Request};
use http_body::Body;
use hyper::{service::Service, Response};
use std::{
    fmt::Debug,
    sync::Arc,
    task::{Context, Poll},
};

#[derive(Debug, Clone)]
pub struct EngineIoService<S, H> where H: EngineIoHandler {
    inner: S,
    engine: Arc<EngineIo<H>>,
}

impl<S, H> EngineIoService<S, H> where H: EngineIoHandler {
    pub fn from_config(inner: S, handler: H, config: EngineIoConfig) -> Self {
        EngineIoService {
            inner,
            engine: EngineIo::from_config(handler, config).into(),
        }
    }
}

impl<ReqBody, ResBody, S, H> Service<Request<ReqBody>> for EngineIoService<S, H>
where
    ResBody: Body,
    ReqBody: http_body::Body + Send + 'static + Debug,
    <ReqBody as http_body::Body>::Error: Debug,
    <ReqBody as http_body::Body>::Data: Send,
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    H: EngineIoHandler
{
    type Response = Response<ResponseBody<ResBody>>;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        if req.uri().path().starts_with("/engine.io") {
            let engine = self.engine.clone();
            match RequestType::parse(&req) {
                RequestType::Invalid => ResponseFuture::empty_response(400),
                //TODO: Avoid cloning ?
                RequestType::HttpOpen => ResponseFuture::open_response(self.engine.config.clone()),
                RequestType::HttpPoll => engine.on_polling_req(req),
                RequestType::HttpSendPacket => engine.on_send_packet_req(req),
                RequestType::WebsocketUpgrade => engine.upgrade_ws_req(req),
            }
        } else {
            ResponseFuture::new(self.inner.call(req))
        }
    }
}

enum RequestType {
    Invalid,
    HttpOpen,
    HttpPoll,
    HttpSendPacket,
    WebsocketUpgrade,
}

impl RequestType {
    fn parse<B>(req: &Request<B>) -> Self {
        if let Some(query) = req.uri().query() {
            if !query.contains("EIO=4")
                || req.method() != Method::GET && req.method() != Method::POST
            {
                return RequestType::Invalid;
            }

            if query.contains("transport=polling") {
                if query.contains("sid=") {
                    if req.method() == Method::GET {
                        RequestType::HttpPoll
                    } else if req.method() == Method::POST {
                        RequestType::HttpSendPacket
                    } else {
                        RequestType::Invalid
                    }
                } else if req.method() == Method::GET {
                    RequestType::HttpOpen
                } else {
                    RequestType::Invalid
                }
            } else if query.contains("transport=websocket") && req.method() == Method::GET {
                RequestType::WebsocketUpgrade
            } else {
                RequestType::Invalid
            }
        } else {
            RequestType::Invalid
        }
    }
}
