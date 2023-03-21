use std::{ops::ControlFlow, time::Duration};

use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::debug;

use crate::{errors::Error, layer::EngineIoHandler, packet::Packet};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ConnectionType {
    Http,
    WebSocket,
}
#[derive(Debug)]
pub struct Socket {
    pub sid: i64,
    conn: RwLock<ConnectionType>,

    // Channel to send packets to the connection
    pub rx: Mutex<mpsc::Receiver<Packet>>,
    tx: mpsc::Sender<Packet>,

    // Channel to receive pong packets from the connection
    pong_rx: Mutex<mpsc::Receiver<()>>,
    pong_tx: mpsc::Sender<()>,
}

impl Socket {
    pub(crate) fn new(sid: i64, conn: ConnectionType) -> Self {
        let (tx, rx) = mpsc::channel(100);
        let (pong_tx, pong_rx) = mpsc::channel(1);
        Self {
            sid,
            tx,
            rx: Mutex::new(rx),
            conn: conn.into(),
            pong_rx: Mutex::new(pong_rx),
            pong_tx,
        }
    }

    pub(crate) async fn handle_packet<H>(
        &self,
        packet: Packet,
        handler: &H,
    ) -> ControlFlow<Result<(), Error>, Result<(), Error>>
    where
        H: EngineIoHandler,
    {
        debug!("[sid={}] received packet: {:?}", self.sid, packet);
        match packet {
            Packet::Close => {
                let res = self.send(Packet::Noop).await;
                ControlFlow::Break(res)
            }
            Packet::Pong => {
                self.pong_tx.try_send(()).ok();
                ControlFlow::Continue(Ok(()))
            }
            Packet::Message(msg) => match handler.handle::<H>(msg, self).await {
                Ok(_) => ControlFlow::Continue(Ok(())),
                Err(e) => ControlFlow::Continue(Err(e)),
            },
            _ => ControlFlow::Continue(Err(Error::BadPacket)),
        }
    }

    pub(crate) async fn handle_binary<H>(&self, data: Vec<u8>, handler: &H) -> Result<(), Error>
    where
        H: EngineIoHandler,
    {
        handler.handle_binary::<H>(data, self).await
    }

    pub async fn close(&self) -> Result<(), Error> {
        self.send(Packet::Close).await
    }

    pub(crate) async fn send(&self, packet: Packet) -> Result<(), Error> {
        debug!("[sid={}] sending packet: {:?}", self.sid, packet);
        self.tx.send(packet).await?;
        Ok(())
    }

    /// Heartbeat is sent every `interval` milliseconds and the client is expected to respond within `timeout` milliseconds.
    ///
    /// If the client does not respond within the timeout, the connection is closed.
    pub(crate) async fn spawn_heartbeat(
        &self,
        interval: Duration,
        timeout: Duration,
    ) -> Result<(), Error> {
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
        loop {
            self.tx
                .try_send(Packet::Ping)
                .map_err(|_| Error::HeartbeatTimeout)?;
            tokio::time::timeout(timeout, pong_rx.recv())
                .await
                .map_err(|_| Error::HeartbeatTimeout)?
                .ok_or(Error::HeartbeatTimeout)?;
            let instant = tokio::time::Instant::now();
            interval_tick.tick().await;
            debug!("tick {:?}", instant.elapsed());
        }
    }
    pub(crate) async fn is_ws(&self) -> bool {
        self.conn.read().await.eq(&ConnectionType::WebSocket)
    }
    pub(crate) async fn is_http(&self) -> bool {
        self.conn.read().await.eq(&ConnectionType::Http)
    }

    /// Sets the connection type to WebSocket
    pub(crate) async fn upgrade_to_websocket(&self) {
        let mut conn = self.conn.write().await;
        *conn = ConnectionType::WebSocket;
    }

    pub async fn emit(&self, msg: String) -> Result<(), Error> {
        self.send(Packet::Message(msg)).await
    }

    pub async fn emit_binary(&self, data: Vec<u8>) -> Result<(), Error> {
        self.send(Packet::Binary(data)).await?;
        Ok(())
    }
}
