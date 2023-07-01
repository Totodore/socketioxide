#![deny(clippy::await_holding_lock)]
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use crate::{sid_generator::Sid, protocol::ProtocolVersion, payload::{Payload, PACKET_SEPARATOR}};
use crate::{
    body::ResponseBody,
    config::EngineIoConfig,
    errors::Error,
    futures::{http_response, ws_response},
    handler::EngineIoHandler,
    packet::{OpenPacket, Packet},
    service::TransportType,
    sid_generator::generate_sid,
    socket::{ConnectionType, Socket, SocketReq},
};
use bytes::Buf;
use cfg_if::cfg_if;
use futures::{stream::SplitStream, SinkExt, StreamExt, TryStreamExt};
use http::{Request, Response, StatusCode};
use hyper::upgrade::Upgraded;
use std::fmt::Debug;
use tokio_tungstenite::{
    tungstenite::{protocol::Role, Message},
    WebSocketStream,
};
use tracing::debug;

type SocketMap<T> = RwLock<HashMap<Sid, Arc<T>>>;
/// Abstract engine implementation for Engine.IO server for http polling and websocket
/// It handle all the connection logic and dispatch the packets to the socket
pub struct EngineIo<H: EngineIoHandler>
{
    sockets: SocketMap<Socket<H>>,
    handler: H,
    pub config: EngineIoConfig,
}

impl<H: EngineIoHandler> EngineIo<H>
{
    /// Create a new Engine.IO server with a handler and a config
    pub fn new(handler: H, config: EngineIoConfig) -> Self {
        Self {
            sockets: RwLock::new(HashMap::new()),
            config,
            handler,
        }
    }
}

