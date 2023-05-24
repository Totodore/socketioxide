#![deny(clippy::await_holding_lock)]
use std::{
    collections::HashMap,
    io::BufRead,
    ops::ControlFlow,
    sync::{Arc, RwLock},
};

use crate::{
    body::ResponseBody,
    errors::Error,
    futures::{http_response, ws_response},
    layer::{EngineIoConfig, EngineIoHandler},
    packet::{OpenPacket, Packet},
    service::TransportType,
    socket::{ConnectionType, Socket, SocketReq},
    utils::generate_sid,
};
use bytes::Buf;
use futures::{SinkExt, StreamExt, TryStreamExt};
use http::{Request, Response, StatusCode};
use hyper::upgrade::Upgraded;
use std::fmt::Debug;
use tokio_tungstenite::{
    tungstenite::{protocol::Role, Message},
    WebSocketStream,
};
use tracing::debug;

type SocketMap<T> = RwLock<HashMap<i64, Arc<T>>>;
/// Abstract engine implementation for Engine.IO server for http polling and websocket
pub struct EngineIo<H>
where
    H: EngineIoHandler + ?Sized,
{
    sockets: SocketMap<Socket<H>>,
    handler: Arc<H>,
    pub config: EngineIoConfig,
}

impl<H> EngineIo<H>
where
    H: EngineIoHandler + ?Sized,
{
    pub fn from_config(handler: Arc<H>, config: EngineIoConfig) -> Self {
        Self {
            sockets: RwLock::new(HashMap::new()),
            config,
            handler: handler.clone(),
        }
    }
}

