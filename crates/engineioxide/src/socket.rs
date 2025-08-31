//! ## A [`Socket`] represents a client connection to the server
//!
//! It can be used to :
//! * Emit binary or string data
//! * Get a reference to the request made to connect to the socket.io server
//! * Close the connection
//!
//! #### Example :
//! ```rust
//! # use bytes::Bytes;
//! # use engineioxide::service::EngineIoService;
//! # use engineioxide::handler::EngineIoHandler;
//! # use engineioxide::{Socket, DisconnectReason, Str};
//! # use std::sync::{Mutex, Arc};
//! # use std::sync::atomic::{AtomicUsize, Ordering};
//! // Global state
//! #[derive(Debug, Default)]
//! struct MyHandler {
//!     user_cnt: AtomicUsize,
//! }
//!
//! // Socket state
//! #[derive(Debug, Default)]
//! struct SocketState {
//!     id: Mutex<String>,
//! }
//!
//! impl EngineIoHandler for MyHandler {
//!     type Data = SocketState;
//!
//!     fn on_connect(self: Arc<Self>, socket: Arc<Socket<SocketState>>) {
//!         // Get the request made to initialize the connection
//!         // and check that the authorization header is correct
//!         let connected = socket.req_parts.headers.get("Authorization")
//!             .map(|a| a == "mysuperpassword!").unwrap_or_default();
//!         // Close the socket if the authentication is invalid
//!         if !connected {
//!             socket.close(DisconnectReason::TransportError);
//!             return;
//!         }
//!
//!         let cnt = self.user_cnt.fetch_add(1, Ordering::Relaxed) + 1;
//!         // Emit string data to the client
//!         socket.emit(cnt.to_string()).ok();
//!     }
//!     fn on_disconnect(&self, socket: Arc<Socket<SocketState>>, reason: DisconnectReason) {
//!         let cnt = self.user_cnt.fetch_sub(1, Ordering::Relaxed) - 1;
//!     }
//!     fn on_message(self: &Arc<Self>, msg: Str, socket: Arc<Socket<SocketState>>) {
//!         *socket.data.id.lock().unwrap() = msg.into(); // bind a provided user id to a socket
//!     }
//!     fn on_binary(self: &Arc<Self>, data: Bytes, socket: Arc<Socket<SocketState>>) { }
//! }
//!
//! let svc = EngineIoService::new(Arc::new(MyHandler::default()));
//! ```
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    time::Duration,
};

use crate::{
    config::EngineIoConfig,
    errors::Error,
    packet::Packet,
    peekable::PeekableReceiver,
    service::{ProtocolVersion, TransportType},
};
use bytes::Bytes;
use engineioxide_core::Str;
use http::request::Parts;
use smallvec::{SmallVec, smallvec};
use tokio::{
    sync::{
        Mutex,
        mpsc::{self},
        mpsc::{Receiver, error::TrySendError},
    },
    task::JoinHandle,
};

pub use engineioxide_core::Sid;

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
            WsTransport(_) | Io(_) => Some(DisconnectReason::TransportError),
            BadPacket(_) | Base64(_) | StrUtf8(_) | PayloadTooLarge | InvalidPacketLength
            | InvalidPacketType(_) => Some(DisconnectReason::PacketParsingError),
            HeartbeatTimeout => Some(DisconnectReason::HeartbeatTimeout),
            _ => None,
        }
    }
}

/// A permit to emit a message to the client.
/// A permit holds a place in the internal channel to send one packet to the client.
pub struct Permit<'a> {
    inner: mpsc::Permit<'a, PacketBuf>,
}
impl Permit<'_> {
    /// Consume the permit and emit a message to the client.
    #[inline]
    pub fn emit(self, msg: Str) {
        self.inner.send(smallvec![Packet::Message(msg)]);
    }
    /// Consume the permit and emit a binary message to the client.
    #[inline]
    pub fn emit_binary(self, data: Bytes) {
        self.inner.send(smallvec![Packet::Binary(data)]);
    }

    /// Consume the permit and emit a message with multiple binary data to the client.
    ///
    /// It can be used to ensure atomicity when sending a string packet with adjacent binary packets.
    pub fn emit_many(self, msg: Str, data: VecDeque<Bytes>) {
        let mut packets = SmallVec::with_capacity(data.len() + 1);
        packets.push(Packet::Message(msg));
        for d in data {
            packets.push(Packet::Binary(d));
        }
        self.inner.send(packets);
    }

    /// Consume the permit and emit a message with multiple binary data to the client.
    ///
    /// It can be used to ensure atomicity when sending a string packet with adjacent binary packets.
    pub fn emit_many_binary(self, bin: Bytes, data: Vec<Bytes>) {
        let mut packets = SmallVec::with_capacity(data.len() + 1);
        packets.push(Packet::Binary(bin));
        for d in data {
            packets.push(Packet::Binary(d));
        }
        self.inner.send(packets);
    }
}

