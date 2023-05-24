use std::{
    ops::ControlFlow,
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc, RwLock,
    },
    time::Duration,
};

use http::{request::Parts, Uri};
use tokio::{
    sync::{mpsc, Mutex},
    task::JoinHandle,
};
use tracing::debug;

use crate::{
    errors::Error,
    layer::{EngineIoConfig, EngineIoHandler},
    packet::Packet,
};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ConnectionType {
    Http = 0b000000001,
    WebSocket = 0b000000010,
}

pub struct SocketReq {
    pub uri: Uri,
    pub headers: http::HeaderMap,
}

impl From<Parts> for SocketReq {
    fn from(parts: Parts) -> Self {
        Self {
            uri: parts.uri,
            headers: parts.headers,
        }
    }
}

impl SocketReq {
    pub fn new(uri: Uri, headers: http::HeaderMap) -> Self {
        Self { uri, headers }
    }
    pub fn update(&mut self, req: SocketReq) {
        self.uri = req.uri;
        self.headers = req.headers;
    }
}

pub struct Socket<H>
where
    H: EngineIoHandler + ?Sized,
{
    pub sid: i64,

    // The connection type casted to u8
    conn: AtomicU8,

    // Channel to send packets to the connection
    pub(crate) rx: Mutex<mpsc::Receiver<Packet>>,
    tx: mpsc::Sender<Packet>,
    handler: Arc<H>,

    // Channel to receive pong packets from the connection
    pong_rx: Mutex<mpsc::Receiver<()>>,
    pong_tx: mpsc::Sender<()>,
    heartbeat_handle: Mutex<Option<JoinHandle<()>>>,

    // User data bound to the socket
    pub data: H::Data,
    pub req_data: RwLock<SocketReq>,
}

impl<H> Drop for Socket<H>
where
    H: EngineIoHandler + ?Sized,
{
    fn drop(&mut self) {
        self.handler.clone().on_disconnect(&self);
    }
}

impl<H> Socket<H>
where
    H: EngineIoHandler + ?Sized,
{
    pub(crate) fn new(
        sid: i64,
        conn: ConnectionType,
        config: &EngineIoConfig,
        handler: Arc<H>,
        req_data: SocketReq,
    ) -> Self {
        let (tx, rx) = mpsc::channel(config.max_buffer_size);
        let (pong_tx, pong_rx) = mpsc::channel(1);
        let socket = Self {
            sid,
            conn: AtomicU8::new(conn as u8),

            rx: Mutex::new(rx),
            tx,
            handler,

            pong_rx: Mutex::new(pong_rx),
            pong_tx,
            heartbeat_handle: Mutex::new(None),

            data: H::Data::default(),
            req_data: req_data.into(),
        };
        socket.handler.clone().on_connect(&socket);
        socket
    }

    /// Handle a packet received from the connection.
    /// Returns a `ControlFlow` to indicate whether the socket should be closed or not.
    pub(crate) async fn handle_packet(
        &self,
        packet: Packet,
    ) -> ControlFlow<Result<(), Error>, Result<(), Error>> {
        debug!("[sid={}] received packet: {:?}", self.sid, packet);
        match packet {
            Packet::Close => ControlFlow::Break(self.send(Packet::Noop)),
            Packet::Pong => {
                self.pong_tx.try_send(()).ok();
                ControlFlow::Continue(Ok(()))
            }
            Packet::Binary(data) => {
                self.handler.clone().on_binary(data, self).await;
                ControlFlow::Continue(Ok(()))
            }
            Packet::Message(msg) => {
                self.handler.clone().on_message(msg, self).await;
                ControlFlow::Continue(Ok(()))
            }
            p => ControlFlow::Continue(Err(Error::BadPacket(p))),
        }
    }

    /// Closes the socket
    /// Abort the heartbeat job if it is running
    pub(crate) fn close(&self) {
        if let Some(handle) = self.heartbeat_handle.try_lock().unwrap().take() {
            handle.abort();
        }
    }

    /// Sends a packet to the connection.
    pub(crate) fn send(&self, packet: Packet) -> Result<(), Error> {
        debug!("[sid={}] sending packet: {:?}", self.sid, packet);
        self.tx.try_send(packet)?;
        Ok(())
    }

    /// Spawn the heartbeat job
    /// Keep a handle to the job so that it can be aborted when the socket is closed
    pub(crate) fn spawn_heartbeat(self: Arc<Self>, interval: Duration, timeout: Duration) {
        let socket = self.clone();

        let handle = tokio::spawn(async move {
            if let Err(e) = socket.heartbeat_job(interval, timeout).await {
                socket.send(Packet::Abort).ok();
                debug!("[sid={}] heartbeat error: {:?}", socket.sid, e);
            }
        });
        self.heartbeat_handle.try_lock().unwrap().replace(handle);
    }

    /// Heartbeat is sent every `interval` milliseconds and the client is expected to respond within `timeout` milliseconds.
    ///
    /// If the client does not respond within the timeout, the connection is closed.
    async fn heartbeat_job(&self, interval: Duration, timeout: Duration) -> Result<(), Error> {
        let mut pong_rx = self
            .pong_rx
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
        debug!("[sid={}] heartbeat routine started", self.sid);
        loop {
            // Some clients send the pong packet in first. If that happens, we should consume it.
            pong_rx.try_recv().ok();

            self.tx
                .try_send(Packet::Ping)
                .map_err(|_| Error::HeartbeatTimeout)?;
            tokio::time::timeout(timeout, pong_rx.recv())
                .await
                .map_err(|_| Error::HeartbeatTimeout)?
                .ok_or(Error::HeartbeatTimeout)?;
            interval_tick.tick().await;
        }
    }
    pub(crate) fn is_ws(&self) -> bool {
        self.conn.load(Ordering::Relaxed) == ConnectionType::WebSocket as u8
    }
    pub(crate) fn is_http(&self) -> bool {
        self.conn.load(Ordering::Relaxed) == ConnectionType::Http as u8
    }

    /// Sets the connection type to WebSocket when the client upgrades the connection.
    pub(crate) fn upgrade_to_websocket(&self, req_data: SocketReq) {
        self.conn
            .store(ConnectionType::WebSocket as u8, Ordering::Relaxed);
        self.req_data.write().unwrap().update(req_data);
    }

    /// Emits a message to the client.
    ///
    /// If the transport is in websocket mode, the message is directly sent as a text frame.
    ///
    /// If the transport is in polling mode, the message is buffered and sent as a text frame to the next polling request.
    /// ⚠️ If the buffer is full or the socket is disconnected, an error will be returned
    pub fn emit(&self, msg: String) -> Result<(), Error> {
        self.send(Packet::Message(msg))
    }

    pub fn emit_close(&self) {
        self.send(Packet::Abort).ok();
    }

    /// Emits a binary message to the client.
    ///
    /// If the transport is in websocket mode, the message is directly sent as a binary frame.
    ///
    /// If the transport is in polling mode, the message is buffered and sent as a text frame **encoded in base64** to the next polling request.
    /// ⚠️ If the buffer is full or the socket is disconnected, an error will be returned
    pub fn emit_binary(&self, data: Vec<u8>) -> Result<(), Error> {
        self.send(Packet::Binary(data))?;
        Ok(())
    }
}
