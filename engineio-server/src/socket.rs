use std::{ops::ControlFlow, time::Duration};

use tokio::{
    sync::{
        mpsc::{self, Receiver},
        Mutex, RwLock,
    },
    time::{self, Instant},
};
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
    // Only one receiver is allowed for each socket
    pub rx: Mutex<Receiver<Packet>>,
    conn: RwLock<ConnectionType>,
    tx: mpsc::Sender<Packet>, // Sender for sending packets to the socket
    last_pong: Mutex<Instant>,
}

impl Socket {
    pub(crate) fn new(sid: i64, conn: ConnectionType) -> Self {
        let (tx, rx) = mpsc::channel(100);
        Self {
            sid,
            last_pong: Mutex::new(time::Instant::now()),
            tx,
            rx: Mutex::new(rx),
            conn: conn.into(),
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
        tracing::debug!("Received packet from conn : {:?}", packet);
        match packet {
            Packet::Close => {
                let res = self.send(Packet::Noop).await;
                ControlFlow::Break(res)
            }
            Packet::Pong => ControlFlow::Continue(Ok(())),
            Packet::Message(msg) => {
                tracing::debug!("Received message: {}", msg);
                match handler.handle::<H>(msg, self).await {
                    Ok(_) => ControlFlow::Continue(Ok(())),
                    Err(e) => ControlFlow::Continue(Err(e)),
                }
            }
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
        // let msg: String = packet.try_into().map_err(Error::from)?;
        debug!("Sending packet for sid={}: {:?}", self.sid, packet);
        self.tx.send(packet).await?;
        Ok(())
    }

    pub(crate) async fn spawn_heartbeat(&self, interval: u64, timeout: u64) -> Result<(), Error> {
        let mut interval = tokio::time::interval(Duration::from_millis(interval - timeout));
        loop {
            self.send_heartbeat(timeout).await?;
            interval.tick().await;
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
    pub(crate) async fn received_pong(&self) {
        let mut last_pong = self.last_pong.lock().await;
        *last_pong = Instant::now();
    }

    async fn send_heartbeat(&self, timeout: u64) -> Result<(), Error> {
        let instant = Instant::now();
        self.send(Packet::Ping).await?;
        tokio::time::sleep(Duration::from_millis(timeout)).await;
        let pong = self.last_pong.lock().await;
        let valid = pong.elapsed().as_millis() > instant.elapsed().as_millis()
            && pong.elapsed().as_millis() < timeout.into();
        valid.then_some(()).ok_or(Error::HeartbeatTimeout)
    }

    pub async fn emit(&self, msg: String) -> Result<(), Error> {
        self.send(Packet::Message(msg)).await
    }

    pub async fn emit_binary(&self, data: Vec<u8>) -> Result<(), Error> {
        self.send(Packet::Binary(data)).await?;
        Ok(())
    }
}
