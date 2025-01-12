//! The ws transport module is responsible for handling websocket connections
//! The only public function is [`new_req`] which is used to upgrade a http request to a websocket connection
//!
//! Other functions are used internally to handle the websocket connection through tasks and channels
//! and to handle upgrade from polling to ws

use std::sync::Arc;

use futures_util::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt, TryStreamExt,
};
use http::{request::Parts, HeaderValue, Request, Response, StatusCode};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    task::JoinHandle,
};
use tokio_tungstenite::{
    tungstenite::{handshake::derive_accept_key, protocol::Role, Message, Utf8Bytes},
    WebSocketStream,
};

use crate::{
    body::ResponseBody,
    config::EngineIoConfig,
    engine::EngineIo,
    errors::Error,
    handler::EngineIoHandler,
    packet::{OpenPacket, Packet},
    service::{ProtocolVersion, TransportType},
    sid::Sid,
    DisconnectReason, Socket, Str,
};

/// Create a response for websocket upgrade
fn ws_response<B>(ws_key: &HeaderValue) -> Result<Response<ResponseBody<B>>, http::Error> {
    let derived = derive_accept_key(ws_key.as_bytes());
    let sec = derived.parse::<HeaderValue>().unwrap();
    Response::builder()
        .status(StatusCode::SWITCHING_PROTOCOLS)
        .header(http::header::UPGRADE, HeaderValue::from_static("websocket"))
        .header(
            http::header::CONNECTION,
            HeaderValue::from_static("Upgrade"),
        )
        .header(http::header::SEC_WEBSOCKET_ACCEPT, sec)
        .body(ResponseBody::empty_response())
}

/// Upgrade a websocket request to create a websocket connection.
///
/// If a sid is provided in the query it means that is is upgraded from an existing HTTP polling request.
/// In this case the http polling request is closed and the SID is kept for the websocket
pub fn new_req<R: Send + 'static, B, H: EngineIoHandler>(
    engine: Arc<EngineIo<H>>,
    protocol: ProtocolVersion,
    sid: Option<Sid>,
    req: Request<R>,
) -> Result<Response<ResponseBody<B>>, Error> {
    let (parts, body) = req.into_parts();
    let req = Request::from_parts(parts.clone(), body);

    let ws_key = parts
        .headers
        .get("Sec-WebSocket-Key")
        .ok_or(Error::HttpErrorResponse(StatusCode::BAD_REQUEST))?
        .clone();

    tokio::spawn(async move {
        let conn = hyper::upgrade::on(req)
            .await
            .map(hyper_util::rt::TokioIo::new);
        let res = match conn {
            Ok(conn) => on_init(engine, conn, protocol, sid, parts).await,
            Err(_e) => {
                #[cfg(feature = "tracing")]
                tracing::debug!("ws upgrade error: {}", _e);
                return;
            }
        };

        match res {
            Ok(_) => {
                #[cfg(feature = "tracing")]
                tracing::debug!("ws closed")
            }
            Err(_e) => {
                #[cfg(feature = "tracing")]
                tracing::debug!("ws closed with error: {:?}", _e)
            }
        }
    });

    Ok(ws_response(&ws_key)?)
}

/// Handle a websocket connection upgrade
///
/// Sends an open packet if it is not an upgrade from a polling request
///
/// Read packets from the websocket and handle them, it will block until the connection is closed
async fn on_init<H: EngineIoHandler, S>(
    engine: Arc<EngineIo<H>>,
    conn: S,
    protocol: ProtocolVersion,
    sid: Option<Sid>,
    req_data: Parts,
) -> Result<(), Error>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let ws_init = move || WebSocketStream::from_raw_socket(conn, Role::Server, None);
    let (socket, ws) = if let Some(sid) = sid {
        match engine.get_socket(sid) {
            None => return Err(Error::UnknownSessionID(sid)),
            Some(socket) if socket.is_ws() => return Err(Error::Upgrade),
            Some(socket) => {
                let mut ws = ws_init().await;
                upgrade_handshake::<H, S>(&socket, &mut ws).await?;
                (socket, ws)
            }
        }
    } else {
        let socket = engine.create_session(
            protocol,
            TransportType::Websocket,
            req_data,
            #[cfg(feature = "v3")]
            false,
        );
        #[cfg(feature = "tracing")]
        tracing::debug!("[sid={}] new websocket connection", socket.id);
        let mut ws = ws_init().await;
        init_handshake(socket.id, &mut ws, &engine.config).await?;
        socket
            .clone()
            .spawn_heartbeat(engine.config.ping_interval, engine.config.ping_timeout);
        (socket, ws)
    };
    let (tx, rx) = ws.split();
    let rx_handle = forward_to_socket::<H, S>(socket.clone(), tx);

    if let Err(ref e) = forward_to_handler(&engine, rx, &socket).await {
        #[cfg(feature = "tracing")]
        tracing::debug!("[sid={}] error when handling packet: {:?}", socket.id, e);
        if let Some(reason) = e.into() {
            engine.close_session(socket.id, reason);
        }
    } else {
        engine.close_session(socket.id, DisconnectReason::TransportClose);
    }
    rx_handle.abort();
    Ok(())
}