/// Buffered packets to send to the client.
/// It is used to ensure atomicity when sending multiple packets to the client.
///
/// The [`PacketBuf`] stack size will impact the dynamically allocated buffer
/// of the internal mpsc channel.
pub(crate) type PacketBuf = SmallVec<[Packet; 2]>;

/// A [`Socket`] represents a client connection to the server.
/// It is agnostic to the [`TransportType`].
///
/// It handles :
/// * the packet communication between with the `Engine`
///   and the user defined [`Handler`](crate::handler::EngineIoHandler).
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

    /// Whether the socket is currently upgrading to a new transport.
    upgrading: AtomicBool,

    /// Channel to send [`PacketBuf`] to the connection
    ///
    /// It is used and managed by the [`EngineIo`](crate::engine) struct depending on the transport type
    ///
    /// It is locked if [`EngineIo`](crate::engine) is currently reading from it :
    /// * In case of polling transport it will be locked and released for each request
    /// * In case of websocket transport it will always be locked until the connection is closed
    ///
    /// It will be closed when a [`Close`](Packet::Close) packet is received:
    /// * From the [encoder](crate::service::encoder) if the transport is polling
    /// * From the fn [`on_ws_req_init`](crate::engine::EngineIo) if the transport is websocket
    /// * Automatically via the [`close_session fn`](crate::engine::EngineIo::close_session) as a fallback.
    ///   Because with polling transport, if the client is not currently polling then the encoder will never be able to close the channel
    ///
    /// The channel is made of a [`SmallVec`] of [`Packet`]s so that adjacent packets can be sent atomically.
    pub(crate) internal_rx: Mutex<PeekableReceiver<PacketBuf>>,

    /// Channel to send [PacketBuf] to the internal connection
    internal_tx: mpsc::Sender<PacketBuf>,

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
    pub req_parts: Parts,

    /// If the client supports binary packets (via polling XHR2)
    #[cfg(feature = "v3")]
    pub(crate) supports_binary: bool,
}

