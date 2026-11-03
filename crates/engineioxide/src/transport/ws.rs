//! The ws transport module is responsible for handling websocket connections
//! The only public function is [`new_req`] which is used to upgrade a http request to a websocket connection
//!
//! Other functions are used internally to handle the websocket connection through tasks and channels
//! and to handle upgrade from polling to ws

use std::{io, sync::Arc};

use futures_util::{
    SinkExt, StreamExt, TryStreamExt,
    stream::{SplitSink, SplitStream},
};
use http::{HeaderValue, Request, Response, StatusCode, request::Parts};
use smallvec::smallvec;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{
        self, Message,
        handshake::derive_accept_key,
        protocol::{Role, WebSocketConfig},
    },
};
use tokio_util::future::FutureExt as _;

use engineioxide_core::{Packet, ProtocolVersion, Sid, Str, TransportType};

use crate::{
    DisconnectReason, Socket, body::ResponseBody, config::EngineIoConfig, engine::EngineIo,
    errors::Error, handler::EngineIoHandler, socket::InternalRx, transport::make_open_packet,
};

#[cfg(feature = "ws-deflate")]
use super::ws_deflate;

/// Transforms a message right before it is written to the websocket.
pub(super) trait MessageEncoder: Send + 'static {
    /// An error means that the encoder is in an invalid state and that the connection must be closed.
    fn encode(&mut self, msg: Message) -> io::Result<Message>;
}

/// Messages are sent as they are.
impl MessageEncoder for () {
    #[inline(always)]
    fn encode(&mut self, msg: Message) -> io::Result<Message> {
        Ok(msg)
    }
}

/// Create a response for websocket upgrade
fn ws_response<B>(
    ws_key: &HeaderValue,
    extensions: Option<HeaderValue>,
) -> Response<ResponseBody<B>> {
    let derived = derive_accept_key(ws_key.as_bytes());
    let sec = derived.parse::<HeaderValue>().unwrap();
    let mut response = Response::builder()
        .status(StatusCode::SWITCHING_PROTOCOLS)
        .header(http::header::UPGRADE, HeaderValue::from_static("websocket"))
        .header(
            http::header::CONNECTION,
            HeaderValue::from_static("Upgrade"),
        )
        .header(http::header::SEC_WEBSOCKET_ACCEPT, sec);
    if let Some(extensions) = extensions {
        response = response.header(http::header::SEC_WEBSOCKET_EXTENSIONS, extensions);
    }
    response.body(ResponseBody::empty_response()).unwrap() // we are only using constants
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
        .ok_or(Error::InvalidWebSocketKey)?
        .clone();

    #[cfg(feature = "ws-deflate")]
    let extensions = engine
        .config
        .ws_per_message_deflate
        .as_ref()
        .and_then(|config| ws_deflate::negotiate(config, &parts.headers))
        .map(|negotiated| negotiated.response);
    #[cfg(not(feature = "ws-deflate"))]
    let extensions = None;

    tokio::spawn(async move {
        let conn = hyper::upgrade::on(req)
            .await
            .map(hyper_util::rt::TokioIo::new);

        let res = match conn {
            Ok(conn) => on_init(engine.clone(), conn, protocol, sid, parts).await,
            Err(_e) => {
                #[cfg(feature = "tracing")]
                tracing::debug!("ws upgrade error: {}", _e);
                return;
            }
        };

        match res {
            Ok(_) => {
                #[cfg(feature = "tracing")]
                tracing::debug!(?sid, "ws closed")
            }
            Err(Error::MultipleWebsocketRequests) => {}
            Err(_e) => {
                #[cfg(feature = "tracing")]
                tracing::debug!(?sid, "ws closed with error: {_e}");
                if let Some(sid) = sid {
                    engine.close_session(sid, DisconnectReason::TransportError);
                }
            }
        }
    });

    Ok(ws_response(&ws_key, extensions))
}

/// Handle a websocket connection upgrade
///
/// Sends an open packet if it is not an upgrade from a polling request
///
/// Read packets from the websocket and handle them, it will block until the connection is closed
#[cfg_attr(
    feature = "tracing",
    tracing::instrument(level = "trace", name = "init_websocket", skip(engine, conn, req_data))
)]
pub async fn on_init<H: EngineIoHandler, S>(
    engine: Arc<EngineIo<H>>,
    conn: S,
    protocol: ProtocolVersion,
    sid: Option<Sid>,
    req_data: Parts,
) -> Result<(), Error>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    #[cfg(feature = "ws-deflate")]
    if let Some(config) = &engine.config.ws_per_message_deflate
        && let Some(negotiated) = ws_deflate::negotiate(config, &req_data.headers)
    {
        let encoder = ws_deflate::Deflater::new(config, &negotiated);
        let conn = ws_deflate::InflateStream::new(conn, &engine.config, &negotiated);
        return serve(engine, conn, protocol, sid, req_data, encoder).await;
    }

    serve(engine, conn, protocol, sid, req_data, ()).await
}

