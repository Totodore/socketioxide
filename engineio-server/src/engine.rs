use std::{collections::HashMap, ops::ControlFlow, sync::Arc};

use crate::{
    body::ResponseBody,
    errors::Error,
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
use tokio::sync::RwLock;
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

type SocketMap<T> = RwLock<HashMap<i64, Arc<T>>>;
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
        {
            self.sockets
                .write()
                .await
                .insert(sid, Socket::new(sid).into());
        }
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
        let socket = match self.get_socket(sid).await.map(|s| s.clone()) {
            Some(s) => s,
            None => return http_empty_response(StatusCode::BAD_REQUEST).unwrap(),
        };

        // If the socket is already locked, it means that the socket is being used by another request
        // and we should close the session (bad request)
        let mut rx = match socket.rx.try_lock() {
            Ok(s) => s,
            Err(_) => {
                socket.send(Packet::Close).await.ok();
                self.close_session(sid).await;
                return http_empty_response(StatusCode::BAD_REQUEST).unwrap();
            }
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
                    return http_empty_response(StatusCode::INTERNAL_SERVER_ERROR).unwrap();
                }
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
                self.close_session(sid).await;
                return http_empty_response(StatusCode::BAD_REQUEST).unwrap();
            }
        };

        if let Some(socket) = self.get_socket(sid).await {
            debug!("Send packet request for socket with sid {}", sid);
            match socket.handle_packet(packet, &self.handler).await {
                ControlFlow::Continue(Err(e)) => {
                    tracing::debug!("Error handling packet: {:?}", e)
                }
                ControlFlow::Continue(Ok(_)) => (),
                ControlFlow::Break(Ok(_)) => self.close_session(sid).await,
                ControlFlow::Break(Err(e)) => {
                    tracing::debug!("Error handling packet, closing session: {:?}", e);
                    self.close_session(sid).await;
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
                Ok(conn) => match self.on_ws_conn_upgrade(conn, sid).await {
                    Ok(_) => tracing::debug!("ws closed successfully"),
                    Err(e) => tracing::debug!("ws closed with error: {:?}", e),
                },
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
    async fn on_ws_conn_upgrade(
        self: Arc<Self>,
        conn: Upgraded,
        sid: Option<i64>,
    ) -> Result<(), Error> {
        let mut ws = WebSocketStream::from_raw_socket(conn, Role::Server, None).await;
        tracing::debug!("WS upgrade comming from polling: {}", sid.is_some());

        let socket = if sid.is_none() || !self.sockets.read().await.contains_key(&sid.unwrap()) {
            let sid = generate_sid();
            let socket: Arc<Socket> = Socket::new(sid).into();
            {
                self.sockets.write().await.insert(sid, socket.clone());
            }
            self.ws_init_handshake(sid, &mut ws).await?;
            socket
        } else {
            let sid = sid.unwrap();
            self.ws_upgrade_handshake(sid, &mut ws).await?;
            self.get_socket(sid).await.unwrap()
        };
        let (mut tx, mut rx) = ws.split();
        let rx_socket = socket.clone();
        let rx_handle = tokio::spawn(async move {
            let mut socket_rx = rx_socket.rx.try_lock().unwrap();
            while let Some(item) = socket_rx.recv().await {
                match item {
                    Packet::Binary(bin) => {
                        if let Err(e) = tx.send(Message::Binary(bin)).await {
                            tracing::debug!("Error sending binary packet: {}", e);
                            return;
                        }
                    },
                    Packet::Close => {
                        if let Err(e) = tx.send(Message::Close(None)).await {
                            tracing::debug!("Error sending close packet: {}", e);
                            return;
                        }
                    }
                    _ => {
                        let packet: String = item.try_into().unwrap();
                        if let Err(e) = tx.send(Message::Text(packet)).await {
                            tracing::debug!("Error sending packet: {}", e);
                            return;
                        }
                    }
                }
            }
        });
        while let Ok(msg) = rx.try_next().await {
            let Some(msg) = msg else { continue };
            let res = match msg {
                Message::Text(msg) => match Packet::try_from(msg) {
                    Ok(packet) => socket.handle_packet(packet, &self.handler).await,
                    Err(err) => ControlFlow::Break(Err(err)),
                },
                Message::Binary(msg) => match socket.handle_binary(msg, &self.handler).await {
                    Ok(_) => ControlFlow::Continue(Ok(())),
                    Err(e) => ControlFlow::Continue(Err(e)),
                },
                Message::Close(_) => break,
                _ => panic!("Unexpected websocket message"),
            };
            match res {
                ControlFlow::Break(Ok(())) => break,
                ControlFlow::Break(Err(e)) => {
                    tracing::debug!("Error handling websocket message, closing conn & session: {:?}", e);
                    break;
                }
                ControlFlow::Continue(Ok(())) => continue,
                ControlFlow::Continue(Err(e)) => {
                    tracing::debug!("Error handling websocket message: {:?}", e);
                }
            }
        }
        self.close_session(socket.sid).await;
        rx_handle.abort();
        Ok(())
    }

    /// Upgrade a session from a polling request to a websocket request.
    ///
    /// Before upgrading the session the server should send a NOOP packet to any pending polling request.
    /// ```text
    /// CLIENT                                                 SERVER
    ///│                                                      │
    ///│   GET /engine.io/?EIO=4&transport=websocket&sid=...  │
    ///│ ───────────────────────────────────────────────────► │
    ///│  ◄─────────────────────────────────────────────────┘ │
    ///│            HTTP 101 (WebSocket handshake)            │
    ///│                                                      │
    ///│            -----  WebSocket frames -----             │
    ///│  ─────────────────────────────────────────────────►  │
    ///│                         2probe                       │ (ping packet)
    ///│  ◄─────────────────────────────────────────────────  │
    ///│                         3probe                       │ (pong packet)
    ///│  ─────────────────────────────────────────────────►  │
    ///│                         5                            │ (upgrade packet)
    ///│                                                      │
    ///│            -----  WebSocket frames -----             │
    /// ```
    async fn ws_upgrade_handshake(
        &self,
        sid: i64,
        ws: &mut WebSocketStream<Upgraded>,
    ) -> Result<(), Error> {
        let socket = self.get_socket(sid).await.unwrap();
        socket.send(Packet::Noop).await?;
        // wait for any polling connection to finish by waiting for the socket to be unlocked
        let _lock = socket.rx.lock().await;
        // Get the next ws message
        let msg = match ws.next().await {
            Some(Ok(Message::Text(d))) => d,
            _ => Err(Error::BadPacket())?,
        };
        // Check it is a ping with probe string
        match Packet::try_from(msg) {
            Ok(Packet::PingUpgrade) => {
                ws.send(Message::Text(Packet::PongUpgrade.try_into().unwrap()))
                    .await?;
            }
            Ok(_) => Err(Error::BadPacket())?,
            Err(e) => Err(e)?,
        };

        let msg = match ws.next().await {
            Some(Ok(Message::Text(d))) => d,
            _ => todo!(),
        };
        match Packet::try_from(msg) {
            Ok(Packet::Upgrade) => debug!("Upgrade successful"),
            Ok(_) => Err(Error::BadPacket())?,
            Err(e) => Err(e)?,
        };
        Ok(())
    }
    async fn ws_init_handshake(
        &self,
        sid: i64,
        ws: &mut WebSocketStream<Upgraded>,
    ) -> Result<(), Error> {
        let msg: String =
            Packet::Open(OpenPacket::new(TransportType::Websocket, sid, &self.config))
                .try_into()?;
        ws.send(Message::Text(msg)).await?;

        Ok(())
    }
    async fn close_session(&self, sid: i64) {
        tracing::debug!("Closing socket {}", sid);
        self.sockets.write().await.remove(&sid);
    }

    /**
     * Get a socket by its sid
     * Clones the socket ref to avoid holding the lock
     */
    async fn get_socket(&self, sid: i64) -> Option<Arc<Socket>> {
        self.sockets.read().await.get(&sid).cloned()
    }
}
