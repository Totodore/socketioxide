use crate::utils::{Generator, Sid};
use crate::{
    body::ResponseBody,
    engine::EngineIo,
    futures::ResponseFuture,
    layer::{EngineIoConfig, EngineIoHandler},
};
use http::{Method, Request};
use http_body::Body;
use hyper::{service::Service, Response};
use std::{
    fmt::Debug,
    str::FromStr,
    sync::Arc,
    task::{Context, Poll},
};

pub struct EngineIoService<S, H, G>
where
    H: EngineIoHandler<G::Sid> + ?Sized,
    G: Generator,
{
    inner: S,
    engine: Arc<EngineIo<H, G>>,
}

impl<S, H, G> EngineIoService<S, H, G>
where
    H: EngineIoHandler<G::Sid> + ?Sized,
    G: Generator,
{
    pub fn from_config(inner: S, handler: Arc<H>, config: EngineIoConfig, g: G) -> Self {
        EngineIoService {
            inner,
            engine: EngineIo::from_config(handler, config, g).into(),
        }
    }

    pub fn from_custom_engine(inner: S, engine: Arc<EngineIo<H, G>>) -> Self {
        EngineIoService { inner, engine }
    }
}

impl<ReqBody, ResBody, S, H, G> Service<Request<ReqBody>> for EngineIoService<S, H, G>
where
    ResBody: Body + Send + 'static,
    ReqBody: http_body::Body + Send + 'static + Debug,
    <ReqBody as http_body::Body>::Error: Debug,
    <ReqBody as http_body::Body>::Data: Send,
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    H: EngineIoHandler<G::Sid> + ?Sized,
    G: Generator,
{
    type Response = Response<ResponseBody<ResBody>>;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future, ResBody>;

    fn poll_ready(&mut self, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        if req.uri().path().starts_with(&self.engine.config.req_path) {
            let engine = self.engine.clone();
            match RequestInfo::parse(&req) {
                Some(RequestInfo {
                    sid: None,
                    transport: TransportType::Polling,
                    method: Method::GET,
                    ..
                }) => ResponseFuture::async_response(Box::pin(engine.on_open_http_req(req))),
                Some(RequestInfo {
                    sid: Some(sid),
                    transport: TransportType::Polling,
                    method: Method::GET,
                    ..
                }) => ResponseFuture::async_response(Box::pin(engine.on_polling_http_req(sid))),
                Some(RequestInfo {
                    sid: Some(sid),
                    transport: TransportType::Polling,
                    method: Method::POST,
                    ..
                }) => ResponseFuture::async_response(Box::pin(engine.on_post_http_req(sid, req))),
                Some(RequestInfo {
                    sid,
                    transport: TransportType::Websocket,
                    method: Method::GET,
                    ..
                }) => ResponseFuture::async_response(Box::pin(engine.on_ws_req(sid, req))),
                _ => ResponseFuture::empty_response(400),
            }
        } else {
            ResponseFuture::new(self.inner.call(req))
        }
    }
}

impl<S, H, G> Clone for EngineIoService<S, H, G>
where
    H: EngineIoHandler<G::Sid> + ?Sized,
    G: Generator,
    S: Clone,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            engine: self.engine.clone(),
        }
    }
}

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

struct RequestInfo<S: Sid> {
    sid: Option<S>,
    transport: TransportType,
    method: Method,
}

impl<S: Sid> RequestInfo<S> {
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
        let info: RequestInfo<i64> = RequestInfo::parse(&req).unwrap();
        assert_eq!(info.sid, None);
        assert_eq!(info.transport, TransportType::Polling);
        assert_eq!(info.method, Method::GET);
    }

    #[test]
    fn request_info_websocket() {
        let req = build_request("http://localhost:3000/socket.io/?EIO=4&transport=websocket");
        let info: RequestInfo<i64> = RequestInfo::parse(&req).unwrap();
        assert_eq!(info.sid, None);
        assert_eq!(info.transport, TransportType::Websocket);
        assert_eq!(info.method, Method::GET);
    }

    #[test]
    fn request_info_polling_with_sid() {
        let req = build_request("http://localhost:3000/socket.io/?EIO=4&transport=polling&sid=123");
        let info: RequestInfo<i64> = RequestInfo::parse(&req).unwrap();
        assert_eq!(info.sid, Some(123));
        assert_eq!(info.transport, TransportType::Polling);
        assert_eq!(info.method, Method::GET);
    }

    #[test]
    fn request_info_websocket_with_sid() {
        let req =
            build_request("http://localhost:3000/socket.io/?EIO=4&transport=websocket&sid=123");
        let info: RequestInfo<i64> = RequestInfo::parse(&req).unwrap();
        assert_eq!(info.sid, Some(123));
        assert_eq!(info.transport, TransportType::Websocket);
        assert_eq!(info.method, Method::GET);
    }
}
