use std::{ops::ControlFlow, time::Duration};

use bytes::BufMut;
use futures::{stream::SplitSink, SinkExt};
use hyper::upgrade::Upgraded;
use tokio::time::{self, Instant};
use tokio_tungstenite::{tungstenite, WebSocketStream};
use tracing::debug;

use crate::{errors::Error, layer::EngineIoHandler, packet::Packet};

#[derive(Debug)]
struct HttpSocket {
    tx: Option<hyper::body::Sender>,
    polling_buffer: Vec<u8>,
}

impl HttpSocket {
    pub fn new() -> Self {
        Self {
            tx: None,
            polling_buffer: Vec::new(),
        }
    }
    pub async fn set_socket(&mut self, mut tx: hyper::body::Sender) -> Result<(), Error> {
        if self.polling_buffer.len() > 0 {
            debug!("New connection, sending {} buffered messages", self.polling_buffer.len());
            tx.send_data(hyper::body::Bytes::from(self.polling_buffer.clone()))
                .await.map_err(Error::from)?;
            self.polling_buffer.clear();
        } else {
            self.tx = Some(tx);
        }
        Ok(())
    }
    pub fn is_connected(&self) -> bool {
        self.tx.is_some()
    }
    pub async fn send(&mut self, msg: String) -> Result<(), Error> {
        if let Some(mut tx) = self.tx.take() {
            debug!("Connection ready, sending message: {:?}", msg);
            tx.send_data(hyper::body::Bytes::from(msg))
                .await
                .map_err(Error::from)?;
        } else {
            debug!("Connection not ready, buffering message: {:?}", msg);
            if self.polling_buffer.len() > 0 {
                self.polling_buffer.push(0x1e);
            }
            self.polling_buffer.put_slice(&msg.into_bytes());
        }
        Ok(())
    }

    pub async fn send_binary(&mut self, mut msg: Vec<u8>) -> Result<(), Error> {
        if let Some(tx) = self.tx.as_mut() {
            tx.send_data(hyper::body::Bytes::from(msg))
                .await
                .map_err(Error::from)?;
        } else {
            if self.polling_buffer.len() > 0 {
                self.polling_buffer.push(0x1e);
            }
            self.polling_buffer.put_slice(&msg);
        }
        Ok(())
    }
    pub fn close(&mut self) {
        if let Some(tx) = self.tx.take() {
            tx.abort();
            self.tx = None;
        }
    }
}
#[derive(Debug)]
pub struct Socket {
    sid: i64,
    http_tx: Option<HttpSocket>,
    ws_tx: Option<SplitSink<WebSocketStream<Upgraded>, tungstenite::Message>>,
    last_pong: Instant,
}

impl Socket {
    pub(crate) fn new_http(sid: i64) -> Self {
        Self {
            sid,
            http_tx: Some(HttpSocket::new()),
            ws_tx: None,
            last_pong: time::Instant::now(),
        }
    }
    pub(crate) fn new_ws(
        sid: i64,
        sender: SplitSink<WebSocketStream<Upgraded>, tungstenite::Message>,
    ) -> Self {
        let socket = Self {
            sid,
            http_tx: None,
            ws_tx: Some(sender),
            last_pong: time::Instant::now(),
        };
        socket
    }

    pub(crate) async fn http_polling_conn(&mut self, tx: hyper::body::Sender) -> Result<(), Error> {
        if let Some(http) = self.http_tx.as_mut() {
            if http.is_connected() {
                return Err(Error::MultiplePollingRequests("Polling connection already established"));
            }
            http.set_socket(tx).await?;
        }
        Ok(())
    }

    pub(crate) fn upgrade_from_http(
        &mut self,
        tx: SplitSink<WebSocketStream<Upgraded>, tungstenite::Message>,
    ) {
        if let Some(http) = &mut self.http_tx {
            http.close();
        }
        self.http_tx = None;
        self.ws_tx = Some(tx);
    }

    pub(crate) fn is_http(&self) -> bool {
        self.http_tx.is_some()
    }
    pub(crate) fn is_ws(&self) -> bool {
        self.ws_tx.is_some()
    }
    pub(crate) fn is_open(&self) -> bool {
        self.http_tx.as_ref()
            .map(|http| http.is_connected())
            .unwrap_or(self.is_ws())
    }

    pub(crate) async fn handle_packet<H>(
        &mut self,
        packet: Packet,
        handler: &H,
    ) -> ControlFlow<Result<(), Error>, Result<(), Error>>
    where
        H: EngineIoHandler,
    {
        tracing::debug!(
            "Received packet from conn http({}) ws({}): {:?}",
            self.is_http(),
            self.is_ws(),
            packet
        );
        match packet {
            Packet::Open(_) => ControlFlow::Continue(Err(Error::BadPacket(
                "Unexpected Open packet, it should be only used in upgrade process",
            ))),
            Packet::Close => ControlFlow::Break(Ok(())),
            Packet::Ping => ControlFlow::Continue(Err(Error::BadPacket("Unexpected Ping packet"))),
            Packet::Pong => {
                self.last_pong = Instant::now();
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
        }
    }

    pub(crate) async fn handle_binary<H>(&mut self, data: Vec<u8>, handler: &H) -> Result<(), Error>
    where
        H: EngineIoHandler,
    {
        handler.handle_binary::<H>(data, self).await
    }

    pub(crate) async fn close(&mut self) -> Result<(), Error> {
        self.send(Packet::Close).await;
        if let Some(http) = &mut self.http_tx {
            http.close();
            self.http_tx = None;
        }
        if let Some(mut tx) = self.ws_tx.take() {
            self.ws_tx = None;
            return tx.close().await.map_err(|e| Error::from(e));
        }
        Ok(())
    }

    pub(crate) async fn send(&mut self, packet: Packet) -> Result<(), Error> {
        let msg: String = packet.try_into().map_err(Error::from)?;
        debug!("Sending packet for sid={}: {:?}", self.sid, msg);
        if let Some(http) = &mut self.http_tx {
            http.send(msg).await?;
        } else if let Some(tx) = &mut self.ws_tx {
            tx.send(tungstenite::Message::Text(msg))
                .await
                .map_err(Error::from)?;
        }
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
        debug!(
            "Sending ping packet for sid={}, waiting for pong (timeout: {})",
            self.sid, timeout
        );
        tokio::time::sleep(Duration::from_millis(timeout)).await;
        Ok(
            self.last_pong.elapsed().as_millis() > instant.elapsed().as_millis()
                && self.last_pong.elapsed().as_millis() < timeout.into(),
        )
    }

    pub async fn emit(&mut self, msg: String) -> Result<(), Error> {
        self.send(Packet::Message(msg)).await
    }

    pub async fn emit_binary(&mut self, data: Vec<u8>) -> Result<(), Error> {
        debug!("Sending packet for sid={}, ws={}, http={}: {:?}", self.sid, self.is_ws(), self.is_http(), data);
        if let Some(http) = &mut self.http_tx {
            http.send_binary(data).await?;
        } else if let Some(tx) = &mut self.ws_tx {
            tx.send(tungstenite::Message::Binary(data))
                .await
                .map_err(Error::from)?;
        }
        Ok(())
    }
}
