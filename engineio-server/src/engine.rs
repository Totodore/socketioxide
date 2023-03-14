use std::{collections::HashMap, sync::Arc};

use futures::{stream::SplitSink, Future, SinkExt, StreamExt};
use http::{request::Parts, Request};
use hyper::upgrade::Upgraded;
use tokio::sync::Mutex;
use tokio_tungstenite::{
    tungstenite::{protocol::Role, Message},
    WebSocketStream,
};

use crate::{
    futures::ResponseFuture,
    packet::{OpenPacket, Packet},
};

#[derive(Debug, Clone)]
pub struct EngineIoConfig {
    pub req_path: String,
}

impl Default for EngineIoConfig {
    fn default() -> Self {
        Self {
            req_path: "/engine.io".to_string(),
        }
    }
}

type SocketMap<T> = Arc<Mutex<HashMap<u64, T>>>;
type WebsocketMap = SocketMap<SplitSink<WebSocketStream<Upgraded>, Message>>;
/// Abstract engine implementation for Engine.IO server for http polling and websocket
#[derive(Debug, Clone)]
pub struct EngineIo {
    ws_sockets: WebsocketMap,
    polling_sockets: SocketMap<hyper::body::Sender>,
    config: EngineIoConfig,
}

impl EngineIo {
    pub fn from_config(config: EngineIoConfig) -> Self {
        Self {
            ws_sockets: Arc::new(Mutex::new(HashMap::new())),
            polling_sockets: Arc::new(Mutex::new(HashMap::new())),
            config,
        }
    }
}

impl EngineIo {
    pub fn on_polling_req<F, B>(&self, req: Request<B>) -> ResponseFuture<F> {
        // let sid = extract_sid(&req);
        // if sid.is_none() {
            // return ResponseFuture::empty_response(400);
        // }
        let (mut sender, body) = hyper::Body::channel();
        // let mut sockets = self.polling_sockets.lock().unwrap();
        // sockets.insert(sid.unwrap(), sender);
        ResponseFuture::streaming_response(body)
    }

    pub fn on_send_packet_req<B, F>(&self, req: Request<B>) -> ResponseFuture<F> {
        // let sid = extract_sid(&req);
        // if sid.is_none() {
        // return ResponseFuture::empty_response(400);
        // }

        // let mut sockets = self.ws_sockets.lock().unwrap;
        // if let tx = sockets.get_mut(&sid.unwrap()) {
        // let packet = Packet::from_request(req);
        // } else {
        // return ResponseFuture::empty_response(400);
        // }
        ResponseFuture::empty_response(200)
    }

    pub fn upgrade_ws_req<B, F>(&self, req: Request<B>) -> ResponseFuture<F>
    where
        B: std::marker::Send,
    {
        println!("Request {:?} {}", req.headers(), req.uri());

        let (parts, _) = req.into_parts();
        let ws_key = parts.headers.get("Sec-WebSocket-Key").unwrap().clone();

        let ws_sockets = self.ws_sockets.clone();
        let polling_sockets = self.polling_sockets.clone();
        tokio::task::spawn(async move {
            let mut polling_sockets = polling_sockets.lock().await;
            let upgrading_from_polling = extract_sid(&parts)
                .map(|sid| polling_sockets.remove(&sid).is_some())
                .unwrap_or_default();
            let req = Request::from_parts(parts, ());

            match hyper::upgrade::on(req).await {
                Ok(conn) => on_connection_upgraded(conn, ws_sockets).await,
                Err(e) => println!("upgrade error: {}", e),
            }
        });
        ResponseFuture::upgrade_response(ws_key)
    }

    fn handle_packet(&self, packet: Packet) {}
}

//TODO: handle packet with rx
//TODO: Send an open message through tx
fn on_connection_upgraded(conn: Upgraded, ws_sockets: WebsocketMap) -> impl Future<Output = ()> {
    async move {
        let ws = WebSocketStream::from_raw_socket(conn, Role::Server, None).await;
        println!("WS Upgrade success: {:?}", ws.get_config());

        let msg: String = Packet::Open(OpenPacket::new()).try_into().unwrap();
        let (mut tx, mut rx) = ws.split();
        {
            let mut sockets = ws_sockets.lock().await;
            sockets.insert(1093019038, tx);
            let tx = sockets.get_mut(&1093019038).unwrap();
            tx.send(Message::Text(msg)).await.unwrap();
        }
    }
}

fn extract_sid(req: &Parts) -> Option<u64> {
    let uri = req.uri.query()?;
    let sid = uri
        .split("&")
        .find(|s| s.starts_with("sid="))?
        .split("=")
        .nth(1)?;
    Some(sid.parse().ok()?)
}
