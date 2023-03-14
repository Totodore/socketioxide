use std::{collections::HashMap, ops::ControlFlow, sync::Arc};

use futures::{stream::SplitSink, SinkExt, StreamExt, TryStreamExt};
use http::{request::Parts, Request};
use hyper::upgrade::Upgraded;
use tokio::sync::RwLock;
use tokio_tungstenite::{
    tungstenite::{protocol::Role, Message},
    WebSocketStream,
};

use crate::{
    errors::Error,
    futures::ResponseFuture,
    packet::{OpenPacket, Packet, TransportType},
    utils::generate_sid,
};

#[derive(Debug, Clone)]
pub struct EngineIoConfig {
    pub req_path: String,
    pub ping_interval: u32,
    pub ping_timeout: u32,
}

impl Default for EngineIoConfig {
    fn default() -> Self {
        Self {
            req_path: "/engine.io".to_string(),
            ping_interval: 300,
            ping_timeout: 200,
        }
    }
}

type SocketMap<T> = RwLock<HashMap<i64, T>>;
type Websocket = SplitSink<WebSocketStream<Upgraded>, Message>;
/// Abstract engine implementation for Engine.IO server for http polling and websocket
#[derive(Debug)]
pub struct EngineIo {
    ws_sockets: SocketMap<Websocket>,
    polling_sockets: SocketMap<hyper::body::Sender>,
    last_pong: RwLock<HashMap<i64, std::time::Instant>>,
    pub config: EngineIoConfig,
}

impl EngineIo {
    pub fn from_config(config: EngineIoConfig) -> Self {
        Self {
            ws_sockets: RwLock::new(HashMap::new()),
            polling_sockets: RwLock::new(HashMap::new()),
            last_pong: RwLock::new(HashMap::new()),
            config,
        }
    }
}

impl EngineIo {
    pub fn on_polling_req<F, B>(self: Arc<Self>, req: Request<B>) -> ResponseFuture<F> {
        // let sid = extract_sid(&req);
        // if sid.is_none() {
        // return ResponseFuture::empty_response(400);
        // }
        let (mut tx, body) = hyper::Body::channel();
        tokio::task::spawn(async move {});
        // let mut sockets = self.polling_sockets.lock().unwrap();
        // sockets.insert(sid.unwrap(), sender);
        ResponseFuture::streaming_response(body)
    }

    pub fn on_send_packet_req<B, F>(self: Arc<Self>, req: Request<B>) -> ResponseFuture<F> {
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

    /// Upgrade a websocket request to create a websocket connection.
    ///
    /// If a sid is provided in the query it means that is is upgraded from an existing HTTP polling request. In this case
    /// the http polling request is closed and the SID is kept for the websocket
    pub fn upgrade_ws_req<B, F>(self: Arc<Self>, req: Request<B>) -> ResponseFuture<F>
    where
        B: std::marker::Send,
    {
        println!("WS Upgrade req {:?}", req.uri());

        let (parts, _) = req.into_parts();
        let ws_key = match parts.headers.get("Sec-WebSocket-Key") {
            Some(key) => key.clone(),
            None => return ResponseFuture::empty_response(500),
        };

        tokio::task::spawn(async move {
            let sid = {
                let mut polling_sockets = self.polling_sockets.write().await;

                extract_sid(&parts).and_then(|sid| {
                    if polling_sockets.remove(&sid).is_some() {
                        Some(sid)
                    } else {
                        None
                    }
                })
            };

            let req = Request::from_parts(parts, ());
            match hyper::upgrade::on(req).await {
                Ok(conn) => self.on_ws_conn_upgrade(conn, sid).await,
                Err(e) => println!("WS upgrade error: {}", e),
            }
        });
        ResponseFuture::upgrade_response(ws_key)
    }

    /// Handle a websocket connection upgrade
    ///
    /// Sends an open packet if it is not an upgrade from a polling request
    ///
    /// Read packets from the websocket and handle them
    async fn on_ws_conn_upgrade(self: Arc<Self>, conn: Upgraded, sid: Option<i64>) {
        let ws = WebSocketStream::from_raw_socket(conn, Role::Server, None).await;
        println!("WS upgrade comming from polling: {}", sid.is_some());

        let (mut tx, mut rx) = ws.split();

        if sid.is_none() {
            let sid = generate_sid();
            let msg: String =
                Packet::Open(OpenPacket::new(TransportType::Websocket, sid, &self.config))
                    .try_into()
                    .expect("Failed to serialize open packet");
            if let Err(e) = tx.send(Message::Text(msg)).await {
                println!("Error sending open packet: {}", e);
                return;
            }
        }

        let sid = sid.unwrap_or(generate_sid());
        {
            self.ws_sockets.write().await.insert(sid, tx);
        }

        while let Ok(msg) = rx.try_next().await {
            let Some(msg) = msg else { continue };
            let res = match msg {
                Message::Text(msg) => match Packet::try_from(msg) {
                    Ok(packet) => self.clone().handle_packet(packet, sid).await,
                    Err(err) => ControlFlow::Break(Err(Error::SerializeError(err))),
                },
                Message::Binary(msg) => self.clone().handle_binary(msg).await,
                Message::Ping(_) => unreachable!(),
                Message::Pong(_) => unreachable!(),
                Message::Close(_) => break,
                Message::Frame(_) => unreachable!(),
            };
            match res {
                ControlFlow::Break(Ok(())) => break,
                ControlFlow::Break(Err(e)) => {
                    println!("Error handling websocket message, closing conn: {:?}", e);
                    break;
                },
                ControlFlow::Continue(Ok(())) => continue,
                ControlFlow::Continue(Err(e)) => {
                    println!("Error handling websocket message: {:?}", e);
                }
            }
        }
        if let Some(mut tx) = self.ws_sockets.write().await.remove(&sid) {
            if let Err(e) = tx.close().await {
                println!("Error closing websocket: {}", e);
            }
        }
    }

    async fn handle_packet(
        self: Arc<Self>,
        packet: Packet,
        sid: i64,
    ) -> ControlFlow<Result<(), Error>, Result<(), Error>> {
        println!("Received packet: {:?}", packet);
        match packet {
            Packet::Open(_) => ControlFlow::Continue(Err(Error::BadPacket(
                "Unexpected Open packet, it should be only used in upgrade process",
            ))),
            Packet::Close => ControlFlow::Break(Ok(())),
            Packet::Ping => todo!(),
            Packet::Pong => todo!(),
            Packet::Message(msg) => {
                println!("Received message: {}", msg);
                ControlFlow::Continue(Ok(()))
            }
            Packet::Upgrade => ControlFlow::Continue(Err(Error::BadPacket(
                "Unexpected Upgrade packet, upgrade from ws connection not supported",
            ))),
            Packet::Noop => ControlFlow::Continue(Err(Error::BadPacket(
                "Unexpected Noop packet, it should be only used in upgrade process",
            ))),
        }
    }

    async fn handle_binary(self: Arc<Self>, msg: Vec<u8>) -> ControlFlow<Result<(), Error>, Result<(), Error>> {
        println!("Received binary payload: {:?}", msg);
        ControlFlow::Continue(Ok(()))
    }
}

fn extract_sid(req: &Parts) -> Option<i64> {
    let uri = req.uri.query()?;
    let sid = uri
        .split("&")
        .find(|s| s.starts_with("sid="))?
        .split("=")
        .nth(1)?;
    Some(sid.parse().ok()?)
}