impl<H: EngineIoHandler> EngineIo<H>
{
    /// Handle Open request
    /// Create a new socket and add it to the socket map
    /// Start the heartbeat task
    /// Send an open packet
    pub(crate) fn on_open_http_req<B, R>(
        self: Arc<Self>,
        protocol: ProtocolVersion,
        req: Request<R>,
    ) -> Result<Response<ResponseBody<B>>, Error>
    where
        B: Send + 'static,
    {
        let engine = self.clone();
        let close_fn = Box::new(move |sid: Sid| engine.close_session(sid));
        let sid = generate_sid();
        let socket = Socket::new(
            sid,
            protocol.clone(),
            ConnectionType::Http,
            &self.config,
            SocketReq::from(req.into_parts().0),
            close_fn,
        );
        let socket = Arc::new(socket);
        {
            self.sockets.write().unwrap().insert(sid, socket.clone());
        }
        socket
            .clone()
            .spawn_heartbeat(self.config.ping_interval, self.config.ping_timeout);
        self.handler.on_connect(&socket);

        let packet = OpenPacket::new(TransportType::Polling, sid, &self.config);
        #[allow(unused_mut)]
        let mut packet: String = Packet::Open(packet).try_into()?;
        cfg_if! {
            if #[cfg(feature = "v3")] {
                if protocol == ProtocolVersion::V3 {
                    packet = format!("{}:{}", packet.chars().count(), packet);
                }
            }
        }
        http_response(StatusCode::OK, packet).map_err(Error::Http)
    }

    /// Handle http polling request
    ///
    /// If there is packet in the socket buffer, it will be sent immediately
    /// Otherwise it will wait for the next packet to be sent from the socket
    pub(crate) async fn on_polling_http_req<B>(
        self: Arc<Self>,
        protocol: ProtocolVersion,
        sid: Sid,
    ) -> Result<Response<ResponseBody<B>>, Error>
    where
        B: Send + 'static,
    {
        let socket = self
            .get_socket(sid)
            .ok_or(Error::UnknownSessionID(sid))
            .and_then(|s| s.is_http().then(|| s).ok_or(Error::TransportMismatch))?;

        // If the socket is already locked, it means that the socket is being used by another request
        // In case of multiple http polling, session should be closed
        let mut rx = match socket.internal_rx.try_lock() {
            Ok(s) => s,
            Err(_) => {
                socket.close();
                return Err(Error::HttpErrorResponse(StatusCode::BAD_REQUEST));
            }
        };

        debug!("[sid={sid}] polling request");
        let mut data = String::new();

        // Send all packets in the buffer
        while let Ok(packet) = rx.try_recv() {
            debug!("sending packet: {:?}", packet);
            let packet: String = packet.try_into().unwrap();
            if !data.is_empty() {
                match protocol {
                    ProtocolVersion::V3 => data.push_str(&format!("{}:", packet.chars().count())),
                    ProtocolVersion::V4 => data.push(std::char::from_u32(PACKET_SEPARATOR as u32).unwrap()),
                }
            } else if protocol == ProtocolVersion::V3 {
                data.push_str(&format!("{}:", packet.chars().count()));
            }
            data.push_str(&packet);
        }

        // If there is no packet in the buffer, wait for the next packet
        if data.is_empty() {
            let packet = rx.recv().await.ok_or(Error::Aborted)?;
            let packet: String = packet.try_into().unwrap();
            cfg_if! {
                if #[cfg(feature = "v3")] {
                    if protocol == ProtocolVersion::V3 {
                        data.push_str(&format!("{}:", packet.chars().count()));
                    }
                }
            }
            data.push_str(&packet);
        }
        Ok(http_response(StatusCode::OK, data)?)
    }

    /// Handle http polling post request
    ///
    /// Split the body into packets and send them to the internal socket
    pub(crate) async fn on_post_http_req<R, B>(
        self: Arc<Self>,
        protocol: ProtocolVersion,
        sid: Sid,
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
        
        let socket = self
            .get_socket(sid)
            .ok_or(Error::UnknownSessionID(sid))
            .and_then(|s| s.is_http().then(|| s).ok_or(Error::TransportMismatch))?;

        let packets = Payload::new(protocol, body.reader());

        for p in packets {
            let raw_packet = p.map_err(|e| {
                debug!("error parsing packets: {:?}", e);
                self.close_session(sid);
                Error::HttpErrorResponse(
                    StatusCode::BAD_REQUEST,
                )
            })?;

            let packet = match Packet::try_from(raw_packet) {
                Ok(p) => p,
                Err(e) => {
                    debug!("[sid={sid}] error parsing packet: {:?}", e);
                    self.close_session(sid);
                    return Err(e);
                }
            };

            match packet {
                Packet::Close => {
                    debug!("[sid={sid}] closing session");
                    socket.send(Packet::Noop)?;
                    self.close_session(sid);
                    break;
                }
                Packet::Pong | Packet::Ping => {
                    socket
                        .pong_tx
                        .try_send(())
                        .map_err(|_| Error::HeartbeatTimeout)
                },
                Packet::Message(msg) => {
                    self.handler.on_message(msg, &socket);
                    Ok(())
                }
                Packet::Binary(bin) | Packet::BinaryV3(bin) => {
                    self.handler.on_binary(bin, &socket);
                    Ok(())
                }
                p => {
                    debug!("[sid={sid}] bad packet received: {:?}", &p);
                    Err(Error::BadPacket(p))
                }
            }?;
        }
        Ok(http_response(StatusCode::OK, "ok")?)
    }

    /// Upgrade a websocket request to create a websocket connection.
    ///
    /// If a sid is provided in the query it means that is is upgraded from an existing HTTP polling request. In this case
    /// the http polling request is closed and the SID is kept for the websocket
    pub(crate) fn on_ws_req<R, B>(
        self: Arc<Self>,
        protocol: ProtocolVersion,
        sid: Option<Sid>,
        req: Request<R>,
    ) -> Result<Response<ResponseBody<B>>, Error> {
        let (parts, _) = req.into_parts();
        let ws_key = parts
            .headers
            .get("Sec-WebSocket-Key")
            .ok_or(Error::HttpErrorResponse(StatusCode::BAD_REQUEST))?
            .clone();
        let req_data = SocketReq::from(&parts);

        let req = Request::from_parts(parts, ());
        tokio::spawn(async move {
            match hyper::upgrade::on(req).await {
                Ok(conn) => match self.on_ws_req_init(conn, protocol, sid, req_data).await {
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
    /// Read packets from the websocket and handle them, it will block until the connection is closed
    async fn on_ws_req_init(
        self: Arc<Self>,
        conn: Upgraded,
        protocol: ProtocolVersion,
        sid: Option<Sid>,
        req_data: SocketReq,
    ) -> Result<(), Error> {
        let ws_init = move || WebSocketStream::from_raw_socket(conn, Role::Server, None);
        let (socket, ws) = if let Some(sid) = sid {
            match self.get_socket(sid) {
                None => return Err(Error::UnknownSessionID(sid)),
                Some(socket) if socket.is_ws() => return Err(Error::UpgradeError),
                Some(_) => {
                    debug!("[sid={sid}] websocket connection upgrade");
                    let mut ws = ws_init().await;
                    self.ws_upgrade_handshake(protocol.clone(), sid, &mut ws).await?;
                    (self.get_socket(sid).unwrap(), ws)
                }
            }
        } else {
            let sid = generate_sid();
            let engine = self.clone();
            let close_fn = Box::new(move |sid: Sid| engine.close_session(sid));
            let socket = Socket::new(
                sid,
                protocol.clone(),
                ConnectionType::WebSocket,
                &self.config,
                req_data,
                close_fn,
            );
            let socket = Arc::new(socket);
            {
                self.sockets.write().unwrap().insert(sid, socket.clone());
            }
            debug!("[sid={sid}] new websocket connection");
            let mut ws = ws_init().await;
            self.ws_init_handshake(sid, &mut ws).await?;
            socket
                .clone()
                .spawn_heartbeat(self.config.ping_interval, self.config.ping_timeout);
            (socket, ws)
        };
        let (mut tx, rx) = ws.split();

        // Pipe between websocket and internal socket channel
        let rx_socket = socket.clone();
        let rx_handle = tokio::spawn(async move {
            let mut socket_rx = rx_socket.internal_rx.try_lock().unwrap();
            while let Some(item) = socket_rx.recv().await {
                let res = match item {
                    Packet::Binary(bin) | Packet::BinaryV3(bin) => tx.send(Message::Binary(bin)).await,
                    Packet::Close => tx.send(Message::Close(None)).await,
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

        self.handler.on_connect(&socket);
        if let Err(e) = self.ws_forward_to_handler(rx, &socket).await {
            debug!("[sid={}] error when handling packet: {:?}", socket.sid, e);
        }
        self.close_session(socket.sid);
        rx_handle.abort();
        Ok(())
    }

    /// Forwards all packets received from a websocket to a EngineIo [`Socket`]
    async fn ws_forward_to_handler(
        &self,
        mut rx: SplitStream<WebSocketStream<Upgraded>>,
        socket: &Arc<Socket<H>>,
    ) -> Result<(), Error> {
        while let Ok(msg) = rx.try_next().await {
            let Some(msg) = msg else { continue };
            match msg {
                Message::Text(msg) => match Packet::try_from(msg.clone())? {
                    Packet::Close => {
                        debug!("[sid={}] closing session", socket.sid);
                        self.close_session(socket.sid);
                        break;
                    }
                    Packet::Pong | Packet::Ping => {                        
                        socket
                            .pong_tx
                            .try_send(())
                            .map_err(|_| Error::HeartbeatTimeout)
                    },
                    Packet::Message(msg) => {
                        self.handler.on_message(msg, socket);
                        Ok(())
                    }
                    p => return Err(Error::BadPacket(p)),
                },
                Message::Binary(data) => {
                    self.handler.on_binary(data, socket);
                    Ok(())
                }
                Message::Close(_) => break,
                _ => panic!("[sid={}] unexpected ws message", socket.sid),
            }?
        }
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
        protocol: ProtocolVersion,
        sid: Sid,
        ws: &mut WebSocketStream<Upgraded>,
    ) -> Result<(), Error> {
        let socket = self.get_socket(sid).unwrap();

        cfg_if! {
            if #[cfg(feature = "v4")] {
                // send a NOOP packet to any pending polling request so it closes gracefully'
                if protocol == ProtocolVersion::V4 {
                    socket.send(Packet::Noop)?;
                }
            }
        }

        // Fetch the next packet from the ws stream, it should be a PingUpgrade packet
        let msg = match ws.next().await {
            Some(Ok(Message::Text(d))) => d,
            _ => Err(Error::UpgradeError)?,
        };
        match Packet::try_from(msg)? {
            Packet::PingUpgrade => {
                // Respond with a PongUpgrade packet
                ws.send(Message::Text(Packet::PongUpgrade.try_into()?))
                    .await?;
            }
            p => Err(Error::BadPacket(p))?,
        };

        cfg_if! {
            if #[cfg(feature = "v3")] {
                // send a NOOP packet to any pending polling request so it closes gracefully
                // V3 protocol introduce _paused_ polling transport which require to close 
                // the polling request **after** the ping/pong handshake
                if protocol == ProtocolVersion::V3 {
                    socket.send(Packet::Noop)?;
                }
            }
        }

        // Fetch the next packet from the ws stream, it should be an Upgrade packet
        let msg = match ws.next().await {
            Some(Ok(Message::Text(d))) => d,
            Some(Ok(Message::Close(_))) => {
                debug!("ws stream closed before upgrade");
                Err(Error::UpgradeError)?
            },
            _ => {
                debug!("unexpected ws message before upgrade");
                Err(Error::UpgradeError)?
            },
        };
        match Packet::try_from(msg)? {
            Packet::Upgrade => debug!("[sid={sid}] ws upgraded successful"),
            p => Err(Error::BadPacket(p))?,
        };

        // wait for any polling connection to finish by waiting for the socket to be unlocked
        let _ = socket.internal_rx.lock().await;
        socket.upgrade_to_websocket();
        Ok(())
    }

    /// Send a Engine.IO [`OpenPacket`] to initiate a websocket connection
    async fn ws_init_handshake(
        &self,
        sid: Sid,
        ws: &mut WebSocketStream<Upgraded>,
    ) -> Result<(), Error> {
        let packet = Packet::Open(OpenPacket::new(TransportType::Websocket, sid, &self.config));
        ws.send(Message::Text(packet.try_into()?)).await?;
        Ok(())
    }

    /// Close an engine.io session by removing the socket from the socket map and closing the socket
    /// It should be the only way to close a session and to remove a socket from the socket map
    fn close_session(&self, sid: Sid) {
        let socket = self.sockets.write().unwrap().remove(&sid);
        if let Some(socket) = socket {
            self.handler.on_disconnect(&socket);
            socket.abort_heartbeat();
            debug!(
                "remaining sockets: {:?}",
                self.sockets.read().unwrap().len()
            );
        } else {
            debug!("[sid={sid}] socket not found");
        }
    }

    /// Get a socket by its sid
    /// Clones the socket ref to avoid holding the lock
    pub fn get_socket(&self, sid: Sid) -> Option<Arc<Socket<H>>> {
        self.sockets.read().unwrap().get(&sid).cloned()
    }
}


#[cfg(test)]
mod tests {
    use async_trait::async_trait;

    use super::*;

    #[derive(Debug, Clone)]
    struct MockHandler;

    #[async_trait]
    impl EngineIoHandler for MockHandler {
        type Data = ();

        fn on_connect(&self, socket: &Socket<Self>) {
            println!("socket connect {}", socket.sid);
        }

        fn on_disconnect(&self, socket: &Socket<Self>) {
            println!("socket disconnect {}", socket.sid);
        }

        fn on_message(&self, msg: String, socket: &Socket<Self>) {
            println!("Ping pong message {:?}", msg);
            socket.emit(msg).ok();
        }

        fn on_binary(&self, data: Vec<u8>, socket: &Socket<Self>) {
            println!("Ping pong binary message {:?}", data);
            socket.emit_binary(data).ok();
        }
    }
}
