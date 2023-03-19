use std::{ops::ControlFlow, time::Duration};

use tokio::{
    sync::{mpsc::{self, Receiver}, Mutex},
    time::{self, Instant},
};
use tracing::debug;

use crate::{errors::Error, layer::EngineIoHandler, packet::Packet};

#[derive(Debug)]
pub struct Socket {
    sid: i64,
    tx: mpsc::Sender<Packet>, // Sender for sending packets to the socket
    // Only one receiver is allowed for each socket
    pub rx: Mutex<Receiver<Packet>>,
    last_pong: Instant,
}

impl Socket {
    pub(crate) fn new(sid: i64) -> Self {
        let (tx, rx) = mpsc::channel(100);
        Self {
            sid,
            last_pong: time::Instant::now(),
            tx,
            rx: Mutex::new(rx),
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
            Packet::Open(_) => ControlFlow::Continue(Err(Error::BadPacket(
                "Unexpected Open packet, it should be only used in upgrade process",
            ))),
            Packet::Close => ControlFlow::Break(Ok(())),
            Packet::Ping => ControlFlow::Continue(Err(Error::BadPacket("Unexpected Ping packet"))),
            Packet::Pong => {
                ControlFlow::Continue(Ok(()))
            }
            Packet::Message(msg) => {
                tracing::debug!("Received message: {}", msg);
                match handler.handle::<H>(msg, self).await {
                    Ok(_) => ControlFlow::Continue(Ok(())),
                    Err(e) => ControlFlow::Continue(Err(e)),
                }
            }
            Packet::Upgrade => ControlFlow::Continue(Err(Error::BadPacket(
                "Unexpected Upgrade packet, upgrade from ws connection not supported",
            ))),
            Packet::Noop => ControlFlow::Continue(Err(Error::BadPacket(
                "Unexpected Noop packet, it should be only used in upgrade process",
            ))),
            Packet::Binary(_) => ControlFlow::Break(Err(Error::BadPacket(
                "Unexpected Binary packet, it should be only used internally",
            ))),
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

    pub(crate) async fn spawn_heartbeat(
        &mut self,
        interval: u64,
        timeout: u64,
    ) -> Result<(), Error> {
        // let timeout = self.ping_timeout;
        tokio::time::sleep(Duration::from_millis(interval * 2)).await;
        let mut interval = tokio::time::interval(Duration::from_millis(interval - timeout));
        loop {
            if !self.send_heartbeat(timeout).await? {
                //TODO: handle heartbeat failure
                break;
            }
            interval.tick().await;
        }
        Ok(())
    }

    async fn send_heartbeat(&mut self, timeout: u64) -> Result<bool, Error> {
        let instant = Instant::now();
        self.send(Packet::Ping).await?;
        tokio::time::sleep(Duration::from_millis(timeout)).await;
        Ok(
            self.last_pong.elapsed().as_millis() > instant.elapsed().as_millis()
                && self.last_pong.elapsed().as_millis() < timeout.into(),
        )
    }

    pub async fn emit(&self, msg: String) -> Result<(), Error> {
        self.send(Packet::Message(msg)).await
    }

    pub async fn emit_binary(&self, data: Vec<u8>) -> Result<(), Error> {
        self.send(Packet::Binary(data)).await?;
        Ok(())
    }
}
