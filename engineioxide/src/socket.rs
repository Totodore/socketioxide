use std::{
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc,
    },
    time::Duration,
};

use http::{request::Parts, Uri};
use tokio::{
    sync::{
        mpsc::{self},
        mpsc::{error::TrySendError, Receiver},
        Mutex,
    },
    task::JoinHandle,
};
use tokio_tungstenite::tungstenite;
use tracing::debug;

use crate::{
    config::EngineIoConfig, errors::Error, packet::Packet, peekable::PeekableReceiver,
    service::ProtocolVersion,
};
use crate::{sid_generator::Sid, transport::TransportType};

/// Http Request data used to create a socket
#[derive(Debug)]
pub struct SocketReq {
    /// Request URI
    pub uri: Uri,

    /// Request headers
    pub headers: http::HeaderMap,
}

/// Convert a `Parts` struct to a `SocketReq` by cloning the fields.
impl From<&Parts> for SocketReq {
    fn from(parts: &Parts) -> Self {
        Self {
            uri: parts.uri.clone(),
            headers: parts.headers.clone(),
        }
    }
}
/// Convert a `Parts` struct to a `SocketReq` by moving the fields.
impl From<Parts> for SocketReq {
    fn from(parts: Parts) -> Self {
        Self {
            uri: parts.uri,
            headers: parts.headers,
        }
    }
}

/// A [`DisconnectReason`] represents the reason why a [`Socket`] was closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisconnectReason {
    /// The client gracefully closed the connection
    TransportClose,
    /// The client sent multiple polling requests at the same time (it is forbidden according to the engine.io protocol)
    MultipleHttpPollingError,
    /// The client sent a bad request / the packet could not be parsed correctly
    PacketParsingError,
    /// An error occured in the transport layer
    /// (e.g. the client closed the connection without sending a close packet)
    TransportError,
    /// The client did not respond to the heartbeat
    HeartbeatTimeout,
    /// The server is being closed
    ClosingServer,
}

/// Convert an [`Error`] to a [`DisconnectReason`] if possible
/// This is used to notify the [`Handler`](crate::handler::EngineIoHandler) of the reason why a [`Socket`] was closed
/// If the error cannot be converted to a [`DisconnectReason`] it means that the error was not fatal and the [`Socket`] can be kept alive
impl From<&Error> for Option<DisconnectReason> {
    fn from(err: &Error) -> Self {
        use Error::*;
        match err {
            WsTransport(tungstenite::Error::ConnectionClosed) => None,
            WsTransport(_) | Io(_) => Some(DisconnectReason::TransportError),
            BadPacket(_) | Serialize(_) | Base64(_) | StrUtf8(_) | PayloadTooLarge
            | InvalidPacketLength => Some(DisconnectReason::PacketParsingError),
            HeartbeatTimeout => Some(DisconnectReason::HeartbeatTimeout),
            _ => None,
        }
    }
}
/// A [`Socket`] represents a connection to the server.
/// It is agnostic to the [`TransportType`](crate::service::TransportType).
/// It handles :
/// * the packet communication between with the `Engine`
/// and the user defined [`Handler`](crate::handler::EngineIoHandler).
/// * the user defined [`Data`](crate::handler::EngineIoHandler::Data) bound to the socket.
/// * the heartbeat job that verify that the connection is still up by sending packets periodically.
pub struct Socket<D>
where
    D: Default + Send + Sync + 'static,
{
    /// The socket id
    pub id: Sid,

    /// The protocol version used by the socket
    pub protocol: ProtocolVersion,

    /// The transport type represented as a bitfield
    /// It is represented as a bitfield to allow the use of an [`AtomicU8`] so it can be shared between threads
    /// without any mutex
    transport: AtomicU8,

    /// Channel to receive [`Packet`] from the connection
    ///
    /// It is used and managed by the [`EngineIo`](crate::engine) struct depending on the transport type
    ///
    /// It is locked if [`EngineIo`](crate::engine) is currently reading from it :
    /// * In case of polling transport it will be locked and released for each request
    /// * In case of websocket transport it will be always locked until the connection is closed
    ///
    /// It will be closed when a [`Close`](Packet::Close) packet is received:
    /// * From the [encoder](crate::service::encoder) if the transport is polling
    /// * From the fn [`on_ws_req_init`](crate::engine::EngineIo) if the transport is websocket
    /// * Automatically via the [`close_session fn`](crate::engine::EngineIo::close_session) as a fallback. Because with polling transport, if the client is not currently polling then the encoder will never be able to close the channel
    pub(crate) internal_rx: Mutex<PeekableReceiver<Packet>>,

    /// Channel to send [Packet] to the internal connection
    internal_tx: mpsc::Sender<Packet>,

    /// Internal channel to receive Pong [`Packets`](Packet) (v4 protocol) or Ping (v3 protocol) in the heartbeat job
    /// which is running in a separate task
    heartbeat_rx: Mutex<Receiver<()>>,
    /// Channel to send Ping [`Packets`](Packet) (v4 protocol) or Ping (v3 protocol) from the connexion to the heartbeat job
    /// which is running in a separate task
    pub(crate) heartbeat_tx: mpsc::Sender<()>,
    /// Handle to the heartbeat job so that it can be aborted when the socket is closed
    heartbeat_handle: Mutex<Option<JoinHandle<()>>>,

    /// Function to call when the socket is closed
    close_fn: Box<dyn Fn(Sid, DisconnectReason) + Send + Sync>,
    /// User data bound to the socket
    pub data: D,

    /// Http Request data used to create a socket
    pub req_data: Arc<SocketReq>,

    /// If the client supports binary packets (via polling XHR2)
    #[cfg(feature = "v3")]
    pub supports_binary: bool,
}

