use crate::{
    body::ResponseBody, futures::ResponseFuture, packet::TransportType,
    websocket::{upgrade_ws_connection}, engine::{EngineIo, EngineIoConfig},
};
use http::{Method, Request};
use http_body::Body;
use hyper::{service::Service, Response};
use std::{
    fmt::Debug,
    task::{Context, Poll},
};

#[derive(Debug, Clone)]
pub struct EngineIoService<S> {
    inner: S,
	engine: EngineIo,
}

impl<S> EngineIoService<S> {
	pub fn from_config(inner: S, config: EngineIoConfig) -> Self {
		EngineIoService { inner, engine: EngineIo::from_config(config) }
	}
}

impl<ReqBody, ResBody, S> Service<Request<ReqBody>> for EngineIoService<S>
where
    ResBody: Body,
    ReqBody: Send + 'static + Debug,
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
{
    type Response = Response<ResponseBody<ResBody>>;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        if req.uri().path().starts_with("/engine.io") {
            if !is_valid_engineio_req(&req) {
                return ResponseFuture::empty_response(400);
            }
            handle_engineio_request(req)
        } else {
            ResponseFuture::new(self.inner.call(req))
        }
    }
}

fn handle_engineio_request<T, V>(req: Request<T>) -> ResponseFuture<V>
where
    T: Send + 'static,
{
    let query = req.uri().query().unwrap_or_default();
    if !query.contains("EIO=4")
        || (!query.contains("transport=polling") && !query.contains("transport=websocket"))
    {
        return ResponseFuture::empty_response(400);
    }

    if let Some(transport_type) = get_transport_type(&req) {
        if transport_type == TransportType::Websocket {
            upgrade_ws_connection(req)
        } else {
			// let (sender, body) = hyper::Body::channel();
			// let res = Response::builder()
            //         .status(200)
            //         .body(body)
            //         .unwrap();
			// ResponseFuture::new(res);

            ResponseFuture::open_response(transport_type)
        }
    } else {
        ResponseFuture::empty_response(400)
    }
}

fn is_valid_engineio_req<T>(req: &Request<T>) -> bool {
    let query = req.uri().query().unwrap_or_default();
    req.method() == Method::GET
        && query.contains("EIO=4")
        && (query.contains("transport=polling") || query.contains("transport=websocket"))
}
fn get_transport_type<T>(req: &Request<T>) -> Option<TransportType> {
    let query = req.uri().query().unwrap_or_default();
    if query.contains("transport=websocket") {
        Some(TransportType::Websocket)
    } else if query.contains("transport=polling") {
        Some(TransportType::Polling)
    } else {
        None
    }
}