/// Forwards all packets received from a websocket to a EngineIo [`Socket`]
async fn forward_to_handler<H: EngineIoHandler, S>(
    engine: &Arc<EngineIo<H>>,
    mut rx: SplitStream<WebSocketStream<S>>,
    socket: &Arc<Socket<H::Data>>,
) -> Result<(), Error>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    while let Some(msg) = rx.try_next().await? {
        match msg {
            Message::Text(msg) => {
                match Packet::try_from(unsafe { Str::from_bytes_unchecked(msg.into()) })? {
                    Packet::Close => {
                        #[cfg(feature = "tracing")]
                        tracing::debug!("[sid={}] closing session", socket.id);
                        engine.close_session(socket.id, DisconnectReason::TransportClose);
                        break;
                    }
                    Packet::Pong | Packet::Ping => socket
                        .heartbeat_tx
                        .try_send(())
                        .map_err(|_| Error::HeartbeatTimeout),
                    Packet::Message(msg) => {
                        engine.handler.on_message(msg, socket.clone());
                        Ok(())
                    }
                    p => return Err(Error::BadPacket(p)),
                }
            }
            Message::Binary(mut data) => {
                #[cfg(feature = "v3")]
                if socket.protocol == ProtocolVersion::V3 && !data.is_empty() {
                    // The first byte is the message type, which we don't need.
                    data = data.slice(1..);
                }
                engine.handler.on_binary(data, socket.clone());
                Ok(())
            }
            Message::Close(_) => break,
            _ => {
                #[cfg(feature = "tracing")]
                tracing::debug!("[sid={}] unexpected ws message", socket.id);
                Ok(())
            }
        }?
    }
    Ok(())
}

/// Forwards all packets waiting to be sent to the websocket
///
/// The websocket stream is flushed only when the internal channel is drained
fn forward_to_socket<H: EngineIoHandler, S>(
    socket: Arc<Socket<H::Data>>,
    mut tx: SplitSink<WebSocketStream<S>, Message>,
) -> JoinHandle<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    // Pipe between websocket and internal socket channel
    tokio::spawn(async move {
        let mut internal_rx = socket.internal_rx.try_lock().unwrap();

        // map a packet to a websocket message
        // It is declared as a macro rather than a closure to avoid ownership issues
        macro_rules! map_fn {
            ($item:ident) => {
                let res = match $item {
                    Packet::Binary(bin) => tx.feed(Message::Binary(bin)).await,
                    Packet::BinaryV3(bin) => {
                        // v3 protocol requires packet type as the first byte
                        let mut buf = Vec::with_capacity(bin.len() + 1);
                        buf.push(0x04);
                        buf.extend_from_slice(&bin);
                        tx.feed(Message::Binary(bin)).await
                    }
                    Packet::Close => {
                        tx.send(Message::Close(None)).await.ok();
                        internal_rx.close();
                        break;
                    },
                    // A Noop Packet maybe sent by the server to upgrade from a polling connection
                    // In the case that the packet was not poll in time it will remain in the buffer and therefore
                    // it should be discarded here
                    Packet::Noop => Ok(()),
                    _ => {
                        let packet: String = $item.try_into().unwrap();
                        tx.feed(Message::Text(Utf8Bytes::from(packet))).await
                    }
                };
                if let Err(_e) = res {
                    #[cfg(feature = "tracing")]
                    tracing::debug!("[sid={}] error sending packet: {}", socket.id, _e);
                }
            };
        }

        while let Some(items) = internal_rx.recv().await {
            for item in items {
                map_fn!(item);
            }
            // For every available packet we continue to send until the channel is drained
            while let Ok(items) = internal_rx.try_recv() {
                for item in items {
                    map_fn!(item);
                }
            }

            tx.flush().await.ok();
        }
    })
}
/// Send a Engine.IO [`OpenPacket`] to initiate a websocket connection
async fn init_handshake<S>(
    sid: Sid,
    ws: &mut WebSocketStream<S>,
    config: &EngineIoConfig,
) -> Result<(), Error>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let packet = Packet::Open(OpenPacket::new(TransportType::Websocket, sid, config));
    let value: String = packet.try_into()?;
    ws.send(Message::Text(Utf8Bytes::from(value))).await?;
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
#[cfg_attr(feature = "tracing", tracing::instrument(skip(socket, ws), fields(sid = socket.id.to_string())))]
async fn upgrade_handshake<H: EngineIoHandler, S>(
    socket: &Arc<Socket<H::Data>>,
    ws: &mut WebSocketStream<S>,
) -> Result<(), Error>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    #[cfg(feature = "tracing")]
    tracing::debug!("websocket connection upgrade");

    // Fetch the next packet from the ws stream, it should be a PingUpgrade packet
    let msg = match ws.next().await {
        Some(Ok(Message::Text(d))) => d,
        _ => Err(Error::Upgrade)?,
    };
    match Packet::try_from(unsafe { Str::from_bytes_unchecked(msg.into()) })? {
        Packet::PingUpgrade => {
            // Respond with a PongUpgrade packet
            let msg: String = Packet::PongUpgrade.try_into()?;
            ws.send(Message::Text(Utf8Bytes::from(msg))).await?;
        }
        p => Err(Error::BadPacket(p))?,
    };

    // send a NOOP packet to any pending polling request so it closes gracefully
    socket.send(Packet::Noop)?;

    // Fetch the next packet from the ws stream, it should be an Upgrade packet
    let msg = match ws.next().await {
        Some(Ok(Message::Text(d))) => d,
        Some(Ok(Message::Close(_))) => {
            #[cfg(feature = "tracing")]
            tracing::debug!("ws stream closed before upgrade");
            Err(Error::Upgrade)?
        }
        _ => {
            #[cfg(feature = "tracing")]
            tracing::debug!("unexpected ws message before upgrade");
            Err(Error::Upgrade)?
        }
    };
    match Packet::try_from(unsafe { Str::from_bytes_unchecked(msg.into()) })? {
        Packet::Upgrade => {
            #[cfg(feature = "tracing")]
            tracing::debug!("ws upgraded successful")
        }
        p => Err(Error::BadPacket(p))?,
    };

    // wait for any polling connection to finish by waiting for the socket to be unlocked
    let _ = socket.internal_rx.lock().await;
    socket.upgrade_to_websocket();
    Ok(())
}