impl<D> Socket<D>
where
    D: Default + Send + Sync + 'static,
{
    pub(crate) fn new(
        sid: Sid,
        protocol: ProtocolVersion,
        transport: TransportType,
        config: &EngineIoConfig,
        req_data: SocketReq,
        close_fn: Box<dyn Fn(Sid, DisconnectReason) + Send + Sync>,
        #[cfg(feature = "v3")] supports_binary: bool,
    ) -> Self {
        let (internal_tx, internal_rx) = mpsc::channel(config.max_buffer_size);
        let (heartbeat_tx, heartbeat_rx) = mpsc::channel(1);

        Self {
            id: sid,
            protocol,
            transport: AtomicU8::new(transport as u8),

            internal_rx: Mutex::new(PeekableReceiver::new(internal_rx)),
            internal_tx,

            heartbeat_rx: Mutex::new(heartbeat_rx),
            heartbeat_tx,
            heartbeat_handle: Mutex::new(None),
            close_fn,

            data: D::default(),
            req_data: req_data.into(),

            #[cfg(feature = "v3")]
            supports_binary,
        }
    }

    /// Abort the heartbeat job if it is running
    pub(crate) fn abort_heartbeat(&self) {
        if let Ok(Some(handle)) = self.heartbeat_handle.try_lock().map(|mut h| h.take()) {
            handle.abort();
        }
    }

    /// Sends a packet to the connection.
    pub(crate) fn send(&self, packet: Packet) -> Result<(), TrySendError<Packet>> {
        debug!("[sid={}] sending packet: {:?}", self.id, packet);
        self.internal_tx.try_send(packet)?;
        Ok(())
    }

    /// Spawn the heartbeat job
    ///
    /// Keep a handle to the job so that it can be aborted when the socket is closed
    pub(crate) fn spawn_heartbeat(self: Arc<Self>, interval: Duration, timeout: Duration) {
        let socket = self.clone();

        let handle = tokio::spawn(async move {
            if let Err(e) = socket.heartbeat_job(interval, timeout).await {
                socket.close(DisconnectReason::HeartbeatTimeout);
                debug!("[sid={}] heartbeat error: {:?}", socket.id, e);
            }
        });
        self.heartbeat_handle
            .try_lock()
            .expect("heartbeat handle mutex should not be locked twice")
            .replace(handle);
    }

    /// Heartbeat is sent every `interval` milliseconds and the client or server (depending on the protocol) is expected to respond within `timeout` milliseconds.
    ///
    /// If the client or server does not respond within the timeout, the connection is closed.
    #[cfg(all(feature = "v3", feature = "v4"))]
    async fn heartbeat_job(&self, interval: Duration, timeout: Duration) -> Result<(), Error> {
        match self.protocol {
            ProtocolVersion::V3 => self.heartbeat_job_v3(interval, timeout).await,
            ProtocolVersion::V4 => self.heartbeat_job_v4(interval, timeout).await,
        }
    }

    /// Heartbeat is sent every `interval` milliseconds by the client and the server is expected to respond within `timeout` milliseconds.
    ///
    /// If the client or server does not respond within the timeout, the connection is closed.
    #[cfg(feature = "v3")]
    #[cfg(not(feature = "v4"))]
    async fn heartbeat_job(&self, interval: Duration, timeout: Duration) -> Result<(), Error> {
        self.heartbeat_job_v3(interval, timeout).await
    }

    /// Heartbeat is sent every `interval` milliseconds and the client is expected to respond within `timeout` milliseconds.
    ///
    /// If the client does not respond within the timeout, the connection is closed.
    #[cfg(feature = "v4")]
    #[cfg(not(feature = "v3"))]
    async fn heartbeat_job(&self, interval: Duration, timeout: Duration) -> Result<(), Error> {
        self.heartbeat_job_v4(interval, timeout).await
    }

    /// Heartbeat is sent every `interval` milliseconds and the client is expected to respond within `timeout` milliseconds.
    ///
    /// If the client does not respond within the timeout, the connection is closed.
    #[cfg(feature = "v4")]
    async fn heartbeat_job_v4(&self, interval: Duration, timeout: Duration) -> Result<(), Error> {
        let mut heartbeat_rx = self
            .heartbeat_rx
            .try_lock()
            .expect("Pong rx should be locked only once");

        let instant = tokio::time::Instant::now();
        let mut interval_tick = tokio::time::interval(interval);
        interval_tick.tick().await;
        // Sleep for an interval minus the time it took to get here
        tokio::time::sleep(interval.saturating_sub(Duration::from_millis(
            15 + instant.elapsed().as_millis() as u64,
        )))
        .await;

        debug!("[sid={}] heartbeat sender routine started", self.id);

        loop {
            // Some clients send the pong packet in first. If that happens, we should consume it.
            heartbeat_rx.try_recv().ok();

            self.internal_tx
                .try_send(Packet::Ping)
                .map_err(|_| Error::HeartbeatTimeout)?;
            tokio::time::timeout(timeout, heartbeat_rx.recv())
                .await
                .map_err(|_| Error::HeartbeatTimeout)?
                .ok_or(Error::HeartbeatTimeout)?;
            interval_tick.tick().await;
        }
    }

    #[cfg(feature = "v3")]
    async fn heartbeat_job_v3(&self, interval: Duration, timeout: Duration) -> Result<(), Error> {
        let mut heartbeat_rx = self
            .heartbeat_rx
            .try_lock()
            .expect("Pong rx should be locked only once");

        debug!("[sid={}] heartbeat receiver routine started", self.id);

        loop {
            tokio::time::timeout(interval + timeout, heartbeat_rx.recv())
                .await
                .map_err(|_| Error::HeartbeatTimeout)?
                .ok_or(Error::HeartbeatTimeout)?;

            debug!("[sid={}] ping received, sending pong", self.id);
            self.internal_tx
                .try_send(Packet::Pong)
                .map_err(|_| Error::HeartbeatTimeout)?;
        }
    }

    /// Returns true if the [`Socket`] has a websocket [`TransportType`]
    pub(crate) fn is_ws(&self) -> bool {
        self.transport.load(Ordering::Relaxed) == TransportType::Websocket as u8
    }
    /// returns true if the [`Socket`] has an HTTP [`TransportType`]
    pub(crate) fn is_http(&self) -> bool {
        self.transport.load(Ordering::Relaxed) == TransportType::Polling as u8
    }

    /// Sets the [`TransportType`] to WebSocket
    /// Used when the client upgrade the connection from HTTP to WebSocket
    pub(crate) fn upgrade_to_websocket(&self) {
        self.transport
            .store(TransportType::Websocket as u8, Ordering::Relaxed);
    }

    /// Emits a message to the client.
    ///
    /// If the transport is in websocket mode, the message is directly sent as a text frame.
    ///
    /// If the transport is in polling mode, the message is buffered and sent as a text frame to the next polling request.
    ///
    /// ⚠️ If the buffer is full or the socket is disconnected, an error will be returned with the original data
    pub fn emit(&self, msg: String) -> Result<(), TrySendError<String>> {
        self.send(Packet::Message(msg)).map_err(|e| match e {
            TrySendError::Full(p) => TrySendError::Full(p.into_message()),
            TrySendError::Closed(p) => TrySendError::Closed(p.into_message()),
        })
    }

    /// Immediately closes the socket and the underlying connection.
    /// The socket will be removed from the `Engine` and the [`Handler`](crate::handler::EngineIoHandler) will be notified.
    pub fn close(&self, reason: DisconnectReason) {
        (self.close_fn)(self.id, reason);
        self.send(Packet::Close).ok();
    }

    pub fn is_closed(&self) -> bool {
        self.internal_tx.is_closed()
    }

    pub async fn closed(&self) {
        self.internal_tx.closed().await
    }

    /// Emits a binary message to the client.
    ///
    /// If the transport is in websocket mode, the message is directly sent as a binary frame.
    ///
    /// If the transport is in polling mode, the message is buffered and sent as a text frame **encoded in base64** to the next polling request.
    ///
    /// ⚠️ If the buffer is full or the socket is disconnected, an error will be returned with the original data
    pub fn emit_binary(&self, data: Vec<u8>) -> Result<(), TrySendError<Vec<u8>>> {
        if self.protocol == ProtocolVersion::V3 {
            self.send(Packet::BinaryV3(data))
        } else {
            self.send(Packet::Binary(data))
        }
        .map_err(|e| match e {
            TrySendError::Full(p) => TrySendError::Full(p.into_binary()),
            TrySendError::Closed(p) => TrySendError::Closed(p.into_binary()),
        })
    }
}