/// Runs a websocket connection until it is closed.
async fn serve<H: EngineIoHandler, S, E: MessageEncoder>(
    engine: Arc<EngineIo<H>>,
    conn: S,
    protocol: ProtocolVersion,
    sid: Option<Sid>,
    req_data: Parts,
    encoder: E,
) -> Result<(), Error>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let ws_config = WebSocketConfig::default()
        .read_buffer_size(engine.config.ws_read_buffer_size)
        .max_message_size(Some(engine.config.ws_max_message_size))
        .max_frame_size(Some(engine.config.ws_max_frame_size));

    let ws_init = move || WebSocketStream::from_raw_socket(conn, Role::Server, Some(ws_config));
    let (socket, ws) = if let Some(sid) = sid {
        match engine.get_socket(sid) {
            None => return Err(Error::UnknownSessionID(sid)),
            Some(socket) if socket.is_ws() => return Err(Error::MultipleWebsocketRequests),
            Some(socket) => {
                let mut ws = ws_init().await;

                let upgrade_fut = upgrade_handshake::<H, S>(&socket, &mut ws);
                match tokio::time::timeout(engine.config.upgrade_timeout, upgrade_fut)
                    .with_cancellation_token(&socket.cancellation_token)
                    .await
                {
                    Some(Ok(res)) => res?,
                    Some(Err(_)) => {
                        #[cfg(feature = "tracing")]
                        tracing::debug!(?sid, "ws upgrade timed out, closing session");
                        engine.close_session(socket.id, DisconnectReason::TransportError);
                        return Err(Error::Upgrade);
                    }
                    None => {
                        #[cfg(feature = "tracing")]
                        tracing::debug!(?sid, "socket is being closed");
                        return Err(Error::Upgrade);
                    }
                }
                (socket, ws)
            }
        }
    } else {
        let socket = engine.create_session(protocol, TransportType::Websocket, req_data, false);

        #[cfg(feature = "tracing")]
        tracing::debug!("new websocket connection");

        let mut ws = ws_init().await;
        init_handshake(socket.id, &mut ws, &engine.config).await?;
        socket
            .clone()
            .spawn_heartbeat(engine.config.ping_interval, engine.config.ping_timeout);
        (socket, ws)
    };
    let (tx, rx) = ws.split();

    // Pipe between websocket and internal socket channel.
    tokio::spawn(forward_to_socket::<H, S, E>(socket.clone(), tx, encoder));

    // pipe between internal socket channel and websocket
    match forward_to_handler(&engine, rx, &socket)
        .with_cancellation_token(&socket.cancellation_token)
        .await
    {
        Some(Err(ref e)) => {
            let reason =
                Option::<DisconnectReason>::from(e).unwrap_or(DisconnectReason::TransportError);

            #[cfg(feature = "tracing")]
            tracing::debug!("error when handling packet: {:?}", e);
            engine.close_session(socket.id, reason);
        }
        Some(Ok(())) => {
            #[cfg(feature = "tracing")]
            tracing::debug!(sid = %socket.id, "ws transport was closed");
            engine.close_session(socket.id, DisconnectReason::TransportClose);
        }
        None => {
            #[cfg(feature = "tracing")]
            tracing::debug!(sid = %socket.id, "socket is closing");
        }
    }
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
            Message::Text(msg) => match Packet::parse(socket.protocol, ws_bytes_to_str(msg))? {
                Packet::Close => {
                    #[cfg(feature = "tracing")]
                    tracing::debug!("[sid={}] closing session", socket.id);
                    engine.close_session(socket.id, DisconnectReason::TransportClose);
                    break;
                }
                Packet::Pong | Packet::Ping => {
                    // A full channel means that the client is already known to be alive.
                    socket.heartbeat_tx.try_send(()).ok();
                    Ok::<(), Error>(())
                }
                Packet::Message(msg) => {
                    engine.handler.on_message(msg, socket.clone());
                    Ok(())
                }
                p => return Err(Error::BadPacket(p)),
            },
            Message::Binary(mut data) => {
                if socket.protocol == ProtocolVersion::V3 && !data.is_empty() {
                    // The first byte is the message type, which we don't need.
                    data = data.split_off(1);
                }
                engine.handler.on_binary(data, socket.clone());
                Ok(())
            }
            Message::Close(_) => {
                #[cfg(feature = "tracing")]
                tracing::debug!("websocket closed, closing session");
                engine.close_session(socket.id, DisconnectReason::TransportClose);
                break;
            }
            _ => {
                #[cfg(feature = "tracing")]
                tracing::debug!(sid = ?socket.id, "unexpected ws message");
                Ok(())
            }
        }?
    }
    Ok(())
}