impl<D> Socket<D>
where
    D: Default + Send + Sync + 'static,
{
    pub(crate) fn new(
        protocol: ProtocolVersion,
        transport: TransportType,
        config: &EngineIoConfig,
        req_parts: Parts,
        close_fn: Box<dyn Fn(Sid, DisconnectReason) + Send + Sync>,
        #[cfg(feature = "v3")] supports_binary: bool,
    ) -> Self {
        let (internal_tx, internal_rx) = mpsc::channel(config.max_buffer_size);
        let (heartbeat_tx, heartbeat_rx) = mpsc::channel(1);

        Self {
            id: Sid::new(),
            protocol,
            transport: AtomicU8::new(transport as u8),
            upgrading: AtomicBool::new(false),

            internal_rx: Mutex::new(PeekableReceiver::new(internal_rx)),
            internal_tx,

            heartbeat_rx: Mutex::new(heartbeat_rx),
            heartbeat_tx,
            heartbeat_handle: Mutex::new(None),
            close_fn,

            data: D::default(),
            req_parts,

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
        #[cfg(feature = "tracing")]
        tracing::debug!("[sid={}] sending packet: {:?}", self.id, packet);
        self.internal_tx
            .try_send(smallvec![packet])
            .map_err(|p| match p {
                TrySendError::Full(mut p) => TrySendError::Full(p.pop().unwrap()),
                TrySendError::Closed(mut p) => TrySendError::Closed(p.pop().unwrap()),
            })?;
        Ok(())
    }
    pub(crate) fn is_upgrading(&self) -> bool {
        self.upgrading.load(Ordering::Relaxed)
    }
    pub(crate) fn start_upgrade(&self) {
        self.upgrading.store(true, Ordering::Relaxed);
    }

    /// Spawn the heartbeat job
    ///
    /// Keep a handle to the job so that it can be aborted when the socket is closed
    pub(crate) fn spawn_heartbeat(self: Arc<Self>, interval: Duration, timeout: Duration) {
        let socket = self.clone();

        let handle = tokio::spawn(async move {
            if let Err(_e) = socket.heartbeat_job(interval, timeout).await {
                socket.close(DisconnectReason::HeartbeatTimeout);
                #[cfg(feature = "tracing")]
                tracing::debug!("[sid={}] heartbeat error: {:?}", socket.id, _e);
            }
        });
        self.heartbeat_handle
            .try_lock()
            .expect("heartbeat handle mutex should not be locked twice")
            .replace(handle);
    }

    /// Heartbeat is sent every `interval` milliseconds by the client and the server `is` expected to respond within `timeout` milliseconds.
    ///
    /// If the client or server does not respond within the timeout, the connection is closed.
    #[cfg(feature = "v3")]
    async fn heartbeat_job(&self, interval: Duration, timeout: Duration) -> Result<(), Error> {
        match self.protocol {
            ProtocolVersion::V3 => self.heartbeat_job_v3(interval, timeout).await,
            ProtocolVersion::V4 => self.heartbeat_job_v4(interval, timeout).await,
        }
    }

    /// Heartbeat is sent every `interval` milliseconds and the client is expected to respond within `timeout` milliseconds.
    ///
    /// If the client does not respond within the timeout, the connection is closed.
    #[cfg(not(feature = "v3"))]
    async fn heartbeat_job(&self, interval: Duration, timeout: Duration) -> Result<(), Error> {
        self.heartbeat_job_v4(interval, timeout).await
    }

    /// Heartbeat is sent every `interval` milliseconds and the client is expected to respond within `timeout` milliseconds.
    ///
    /// If the client does not respond within the timeout, the connection is closed.
    async fn heartbeat_job_v4(&self, interval: Duration, timeout: Duration) -> Result<(), Error> {
        let mut heartbeat_rx = self
            .heartbeat_rx
            .try_lock()
            .expect("Pong rx should be locked only once");

        #[cfg(feature = "tracing")]
        tracing::debug!(sid = ?self.id, "heartbeat sender routine started");

        let mut interval_tick = tokio::time::interval(interval);
        interval_tick.tick().await;
        // Some clients send the pong packet in first. If that happens, we should consume it.
        heartbeat_rx.try_recv().ok();
        loop {
            // If we are currently upgrading we should pause the heartbeat process
            if self.is_upgrading() {
                #[cfg(feature = "tracing")]
                tracing::debug!(sid = ?self.id, "heartbeat paused due to upgrade, skipping");

                interval_tick.tick().await;
                continue;
            }

            #[cfg(feature = "tracing")]
            tracing::trace!(sid = ?self.id, "emitting ping");

            self.internal_tx
                .try_send(smallvec![Packet::Ping])
                .map_err(|_| Error::HeartbeatTimeout)?;

            #[cfg(feature = "tracing")]
            tracing::trace!(sid = ?self.id, "waiting for pong");

            tokio::time::timeout(timeout, heartbeat_rx.recv())
                .await
                .map_err(|_| Error::HeartbeatTimeout)?
                .ok_or(Error::HeartbeatTimeout)?;

            #[cfg(feature = "tracing")]
            tracing::trace!(sid = ?self.id, "pong received");

            interval_tick.tick().await;
        }
    }

    #[cfg(feature = "v3")]
    async fn heartbeat_job_v3(&self, interval: Duration, timeout: Duration) -> Result<(), Error> {
        let mut heartbeat_rx = self
            .heartbeat_rx
            .try_lock()
            .expect("Pong rx should be locked only once");

        #[cfg(feature = "tracing")]
        tracing::debug!(sid = ?self.id, "heartbeat receiver routine started");

        loop {
            match tokio::time::timeout(interval + timeout, heartbeat_rx.recv()).await {
                Ok(Some(_)) => (),
                Err(_) if self.is_upgrading() => {
                    // If we are currently upgrading we
                    // should pause the heartbeat process
                    // hence skipping timing out
                    tracing::debug!(sid = ?self.id, "heartbeat paused due to upgrade, skipping timeout");
                    continue;
                }
                _ => return Err(Error::HeartbeatTimeout),
            }

            #[cfg(feature = "tracing")]
            tracing::trace!(sid = ?self.id, "ping received, sending pong");
            self.internal_tx
                .try_send(smallvec![Packet::Pong])
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
        self.upgrading.store(false, Ordering::Relaxed);
        self.transport
            .store(TransportType::Websocket as u8, Ordering::Relaxed);
    }

    /// Returns the current [`TransportType`] of the [`Socket`]
    pub fn transport_type(&self) -> TransportType {
        TransportType::from(self.transport.load(Ordering::Relaxed))
    }

    /// Reserve `n` permits to emit multiple messages and ensure that there is enough
    /// space in the internal chan.
    ///
    /// If the internal chan is full, the function will return a [`TrySendError::Full`] error.
    /// If the socket is closed, the function will return a [`TrySendError::Closed`] error.
    #[inline]
    pub fn reserve(&self) -> Result<Permit<'_>, TrySendError<()>> {
        let permit = self.internal_tx.try_reserve()?;
        Ok(Permit { inner: permit })
    }

    /// Emits a message to the client.
    ///
    /// If the transport is in websocket mode, the message is directly sent as a text frame.
    ///
    /// If the transport is in polling mode, the message is buffered and sent as a text frame to the next polling request.
    ///
    /// ⚠️ If the buffer is full or the socket is disconnected, an error will be returned with the original data
    pub fn emit(&self, msg: impl Into<Str>) -> Result<(), TrySendError<Str>> {
        self.send(Packet::Message(msg.into())).map_err(|e| match e {
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

    /// Returns true if the socket is closed
    /// It means that no more packets can be sent to the client
    pub fn is_closed(&self) -> bool {
        self.internal_tx.is_closed()
    }

    /// Wait for the socket to be fully closed
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
    pub fn emit_binary<B: Into<Bytes>>(&self, data: B) -> Result<(), TrySendError<Bytes>> {
        if self.protocol == ProtocolVersion::V3 {
            self.send(Packet::BinaryV3(data.into()))
        } else {
            self.send(Packet::Binary(data.into()))
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
            .field("req_data", &self.req_parts)
            .finish()
    }
}

#[doc(hidden)]
#[cfg(feature = "__test_harness")]
impl<D> Drop for Socket<D>
where
    D: Default + Send + Sync + 'static,
{
    fn drop(&mut self) {
        #[cfg(feature = "tracing")]
        tracing::debug!("[sid={}] dropping socket", self.id);
    }
}

#[doc(hidden)]
#[cfg(feature = "__test_harness")]
impl<D> Socket<D>
where
    D: Default + Send + Sync + 'static,
{
    /// Create a dummy socket for testing purpose
    pub fn new_dummy(
        sid: Sid,
        close_fn: Box<dyn Fn(Sid, DisconnectReason) + Send + Sync>,
    ) -> Arc<Socket<D>> {
        let (s, mut rx) = Socket::new_dummy_piped(sid, close_fn, 1024);
        tokio::spawn(async move {
            while let Some(_el) = rx.recv().await {
                #[cfg(feature = "tracing")]
                tracing::debug!(?sid, ?_el, "emitting eio msg");
            }
        });
        s
    }

    /// Create a dummy socket for testing purpose with a
    /// receiver to get the packets sent to the client
    pub fn new_dummy_piped(
        sid: Sid,
        close_fn: Box<dyn Fn(Sid, DisconnectReason) + Send + Sync>,
        buffer_size: usize,
    ) -> (Arc<Socket<D>>, tokio::sync::mpsc::Receiver<Packet>) {
        let (internal_tx, internal_rx) = mpsc::channel(buffer_size);
        let (heartbeat_tx, heartbeat_rx) = mpsc::channel(1);

        let sock = Self {
            id: sid,
            protocol: ProtocolVersion::V4,
            transport: AtomicU8::new(TransportType::Websocket as u8),
            upgrading: AtomicBool::new(false),

            internal_rx: Mutex::new(PeekableReceiver::new(internal_rx)),
            internal_tx,

            heartbeat_rx: Mutex::new(heartbeat_rx),
            heartbeat_tx,
            heartbeat_handle: Mutex::new(None),
            close_fn,

            data: D::default(),
            req_parts: http::Request::<()>::default().into_parts().0,

            #[cfg(feature = "v3")]
            supports_binary: true,
        };
        let sock = Arc::new(sock);

        let (tx, rx) = mpsc::channel(buffer_size);
        let sock_clone = sock.clone();
        tokio::spawn(async move {
            let mut internal_rx = sock_clone.internal_rx.try_lock().unwrap();
            while let Some(packets) = internal_rx.recv().await {
                for packet in packets {
                    tx.send(packet).await.unwrap();
                }
            }
        });

        (sock, rx)
    }
}
