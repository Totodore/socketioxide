use std::{
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc,
    },
    time::Duration,
};

use http::{request::Parts, Uri};
use tokio::{
    sync::{mpsc, mpsc::Receiver, Mutex},
    task::JoinHandle,
};
use tracing::debug;

use crate::{
    config::EngineIoConfig, errors::Error, handler::EngineIoHandler, packet::Packet,
    utils::forward_map_chan, SendPacket,
};
use crate::{protocol::ProtocolVersion, sid_generator::Sid};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ConnectionType {
    Http = 0b000000001,
    WebSocket = 0b000000010,
}

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

/// A [`Socket`] represents a connection to the server.
/// It is agnostic to the [`TransportType`](crate::service::TransportType).
/// It handles :
/// * the packet communication between with the `Engine`
/// and the user defined [`Handler`](crate::handler::EngineIoHandler).
/// * the user defined [`Data`](crate::handler::EngineIoHandler::Data) bound to the socket.
/// * the heartbeat job that verify that the connection is still up by sending packets periodically.
pub struct Socket<H>
where
    H: EngineIoHandler + ?Sized,
{
    /// The socket id
    pub sid: Sid,

    /// The connection type represented as a bitfield
    /// It is represented as a bitfield to allow the use of an [`AtomicU8`] so it can be shared between threads
    /// without any mutex
    conn: AtomicU8,

    /// Channel to receive [`Packet`] from the connection
    ///
    /// It is used and managed by the [`EngineIo`](crate::engine) struct depending on the transport type
    ///
    /// It is locked if [`EngineIo`](crate::engine) is currently reading from it :
    /// * In case of polling transport it will be locked and released for each request
    /// * In case of websocket transport it will be always locked until the connection is closed
    pub(crate) internal_rx: Mutex<Receiver<Packet>>,

    /// Channel to send [Packet] to the internal connection
    internal_tx: mpsc::Sender<Packet>,
    pub tx: mpsc::Sender<SendPacket>,

    /// Internal channel to receive Pong [`Packets`](Packet) in the heartbeat job
    /// which is running in a separate task
    pong_rx: Mutex<mpsc::Receiver<()>>,
    /// Channel to send Ping [`Packets`](Packet) from the connexion to the heartbeat job
    /// which is running in a separate task
    pub(crate) pong_tx: mpsc::Sender<()>,
    /// Handle to the heartbeat job so that it can be aborted when the socket is closed
    heartbeat_handle: Mutex<Option<JoinHandle<()>>>,

    /// Function to call when the socket is closed
    close_fn: Box<dyn Fn(Sid) + Send + Sync>,
    /// User data bound to the socket
    pub data: H::Data,

    /// Http Request data used to create a socket
    pub req_data: Arc<SocketReq>,
}