impl<D: Default + Send + Sync + 'static> std::fmt::Debug for Socket<D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Socket")
            .field("sid", &self.id)
            .field("protocol", &self.protocol)
            .field("conn", &self.transport)
            .field("internal_rx", &self.internal_rx)
            .field("internal_tx", &self.internal_tx)
            .field("heartbeat_rx", &self.heartbeat_rx)
            .field("heartbeat_tx", &self.heartbeat_tx)
            .field("heartbeat_handle", &self.heartbeat_handle)
            .field("req_data", &self.req_data)
            .finish()
    }
}

#[cfg(feature = "test-utils")]
impl<D> Drop for Socket<D>
where
    D: Default + Send + Sync + 'static,
{
    fn drop(&mut self) {
        debug!("[sid={}] dropping socket", self.id);
    }
}

#[cfg(feature = "test-utils")]
impl<D> Socket<D>
where
    D: Default + Send + Sync + 'static,
{
    pub fn new_dummy(
        sid: Sid,
        close_fn: Box<dyn Fn(Sid, DisconnectReason) + Send + Sync>,
    ) -> Socket<D> {
        let (internal_tx, internal_rx) = mpsc::channel(200);
        let (heartbeat_tx, heartbeat_rx) = mpsc::channel(1);

        Self {
            id: sid,
            protocol: ProtocolVersion::V4,
            transport: AtomicU8::new(TransportType::Websocket as u8),

            internal_rx: Mutex::new(PeekableReceiver::new(internal_rx)),
            internal_tx,

            heartbeat_rx: Mutex::new(heartbeat_rx),
            heartbeat_tx,
            heartbeat_handle: Mutex::new(None),
            close_fn,

            data: D::default(),
            req_data: SocketReq {
                headers: http::HeaderMap::new(),
                uri: Uri::default(),
            }
            .into(),

            #[cfg(feature = "v3")]
            supports_binary: true,
        }
    }
}
