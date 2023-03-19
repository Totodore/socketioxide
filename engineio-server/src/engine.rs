use std::{
    collections::HashMap,
    ops::ControlFlow,
    sync::Arc,
};

use crate::{
    body::ResponseBody,
    futures::{http_empty_response, http_response, ws_response},
    layer::EngineIoHandler,
    packet::{OpenPacket, Packet, TransportType},
    socket::Socket,
    utils::generate_sid,
};
use futures::{SinkExt, StreamExt, TryStreamExt};
use http::{Request, Response, StatusCode};
use hyper::upgrade::Upgraded;
use std::fmt::Debug;
use tokio::sync::{RwLock};
use tokio_tungstenite::{
    tungstenite::{protocol::Role, Message},
    WebSocketStream,
};
use tracing::debug;

#[derive(Debug, Clone)]
pub struct EngineIoConfig {
    pub req_path: String,
    pub ping_interval: u64,
    pub ping_timeout: u64,
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
/// Abstract engine implementation for Engine.IO server for http polling and websocket
#[derive(Debug)]
pub struct EngineIo<H>
where
    H: EngineIoHandler,
{
    sockets: SocketMap<Socket>,
    handler: H,
    pub config: EngineIoConfig,
}

impl<H> EngineIo<H>
where
    H: EngineIoHandler,
{
    pub fn from_config(handler: H, config: EngineIoConfig) -> Self {
        Self {
            sockets: RwLock::new(HashMap::new()),
            config,
            handler,
        }
    }
}

impl<H> EngineIo<H>
where
    H: EngineIoHandler,
{
    pub async fn on_open_http_req<B>(self: Arc<Self>) -> Response<ResponseBody<B>>
    where
        B: Send + 'static,
    {
        let sid = generate_sid();
        self.sockets.write().await.insert(sid, Socket::new(sid));
        let packet: String =
            Packet::Open(OpenPacket::new(TransportType::Polling, sid, &self.config))
                .try_into()
                .unwrap();
        http_response(StatusCode::OK, packet).unwrap()
    }

    pub async fn on_polling_req<B>(self: Arc<Self>, sid: i64) -> Response<ResponseBody<B>>
    where
        B: Send + 'static,
    {
        let sockets = self.sockets.read().await;
        let mut rx = match sockets.get(&sid).map(|s| s.rx.try_lock()) {
            Some(Ok(s)) => s,
            None | Some(Err(_)) => return http_empty_response(StatusCode::BAD_REQUEST).unwrap(),
        };

        debug!("Polling request for socket with sid {}", sid);
        let mut data = String::new();
        while let Ok(packet) = rx.try_recv() {
            let packet: String = packet.try_into().unwrap();
            data.push_str(&packet);
        }
        
        if data.is_empty() {
            match rx.recv().await {
                Some(packet) => {
                    let packet: String = packet.try_into().unwrap();
                    data.push_str(&packet);
                }
                None => {
                    //TODO: Handle socket disconnection
                    return http_empty_response(StatusCode::INTERNAL_SERVER_ERROR).unwrap()
                },
            };
        }
        http_response(StatusCode::OK, data).unwrap()
    }

    pub async fn on_send_packet_req<R, B>(
        self: Arc<Self>,
        sid: i64,
        body: Request<R>,
    ) -> Response<ResponseBody<B>>
    where
        R: http_body::Body + std::marker::Send + 'static,
        <R as http_body::Body>::Error: Debug,
        <R as http_body::Body>::Data: std::marker::Send,
        B: Send + 'static,
    {
        let body = hyper::body::to_bytes(body).await.unwrap();
        let packet = match Packet::try_from(body) {
            Ok(packet) => packet,
            Err(e) => {
                tracing::debug!("Error parsing packet: {:?}", e);
                if let Some(socket) = self.sockets.write().await.remove(&sid) {
                    socket.close().await.unwrap();
                }
                return http_empty_response(StatusCode::BAD_REQUEST).unwrap();
            }
        };
        
        if let Some(socket) = self.sockets.read().await.get(&sid) {
            debug!("Send packet request for socket with sid {}", sid);
            match socket.handle_packet(packet, &self.handler).await {
                ControlFlow::Continue(Err(e)) => {
                    tracing::debug!("Error handling packet: {:?}", e)
                }
                ControlFlow::Continue(Ok(_)) => (),
                ControlFlow::Break(Ok(_)) => self.close_socket(sid).await,
                ControlFlow::Break(Err(e)) => {
                    tracing::debug!("Error handling packet, closing session: {:?}", e);
                    self.close_socket(sid).await;
                }
            };
            http_response(StatusCode::OK, "ok").unwrap()
        } else {
            http_empty_response(StatusCode::BAD_REQUEST).unwrap()
        }
    }

    /// Upgrade a websocket request to create a websocket connection.
    ///
    /// If a sid is provided in the query it means that is is upgraded from an existing HTTP polling request. In this case
    /// the http polling request is closed and the SID is kept for the websocket
    pub async fn upgrade_ws_req<R, B>(
        self: Arc<Self>,
        sid: Option<i64>,
        req: Request<R>,
    ) -> Response<ResponseBody<B>> {
        tracing::debug!("WS Upgrade req {:?}", req.uri());

        let (parts, _) = req.into_parts();
        let ws_key = match parts.headers.get("Sec-WebSocket-Key") {
            Some(key) => key.clone(),
            None => {
                return Response::builder()
                    .status(400)
                    .body(ResponseBody::empty_response())
                    .unwrap()
            }
        };

        let req = Request::from_parts(parts, ());
        tokio::spawn(async move {
            match hyper::upgrade::on(req).await {
                Ok(conn) => self.on_ws_conn_upgrade(conn, sid).await,
                Err(e) => tracing::debug!("WS upgrade error: {}", e),
            }
        });

        ws_response(&ws_key)
    }

    /// Handle a websocket connection upgrade
    ///
    /// Sends an open packet if it is not an upgrade from a polling request
    ///
    /// Read packets from the websocket and handle them
    async fn on_ws_conn_upgrade(self: Arc<Self>, conn: Upgraded, sid: Option<i64>) {
        let ws = WebSocketStream::from_raw_socket(conn, Role::Server, None).await;
        tracing::debug!("WS upgrade comming from polling: {}", sid.is_some());

        let (mut tx, mut rx) = ws.split();
        
        let sid = if sid.is_none() || !self.sockets.read().await.contains_key(&sid.unwrap()) {
            let sid = generate_sid();
            let msg: String =
                Packet::Open(OpenPacket::new(TransportType::Websocket, sid, &self.config))
                    .try_into()
                    .expect("Failed to serialize open packet");
            if let Err(e) = tx.send(Message::Text(msg)).await {
                tracing::debug!("Error sending open packet: {}", e);
                return;
            }
            self.sockets
                .write()
                .await
                .insert(sid, Socket::new(sid));
            sid
        } else {
            let sid = sid.unwrap();
            // self.sockets
            //     .write()
            //     .await
            //     .get_mut(&sid)
            //     .unwrap()
            //     .upgrade_from_http(tx);
            //TODO: upgrade from http
            sid
        };

        // let sockets = self.sockets.read().await;
        // let mut socket_rx = sockets.get(&sid).unwrap().rx.try_lock().unwrap();
        // tokio::spawn(async move {
        //     while let Some(item) = socket_rx.recv().await {
        //         let packet: String = item.try_into().unwrap();
        //         if let Err(e) = tx.send(Message::Text(packet)).await {
        //             tracing::debug!("Error sending packet: {}", e);
        //             return;
        //         }
        //     }
        // });
        while let Ok(msg) = rx.try_next().await {
            let Some(msg) = msg else { continue };
            let res = match msg {
                Message::Text(msg) => match Packet::try_from(msg) {
                    Ok(packet) => {
                        self.sockets
                            .read()
                            .await
                            .get(&sid)
                            .unwrap()
                            .handle_packet(packet, &self.handler)
                            .await
                    }
                    Err(err) => ControlFlow::Break(Err(err)),
                },
                Message::Binary(msg) => {
                    match self
                        .sockets
                        .read()
                        .await
                        .get(&sid)
                        .unwrap()
                        .handle_binary(msg, &self.handler)
                        .await
                    {
                        Ok(_) => ControlFlow::Continue(Ok(())),
                        Err(e) => ControlFlow::Continue(Err(e)),
                    }
                }
                Message::Ping(_) => unreachable!(),
                Message::Pong(_) => unreachable!(),
                Message::Close(_) => break,
                Message::Frame(_) => unreachable!(),
            };
            match res {
                ControlFlow::Break(Ok(())) => break,
                ControlFlow::Break(Err(e)) => {
                    tracing::debug!("Error handling websocket message, closing conn: {:?}", e);
                    break;
                }
                ControlFlow::Continue(Ok(())) => continue,
                ControlFlow::Continue(Err(e)) => {
                    tracing::debug!("Error handling websocket message: {:?}", e);
                }
            }
        }
        self.close_socket(sid).await;
    }

    async fn close_socket(&self, sid: i64) {
        tracing::debug!("Closing socket {}", sid);
        if let Some(socket) = self.sockets.write().await.remove(&sid) {
            if let Err(e) = socket.close().await {
                tracing::debug!("Error closing websocket: {:?}", e);
            }
        }
    }
}