/// Forwards all packets waiting to be sent to the websocket
///
/// The websocket stream is flushed only when the internal channel is drained
async fn forward_to_socket<H: EngineIoHandler, S, E: MessageEncoder>(
    socket: Arc<Socket<H::Data>>,
    mut tx: SplitSink<WebSocketStream<S>, Message>,
    mut encoder: E,
) where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    // A polling request that was in progress during the upgrade may still hold the lock.
    let lock = socket.internal_rx.lock();
    let Some(mut internal_rx) = lock
        .with_cancellation_token(&socket.cancellation_token)
        .await
    else {
        #[cfg(feature = "tracing")]
        tracing::debug!(sid = %socket.id, "session closed before starting the ws writer");
        return;
    };
    let InternalRx {
        buffered_rx,
        volatile_rx,
        ..
    } = &mut *internal_rx;

    // encode a message and write it to the websocket
    macro_rules! feed {
        ($msg:expr) => {
            match encoder.encode($msg) {
                Ok(msg) => tx.feed(msg).await,
                Err(_e) => {
                    #[cfg(feature = "tracing")]
                    tracing::debug!("[sid={}] error encoding packet: {}", socket.id, _e);
                    socket.close(DisconnectReason::TransportError);
                    Ok(())
                }
            }
        };
    }

    // map a packet to a websocket message
    // It is declared as a macro rather than a closure to avoid ownership issues
    macro_rules! map_fn {
            ($item:ident) => {
                let res = match $item {
                    Packet::Binary(bin) | Packet::BinaryV3(bin) => {
                        if socket.protocol == ProtocolVersion::V3 {
                            // v3 protocol requires packet type as the first byte.
                            // This requires a new buffer. This is OK as it is only for the V3 protocol.
                            let mut buff = Vec::with_capacity(bin.len() + 1);
                            buff.push(0x04);
                            buff.extend(bin);
                            feed!(Message::Binary(buff.into()))
                        } else {
                            feed!(Message::Binary(bin))
                        }
                    }
                    // A Noop Packet maybe sent by the server to upgrade from a polling connection
                    // In the case that the packet was not poll in time it will remain in the buffer and therefore
                    // it should be discarded here
                    Packet::Noop => Ok(()),
                    _ => {
                        let packet: String = $item.try_into().unwrap();
                        feed!(Message::Text(packet.into()))
                    }
                };
                if let Err(_e) = res {
                    #[cfg(feature = "tracing")]
                    tracing::debug!("[sid={}] error sending packet: {}", socket.id, _e);
                }
            };
        }

    // Forward buffered packets until the channel is closed or the session is cancelled.
    loop {
        tokio::select! {
            biased;
            items = buffered_rx.recv() => {
                match items {
                    Some(packets) => {
                        for item in packets {
                            map_fn!(item);
                        }
                        while let Ok(packets) = buffered_rx.try_recv() {
                            for item in packets {
                                map_fn!(item);
                            }
                        }
                    }
                    None => break
                }
            }
            _ = socket.cancellation_token.cancelled() => break,
            Ok(()) = volatile_rx.changed() => {
                let val = volatile_rx.borrow_and_update().clone();
                if let Some(packets) = val {
                    #[cfg(feature = "tracing")]
                    tracing::debug!(sid = ?socket.id, "ws volatile flush: {} packets", packets.len());
                    for item in packets {
                        map_fn!(item);
                    }
                }
            },
        }

        #[cfg(feature = "tracing")]
        tracing::trace!(sid = %socket.id, "ws flush");
        tx.flush().await.ok();
    }

    #[cfg(feature = "tracing")]
    tracing::trace!(sid = %socket.id, "ws closing flush");

    tx.flush().await.ok();
    tx.close().await.ok();
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
    let packet = Packet::Open(make_open_packet(TransportType::Websocket, sid, config));
    let packet: String = packet.into();
    ws.send(Message::Text(packet.into())).await?;
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
#[cfg_attr(
    feature = "tracing",
    tracing::instrument(level = "trace", skip(socket, ws))
)]
async fn upgrade_handshake<H: EngineIoHandler, S>(
    socket: &Arc<Socket<H::Data>>,
    ws: &mut WebSocketStream<S>,
) -> Result<(), Error>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    #[cfg(feature = "tracing")]
    tracing::debug!("starting websocket connection upgrade");

    // The socket is upgrading, from now on all polling
    // request will return a `Packet::Noop`
    socket.start_upgrade();

    // We send a last Noop request to close a potential waiting polling request.
    socket
        .internal_tx
        .send(smallvec![Packet::Noop])
        .await
        .map_err(|_| Error::Upgrade)?;

    // wait for any current polling connection to finish by waiting for the socket to be unlocked
    // All other polling connection will be immediately closed with a NOOP packet.
    let _lock = socket.internal_rx.lock().await;

    // Fetch the next packet from the ws stream, it should be a PingUpgrade packet
    let msg = match ws.next().await {
        Some(Ok(Message::Text(d))) => d,
        _ => Err(Error::Upgrade)?,
    };
    match Packet::parse(socket.protocol, ws_bytes_to_str(msg))? {
        Packet::PingUpgrade => {
            #[cfg(feature = "tracing")]
            tracing::debug!("received first ping upgrade");
            // Respond with a PongUpgrade packet
            ws.send(Message::Text(String::from(Packet::PongUpgrade).into()))
                .await?;
        }
        p => Err(Error::BadPacket(p))?,
    };

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
    match Packet::parse(socket.protocol, ws_bytes_to_str(msg))? {
        Packet::Upgrade => {
            #[cfg(feature = "tracing")]
            tracing::debug!("ws upgraded successfully")
        }
        p => Err(Error::BadPacket(p))?,
    };

    socket.upgrade_to_websocket();
    Ok(())
}