impl<H> Socket<H>
where
    H: EngineIoHandler + ?Sized,
{
    pub(crate) fn new(
        sid: Sid,
        conn: ConnectionType,
        config: &EngineIoConfig,
        req_data: SocketReq,
        close_fn: Box<dyn Fn(Sid) + Send + Sync>,
    ) -> Self {
        let (internal_tx, internal_rx) = mpsc::channel(config.max_buffer_size);
        let (tx, rx) = mpsc::channel(config.max_buffer_size);
        let (pong_tx, pong_rx) = mpsc::channel(1);

        tokio::spawn(forward_map_chan(rx, internal_tx.clone(), SendPacket::into));

        Self {
            sid,
            conn: AtomicU8::new(conn as u8),

            internal_rx: Mutex::new(internal_rx),
            internal_tx,
            tx,

            pong_rx: Mutex::new(pong_rx),
            pong_tx,
            heartbeat_handle: Mutex::new(None),
            close_fn,

            data: H::Data::default(),
            req_data: req_data.into(),
        }
    }

    /// Abort the heartbeat job if it is running
    pub(crate) fn abort_heartbeat(&self) {
        if let Ok(Some(handle)) = self.heartbeat_handle.try_lock().map(|mut h| h.take()) {
            handle.abort();
        }
    }

    /// Sends a packet to the connection.
    pub(crate) fn send(&self, packet: Packet) -> Result<(), Error> {
        debug!("[sid={}] sending packet: {:?}", self.sid, packet);
        self.internal_tx.try_send(packet)?;
        Ok(())
    }

    /// Spawn the heartbeat job
    ///
    /// Keep a handle to the job so that it can be aborted when the socket is closed
    pub(crate) fn spawn_heartbeat(
        self: Arc<Self>,
        protocol: ProtocolVersion,
        interval: Duration,
        timeout: Duration,
    ) {
        let socket = self.clone();

        let handle = tokio::spawn(async move {
            if let Err(e) = socket.heartbeat_job(protocol, interval, timeout).await {
                socket.close();
                debug!("[sid={}] heartbeat error: {:?}", socket.sid, e);
            }
        });
        self.heartbeat_handle
            .try_lock()
            .expect("heartbeat handle mutex should not be locked twice")
            .replace(handle);
    }

    /// Heartbeat is sent every `interval` milliseconds and the client is expected to respond within `timeout` milliseconds.
    ///
    /// If the client does not respond within the timeout, the connection is closed.
    async fn heartbeat_job(
        &self,
        protocol: ProtocolVersion,
        interval: Duration,
        timeout: Duration,
    ) -> Result<(), Error> {
        let mut pong_rx = self
            .pong_rx
            .try_lock()
            .expect("Pong rx should be locked only once");

        match protocol {
            ProtocolVersion::V3 => {
                debug!("[sid={}] heartbeat receiver routine started", self.sid);
                loop {
                    if pong_rx.recv().await.is_some() {
                        debug!("[sid={}] ping received, sending pong", self.sid);
                        self.internal_tx
                            .try_send(Packet::Pong)
                            .map_err(|_| Error::HeartbeatTimeout)?;
                    }
                }
            }
            ProtocolVersion::V4 => {
                let instant = tokio::time::Instant::now();
                let mut interval_tick = tokio::time::interval(interval);
                interval_tick.tick().await;
                // Sleep for an interval minus the time it took to get here
                tokio::time::sleep(interval.saturating_sub(Duration::from_millis(
                    15 + instant.elapsed().as_millis() as u64,
                )))
                .await;
                debug!("[sid={}] heartbeat sender routine started", self.sid);
                loop {
                    // Some clients send the pong packet in first. If that happens, we should consume it.
                    pong_rx.try_recv().ok();

                    self.internal_tx
                        .try_send(Packet::Ping)
                        .map_err(|_| Error::HeartbeatTimeout)?;
                    tokio::time::timeout(timeout, pong_rx.recv())
                        .await
                        .map_err(|_| Error::HeartbeatTimeout)?
                        .ok_or(Error::HeartbeatTimeout)?;
                    interval_tick.tick().await;
                }
            }
        }
    }

    /// Returns true if the [`Socket`] has a websocket [`ConnectionType`]
    pub(crate) fn is_ws(&self) -> bool {
        self.conn.load(Ordering::Relaxed) == ConnectionType::WebSocket as u8
    }
    /// returns true if the [`Socket`] has an HTTP [`ConnectionType`]
    pub(crate) fn is_http(&self) -> bool {
        self.conn.load(Ordering::Relaxed) == ConnectionType::Http as u8
    }

    /// Sets the [`ConnectionType`] to WebSocket
    /// Used when the client upgrade the connection from HTTP to WebSocket
    pub(crate) fn upgrade_to_websocket(&self) {
        self.conn
            .store(ConnectionType::WebSocket as u8, Ordering::Relaxed);
    }

    /// Emits a message to the client.
    ///
    /// If the transport is in websocket mode, the message is directly sent as a text frame.
    ///
    /// If the transport is in polling mode, the message is buffered and sent as a text frame to the next polling request.
    ///
    /// ⚠️ If the buffer is full or the socket is disconnected, an error will be returned
    pub fn emit(&self, msg: String) -> Result<(), Error> {
        self.send(Packet::Message(msg))
    }

    /// Immediately closes the socket and the underlying connection.
    /// The socket will be removed from the `Engine` and the [`Handler`](crate::handler::EngineIoHandler) will be notified.
    pub fn close(&self) {
        (self.close_fn)(self.sid);
        self.send(Packet::Close).ok();
    }

    /// Emits a binary message to the client.
    ///
    /// If the transport is in websocket mode, the message is directly sent as a binary frame.
    ///
    /// If the transport is in polling mode, the message is buffered and sent as a text frame **encoded in base64** to the next polling request.
    ///
    /// ⚠️ If the buffer is full or the socket is disconnected, an error will be returned
    pub fn emit_binary(&self, data: Vec<u8>) -> Result<(), Error> {
        self.send(Packet::Binary(data))?;
        Ok(())
    }
}

#[cfg(test)]
impl<H: EngineIoHandler> Socket<H> {
    pub fn new_dummy(sid: Sid, close_fn: Box<dyn Fn(Sid) + Send + Sync>) -> Socket<H> {
        let (internal_tx, internal_rx) = mpsc::channel(200);
        let (tx, rx) = mpsc::channel(200);
        let (pong_tx, pong_rx) = mpsc::channel(1);

        tokio::spawn(forward_map_chan(rx, internal_tx.clone(), SendPacket::into));

        Self {
            sid,
            conn: AtomicU8::new(ConnectionType::WebSocket as u8),

            internal_rx: Mutex::new(internal_rx),
            internal_tx,
            tx,

            pong_rx: Mutex::new(pong_rx),
            pong_tx,
            heartbeat_handle: Mutex::new(None),
            close_fn,

            data: H::Data::default(),
            req_data: SocketReq {
                headers: http::HeaderMap::new(),
                uri: Uri::default(),
            }
            .into(),
        }
    }
}
