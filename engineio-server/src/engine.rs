#![deny(clippy::await_holding_lock)]
use std::{
    collections::HashMap,
    ops::ControlFlow,
    sync::{Arc, RwLock},
    time::Duration,
};

use crate::{
    body::ResponseBody,
    errors::Error,
    futures::{http_empty_response, http_response, ws_response},
    layer::EngineIoHandler,
    packet::{OpenPacket, Packet, TransportType},
    socket::{ConnectionType, Socket},
    utils::generate_sid,
};
use futures::{SinkExt, StreamExt, TryStreamExt};
use http::{Request, Response, StatusCode};
use hyper::upgrade::Upgraded;
use std::fmt::Debug;
use tokio_tungstenite::{
    tungstenite::{protocol::Role, Message},
    WebSocketStream,
};
use tracing::debug;

#[derive(Debug, Clone)]
pub struct EngineIoConfig {
    pub req_path: String,
    pub ping_interval: Duration,
    pub ping_timeout: Duration,
}

impl Default for EngineIoConfig {
    fn default() -> Self {
        Self {
            req_path: "/engine.io".to_string(),
            ping_interval: Duration::from_millis(300),
            ping_timeout: Duration::from_millis(200),
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
                .unwrap()
                .insert(sid, Socket::new(sid, ConnectionType::Http).into());
        }
        self.clone().start_heartbeat_routine(sid);
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
        let socket = match self.get_socket(sid).map(|s| s.clone()) {
            Some(s) => s,
            None => return http_empty_response(StatusCode::BAD_REQUEST).unwrap(),
        };
        if socket.is_ws().await {
            return http_empty_response(StatusCode::BAD_REQUEST).unwrap();
        }

        // If the socket is already locked, it means that the socket is being used by another request
        // In case of multiple http polling, session should be closed
        let mut rx = match socket.rx.try_lock() {
            Ok(s) => s,
            Err(_) => {
                if socket.is_http().await {
                    socket.send(Packet::Close).await.ok();
                    self.close_session(sid);
                }
                return http_empty_response(StatusCode::BAD_REQUEST).unwrap();
            }
        };

        debug!("[sid={sid}] polling request");
        let mut data = String::new();
        while let Ok(packet) = rx.try_recv() {
            if packet == Packet::Abort {
                debug!("aborting immediate polling request");
                return http_empty_response(StatusCode::BAD_REQUEST).unwrap();
            }
            debug!("sending packet: {:?}", packet);
            let packet: String = packet.try_into().unwrap();
            data.push_str(&packet);
        }

        if data.is_empty() {
            match rx.recv().await {
                Some(Packet::Abort) => {
                    debug!("aborting waiting polling request");
                    return http_empty_response(StatusCode::BAD_REQUEST).unwrap();
                }
                Some(packet) => {
                    let packet: String = packet.try_into().unwrap();
                    data.push_str(&packet);
                }
                None => {
                    self.close_session(sid);
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
                debug!("[sid={sid}] error parsing packet: {:?}", e);
                self.close_session(sid);
                return http_empty_response(StatusCode::BAD_REQUEST).unwrap();
            }
        };

        if let Some(socket) = self.get_socket(sid) {
            if socket.is_ws().await {
                return http_empty_response(StatusCode::BAD_REQUEST).unwrap();
            }
            match socket.handle_packet(packet, &self.handler).await {
                ControlFlow::Continue(Err(e)) => {
                    debug!("[sid={sid}] error handling packet: {:?}", e)
                }
                ControlFlow::Continue(Ok(_)) => (),
                ControlFlow::Break(Ok(_)) => self.close_session(sid),
                ControlFlow::Break(Err(e)) => {
                    debug!(
                        "[sid={sid}] error handling packet, closing session: {:?}",
                        e
                    );
                    self.close_session(sid);
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
        let (parts, _) = req.into_parts();
        let ws_key = match parts.headers.get("Sec-WebSocket-Key") {
            Some(key) => key.clone(),
            None => {
                return http_empty_response(StatusCode::BAD_REQUEST).unwrap();
            }
        };
        if sid.is_some() {
            if let Some(s) = self.get_socket(sid.unwrap()) {
                if s.is_ws().await {
                    return http_empty_response(StatusCode::BAD_REQUEST).unwrap();
                }
            }
        }
        let req = Request::from_parts(parts, ());
        tokio::spawn(async move {
            match hyper::upgrade::on(req).await {
                Ok(conn) => match self.on_ws_conn_upgrade(conn, sid).await {
                    Ok(_) => debug!("ws closed"),
                    Err(e) => debug!("ws closed with error: {:?}", e),
                },
                Err(e) => debug!("ws upgrade error: {}", e),
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

        let socket = if sid.is_none() || !self.get_socket(sid.unwrap()).is_some() {
            let sid = generate_sid();
            let socket: Arc<Socket> = Socket::new(sid, ConnectionType::WebSocket).into();
            {
                self.sockets.write().unwrap().insert(sid, socket.clone());
            }
            self.ws_init_handshake(sid, &mut ws).await?;
            self.clone().start_heartbeat_routine(sid);
            socket
        } else {
            let sid = sid.unwrap();
            self.ws_upgrade_handshake(sid, &mut ws).await?;
            self.get_socket(sid).unwrap()
        };
        let (mut tx, mut rx) = ws.split();

        // Pipe between websocket and socket channel
        let rx_socket = socket.clone();
        let rx_handle = tokio::spawn(async move {
            let mut socket_rx = rx_socket.rx.try_lock().unwrap();
            while let Some(item) = socket_rx.recv().await {
                let res = match item {
                    Packet::Binary(bin) => tx.send(Message::Binary(bin)).await,
                    Packet::Close => tx.send(Message::Close(None)).await,
                    Packet::Abort => tx.close().await,
                    _ => {
                        let packet: String = item.try_into().unwrap();
                        tx.send(Message::Text(packet)).await
                    }
                };
                if let Err(e) = res {
                    debug!("[sid={}] error sending packet: {}", rx_socket.sid, e);
                    break;
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
                _ => panic!("[sid={}] unexpected ws message", socket.sid),
            };
            match res {
                ControlFlow::Break(Ok(())) => break,
                ControlFlow::Break(Err(e)) => {
                    debug!(
                        "[sid={}] error handling ws message, closing session: {:?}",
                        socket.sid, e
                    );
                    break;
                }
                ControlFlow::Continue(Ok(())) => continue,
                ControlFlow::Continue(Err(e)) => {
                    debug!("[sid={}] error handling ws message: {:?}", socket.sid, e);
                }
            }
        }
        self.close_session(socket.sid);
        rx_handle.abort();
        Ok(())
    }

    fn start_heartbeat_routine(self: Arc<Self>, sid: i64) {
        tokio::spawn(async move {
            let socket = self.get_socket(sid).unwrap();
            if let Err(e) = socket
                .spawn_heartbeat(self.config.ping_interval, self.config.ping_timeout)
                .await
            {
                if socket.rx.try_lock().is_err() {
                    debug!("[sid={sid}] sending abort to existing connection");
                    socket.send(Packet::Abort).await.ok();
                }
                debug!("[sid={sid}] trying to close session");
                self.close_session(sid);
                debug!("[sid={sid}] heartbeat error: {:?}", e);
            }
        });
    }

    /// Upgrade a session from a polling request to a websocket request.
    ///
    /// Before upgrading the session the server should send a NOOP packet to any pending polling request.
    ///
    /// ## Handshake :
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
        let socket = self.get_socket(sid).unwrap();
        // send a NOOP packet to any pending polling request
        socket.send(Packet::Noop).await?;

        let msg = match ws.next().await {
            Some(Ok(Message::Text(d))) => d,
            _ => Err(Error::BadPacket)?,
        };
        match Packet::try_from(msg)? {
            Packet::PingUpgrade => {
                ws.send(Message::Text(Packet::PongUpgrade.try_into()?))
                    .await?;
            }
            _ => Err(Error::BadPacket)?,
        };

        let msg = match ws.next().await {
            Some(Ok(Message::Text(d))) => d,
            _ => Err(Error::BadPacket)?,
        };
        match Packet::try_from(msg)? {
            Packet::Upgrade => debug!("[sid={sid}] ws upgraded successful"),
            _ => Err(Error::BadPacket)?,
        };

        // wait for any polling connection to finish by waiting for the socket to be unlocked
        let _lock = socket.rx.lock().await;
        socket.upgrade_to_websocket().await;
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
    fn close_session(&self, sid: i64) {
        self.sockets.write().unwrap().remove(&sid);
        debug!("[sid={sid}] socket closed");
    }

    /**
     * Get a socket by its sid
     * Clones the socket ref to avoid holding the lock
     */
    fn get_socket(&self, sid: i64) -> Option<Arc<Socket>> {
        self.sockets.read().unwrap().get(&sid).cloned()
    }
}