fn ws_bytes_to_str(bytes: tungstenite::Utf8Bytes) -> Str {
    // SAFETY: the bytes are guaranteed to be valid UTF-8 by the tungstenite parser
    unsafe { Str::from_bytes_unchecked(bytes.into()) }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bytes::Bytes;
    use futures_util::StreamExt;
    use http::Request;
    use tokio::io::DuplexStream;

    use super::*;

    const TIMEOUT: Duration = Duration::from_secs(1);

    #[derive(Debug)]
    struct StubHandler;
    impl EngineIoHandler for StubHandler {
        type Data = ();
        fn on_connect(self: Arc<Self>, _: Arc<Socket<()>>) {}
        fn on_disconnect(&self, _: Arc<Socket<()>>, _: DisconnectReason) {}
        fn on_message(self: &Arc<Self>, _: Str, _: Arc<Socket<()>>) {}
        fn on_binary(self: &Arc<Self>, _: Bytes, _: Arc<Socket<()>>) {}
    }

    fn socket() -> Arc<Socket<()>> {
        Arc::new(Socket::new(
            ProtocolVersion::V4,
            TransportType::Websocket,
            &EngineIoConfig::default(),
            Request::<()>::default().into_parts().0,
            Box::new(|_, _| ()),
            false,
        ))
    }

    async fn ws_pair() -> (WebSocketStream<DuplexStream>, WebSocketStream<DuplexStream>) {
        let (server, client) = tokio::io::duplex(1024);
        (
            WebSocketStream::from_raw_socket(server, Role::Server, None).await,
            WebSocketStream::from_raw_socket(client, Role::Client, None).await,
        )
    }

    /// A polling request can still hold the lock at the end of an upgrade.
    #[tokio::test]
    async fn writer_waits_for_the_lock_of_a_polling_request() {
        let socket = socket();
        let (server, mut client) = ws_pair().await;
        let (tx, _rx) = server.split();

        let polling = socket.internal_rx.lock().await;
        let writer = tokio::spawn(forward_to_socket::<StubHandler, _, ()>(
            socket.clone(),
            tx,
            (),
        ));
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(!writer.is_finished(), "the writer should wait for the lock");

        drop(polling);
        socket.emit(Str::from("hello")).unwrap();
        let msg = tokio::time::timeout(TIMEOUT, client.next())
            .await
            .expect("the writer should send the packet")
            .unwrap()
            .unwrap();
        assert_eq!(msg, Message::text("4hello"));
    }

    #[tokio::test]
    async fn writer_waiting_for_the_lock_stops_with_the_session() {
        let socket = socket();
        let (server, _client) = ws_pair().await;
        let (tx, _rx) = server.split();

        let _polling = socket.internal_rx.lock().await;
        let writer = tokio::spawn(forward_to_socket::<StubHandler, _, ()>(
            socket.clone(),
            tx,
            (),
        ));
        tokio::time::sleep(Duration::from_millis(20)).await;

        socket.cancellation_token.cancel();
        tokio::time::timeout(TIMEOUT, writer)
            .await
            .expect("the writer should stop")
            .unwrap();
    }
}
