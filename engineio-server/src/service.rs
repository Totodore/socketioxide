use crate::{
    body::ResponseBody,
    futures::ResponseFuture,
    packet::{OpenPacket, Packet, TransportType},
};
use futures::{
    channel::mpsc::{unbounded, UnboundedSender},
    future, SinkExt, StreamExt, TryStreamExt,
};
use http::{Method, Request};
use http_body::Body;
use hyper::{service::Service, upgrade::Upgraded, Response};
use std::{
    collections::HashMap,
    fmt::Debug,
    net::SocketAddr,
    sync::Arc,
    task::{Context, Poll},
};
use tokio::sync::Mutex;
use tungstenite::{protocol::Role, Message};

use tokio_tungstenite::WebSocketStream;

type Tx = UnboundedSender<Message>;
type PeerMap = Arc<Mutex<HashMap<SocketAddr, Tx>>>;

#[derive(Debug, Clone)]
pub struct EngineIoService<S> {
    inner: S,
}

impl<S> EngineIoService<S> {
    pub fn new(inner: S) -> Self {
        EngineIoService { inner }
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
        let headers = req.headers().clone();
        if transport_type == TransportType::Websocket {
            tokio::task::spawn(async move {
                match hyper::upgrade::on(req).await {
                    Ok(upgraded) => {
                        let ws =
                            WebSocketStream::from_raw_socket(upgraded, Role::Server, None).await;
                        println!("upgrade success: {:?}", ws.get_config());
                        handle_connection(ws);
                    }
                    Err(e) => println!("upgrade error: {}", e),
                }
            });
            ResponseFuture::upgrade_response(headers)
        } else {
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

fn handle_connection(ws_stream: WebSocketStream<Upgraded>) {
    println!("WebSocket connection established");

    let (mut tx, mut rx) = ws_stream.split();

    tokio::spawn(async move {
        loop {
            if let Some(message) = rx.next().await {
                if let Ok(msg) = message {
                    println!("Received message: {:?}", msg);
                    let msg: String = Packet::Pong.try_into().unwrap();
                    tx.send(Message::text(msg + &"probe".to_string()))
                        .await
                        .unwrap();
                }
            }
        }
    });
}