impl<H> EngineIo<H>
where
    H: EngineIoHandler + ?Sized,
{
    pub(crate) async fn on_open_http_req<B, R>(
        self: Arc<Self>,
        req: Request<R>,
    ) -> Result<Response<ResponseBody<B>>, Error>
    where
        B: Send + 'static,
    {
        let sid = generate_sid();
        {
            self.sockets.write().unwrap().insert(
                sid,
                Socket::new(
                    sid,
                    ConnectionType::Http,
                    &self.config,
                    self.handler.clone(),
                    SocketReq::from(req.into_parts().0)
                )
                .into(),
            );
        }
        let socket = self.get_socket(sid).unwrap();
        socket.spawn_heartbeat(self.config.ping_interval, self.config.ping_timeout);
        let packet: String =
            Packet::Open(OpenPacket::new(TransportType::Polling, sid, &self.config)).try_into()?;
        http_response(StatusCode::OK, packet).map_err(Error::HttpError)
    }

    /// Handle http polling request
    ///
    /// If there is packet in the socket buffer, it will be sent immediately
    /// Otherwise it will wait for the next packet to be sent from the socket
    pub(crate) async fn on_polling_http_req<B>(
        self: Arc<Self>,
        sid: i64,
    ) -> Result<Response<ResponseBody<B>>, Error>
    where
        B: Send + 'static,
    {
        let socket = self
            .get_socket(sid)
            .map(|s| s.is_http().then(|| s))
            .flatten()
            .ok_or(Error::HttpErrorResponse(StatusCode::BAD_REQUEST))?;

        // If the socket is already locked, it means that the socket is being used by another request
        // In case of multiple http polling, session should be closed
        let mut rx = match socket.rx.try_lock() {
            Ok(s) => s,
            Err(_) => {
                if socket.is_http() {
                    socket.send(Packet::Close)?;
                    self.close_session(sid);
                }
                return Err(Error::HttpErrorResponse(StatusCode::BAD_REQUEST));
            }
        };

        debug!("[sid={sid}] polling request");
        let mut data = String::new();
        while let Ok(packet) = rx.try_recv() {
            if packet == Packet::Abort {
                debug!("aborting immediate polling request");
                return Err(Error::HttpErrorResponse(StatusCode::BAD_REQUEST));
            }
            debug!("sending packet: {:?}", packet);
            let packet: String = packet.try_into().unwrap();
            if !data.is_empty() {
                data.push('\x1e');
            }
            data.push_str(&packet);
        }

        if data.is_empty() {
            match rx.recv().await {
                Some(Packet::Abort) | None => {
                    debug!("aborting waiting polling request");
                    return Err(Error::HttpErrorResponse(StatusCode::BAD_REQUEST));
                }
                Some(packet) => {
                    let packet: String = packet.try_into().unwrap();
                    data.push_str(&packet);
                }
            };
        }
        Ok(http_response(StatusCode::OK, data)?)
    }

    /// Handle http polling post request
    ///
    /// Split the body into packets and send them to the internal socket
    pub(crate) async fn on_post_http_req<R, B>(
        self: Arc<Self>,
        sid: i64,
        body: Request<R>,
    ) -> Result<Response<ResponseBody<B>>, Error>
    where
        R: http_body::Body + std::marker::Send + 'static,
        <R as http_body::Body>::Error: Debug,
        <R as http_body::Body>::Data: std::marker::Send,
        B: Send + 'static,
    {
        let body = hyper::body::aggregate(body).await.map_err(|e| {
            debug!("error aggregating body: {:?}", e);
            Error::HttpErrorResponse(StatusCode::BAD_REQUEST)
        })?;
        let packets = body.reader().split(b'\x1e');

        let socket = self
            .get_socket(sid)
            .map(|s| s.is_http().then(|| s))
            .flatten()
            .ok_or(Error::HttpErrorResponse(StatusCode::BAD_REQUEST))?;

        for packet in packets {
            let packet = match Packet::try_from(packet?) {
                Ok(p) => p,
                Err(e) => {
                    debug!("[sid={sid}] error parsing packet: {:?}", e);
                    self.close_session(sid);
                    return Err(Error::HttpErrorResponse(StatusCode::BAD_REQUEST));
                }
            };
            match socket.handle_packet(packet).await {
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
        }
        Ok(http_response(StatusCode::OK, "ok")?)
    }

    /// Upgrade a websocket request to create a websocket connection.
    ///
    /// If a sid is provided in the query it means that is is upgraded from an existing HTTP polling request. In this case
    /// the http polling request is closed and the SID is kept for the websocket
    pub(crate) async fn on_ws_req<R, B>(
        self: Arc<Self>,
        sid: Option<i64>,
        req: Request<R>,
    ) -> Result<Response<ResponseBody<B>>, Error> {
        let (parts, _) = req.into_parts();
        let ws_key = parts
            .headers
            .get("Sec-WebSocket-Key")
            .ok_or(Error::HttpErrorResponse(StatusCode::BAD_REQUEST))?
            .clone();
        let (uri, headers) = (parts.uri.clone(), parts.headers.clone());
        let req_data = SocketReq::new(uri, headers);

        let req = Request::from_parts(parts, ());
        tokio::spawn(async move {
            match hyper::upgrade::on(req).await {
                Ok(conn) => match self.on_ws_req_init(conn, sid, req_data).await {
                    Ok(_) => debug!("ws closed"),
                    Err(e) => debug!("ws closed with error: {:?}", e),
                },
                Err(e) => debug!("ws upgrade error: {}", e),
            }
        });

        Ok(ws_response(&ws_key)?)
    }

    /// Handle a websocket connection upgrade
    ///
    /// Sends an open packet if it is not an upgrade from a polling request
    ///
    /// Read packets from the websocket and handle them
    async fn on_ws_req_init(
        self: Arc<Self>,
        conn: Upgraded,
        sid: Option<i64>,
        req_data: SocketReq,
    ) -> Result<(), Error> {
        let mut ws = WebSocketStream::from_raw_socket(conn, Role::Server, None).await;

        let socket = if sid.is_none() || !self.get_socket(sid.unwrap()).is_some() {
            let sid = generate_sid();
            let socket: Arc<Socket<H>> = Socket::new(
                sid,
                ConnectionType::WebSocket,
                &self.config,
                self.handler.clone(),
                req_data
            )
            .into();
            {
                self.sockets.write().unwrap().insert(sid, socket.clone());
            }
            debug!("[sid={sid}] new websocket connection");
            self.ws_init_handshake(sid, &mut ws).await?;
            socket
                .clone()
                .spawn_heartbeat(self.config.ping_interval, self.config.ping_timeout);
            socket
        } else {
            let sid = sid.unwrap();
            debug!("[sid={sid}] websocket connection upgrade");
            if let Some(socket) = self.get_socket(sid) {
                if socket.is_ws() {
                    return Err(Error::UpgradeError);
                }
            }
            self.ws_upgrade_handshake(sid, &mut ws, req_data).await?;
            self.get_socket(sid).unwrap()
        };
        let (mut tx, mut rx) = ws.split();

        // Pipe between websocket and internal socket channel
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
                debug!("[sid={}] sent packet", rx_socket.sid);
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
                    Ok(packet) => socket.handle_packet(packet).await,
                    Err(err) => ControlFlow::Break(Err(err)),
                },
                Message::Binary(msg) => socket.handle_packet(Packet::Binary(msg)).await,
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
        req_data: SocketReq,
    ) -> Result<(), Error> {
        let socket = self.get_socket(sid).unwrap();
        // send a NOOP packet to any pending polling request
        socket.send(Packet::Noop)?;

        let msg = match ws.next().await {
            Some(Ok(Message::Text(d))) => d,
            _ => Err(Error::UpgradeError)?,
        };
        match Packet::try_from(msg)? {
            Packet::PingUpgrade => {
                ws.send(Message::Text(Packet::PongUpgrade.try_into()?))
                    .await?;
            }
            p => Err(Error::BadPacket(p))?,
        };

        let msg = match ws.next().await {
            Some(Ok(Message::Text(d))) => d,
            _ => Err(Error::UpgradeError)?,
        };
        match Packet::try_from(msg)? {
            Packet::Upgrade => debug!("[sid={sid}] ws upgraded successful"),
            p => Err(Error::BadPacket(p))?,
        };

        // wait for any polling connection to finish by waiting for the socket to be unlocked
        let _lock = socket.rx.lock().await;
        socket.upgrade_to_websocket(req_data);
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
        if let Some(socket) = self.sockets.write().unwrap().remove(&sid) {
            socket.close();
        }
        debug!("[sid={sid}] socket closed");
    }

    /**
     * Get a socket by its sid
     * Clones the socket ref to avoid holding the lock
     */
    pub fn get_socket(&self, sid: i64) -> Option<Arc<Socket<H>>> {
        self.sockets.read().unwrap().get(&sid).cloned()
    }
}
