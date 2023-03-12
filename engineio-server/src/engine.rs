use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use futures::{stream::SplitSink, SinkExt, StreamExt};
use http::Request;
use hyper::upgrade::Upgraded;
use tokio_tungstenite::WebSocketStream;
use tungstenite::{protocol::Role, Message};

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
/// Abstract engine implementation for Engine.IO server for http polling and websocket
#[derive(Debug, Clone)]
pub struct EngineIo {
    ws_sockets: SocketMap<SplitSink<WebSocketStream<Upgraded>, Message>>,
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
        let sid = extract_sid(&req);
        if sid.is_none() {
            return ResponseFuture::empty_response(400);
        }
        let (mut sender, body) = hyper::Body::channel();
        let mut sockets = self.polling_sockets.lock().unwrap();
        sockets.insert(sid.unwrap(), sender);
        ResponseFuture::streaming_response(body)
    }

    pub fn on_send_packet_req<B, F>(&self, req: Request<B>) -> ResponseFuture<F> {
        let sid = extract_sid(&req);
        if sid.is_none() {
            return ResponseFuture::empty_response(400);
        }

        let mut sockets = self.ws_sockets.lock().unwrap();
        if let tx = sockets.get_mut(&sid.unwrap()) {
            // let packet = Packet::from_request(req);
        } else {
            return ResponseFuture::empty_response(400);
        }
        ResponseFuture::empty_response(200)
    }

    pub fn upgrade_ws_req<B, F>(&self, req: Request<B>) -> ResponseFuture<F>
    where
        B: std::marker::Send,
    {
        println!("Request {:?} {}", req.headers(), req.uri());
        let mut upgrading_from_polling = false;
        if let Some(sid) = extract_sid(&req) {
            println!("Upgrading from http polling with {}", sid);
            upgrading_from_polling = self.polling_sockets.lock().unwrap().remove(&sid).is_some();
        }
        let (parts, _) = req.into_parts();
        let ws_key = parts.headers.get("Sec-WebSocket-Key").unwrap().clone();

        let sock_map = self.ws_sockets.clone();

        tokio::task::spawn(async move {
            let req = Request::from_parts(parts, ());
            match hyper::upgrade::on(req).await {
                Ok(upgraded) => {
                    let ws = WebSocketStream::from_raw_socket(upgraded, Role::Server, None).await;
                    println!("WS Upgrade success: {:?}", ws.get_config());

					let msg: String = Packet::Open(OpenPacket::new()).try_into().unwrap();
                    let (mut tx, mut rx) = ws.split();
                    {
                        let mut sockets = sock_map.lock().unwrap();
                        sockets.insert(1093019038, tx);
						// let tx = sockets.get_mut(&1093019038).unwrap();
						// tx.send(Message::Text(msg)).await.unwrap();
                    }

                    //TODO: handle packet with rx
                    //TODO: Send an open message through tx
                }
                Err(e) => println!("upgrade error: {}", e),
            }
        });
        ResponseFuture::upgrade_response(ws_key)
    }

    fn handle_packet(&self, packet: Packet) {}
}

fn extract_sid<B>(req: &Request<B>) -> Option<u64> {
    let uri = req.uri().query()?;
    let sid = uri
        .split("&")
        .find(|s| s.starts_with("sid="))?
        .split("=")
        .nth(1)?;
    Some(sid.parse().ok()?)
}
