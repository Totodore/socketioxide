use core::fmt;

use engineioxide_core::Packet;
use http::uri;
use thiserror::Error;

use crate::transport::{TransportSvc, polling::PollingError, ws::WsError};

#[derive(Error)]
pub enum ConnectError<S: TransportSvc> {
    #[error(transparent)]
    Client(ClientError<S>),
    #[error("failed to build client, invalid uri: {0}")]
    Config(#[from] uri::InvalidUri),
}

#[derive(Error)]
pub enum ClientError<S: TransportSvc> {
    #[error("polling transport error: {0}")]
    Polling(PollingError<S>),
    #[error("websocket transport error: {0}")]
    Websocket(WsError<S>),

    #[error("heartbeat timeout, closing connection")]
    HeartbeatTimeout,

    #[error("transport closed, it is not possible to send or receive data")]
    TransportClosed,
    #[error("invalid packet received from server: {0:?}")]
    InvalidPacket(Packet),
}

impl<S: TransportSvc> ClientError<S> {
    pub(crate) fn should_close(&self) -> bool {
        match self {
            ClientError::Polling(e) => e.should_close(),
            ClientError::Websocket(e) => e.should_close(),
            ClientError::TransportClosed => false, // we are already closed, no need to close again
            ClientError::HeartbeatTimeout | ClientError::InvalidPacket(_) => true,
        }
    }
}

impl<S: TransportSvc> fmt::Debug for ClientError<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClientError::Polling(e) => f.debug_tuple("Polling").field(e).finish(),
            ClientError::Websocket(e) => f.debug_tuple("Websocket").field(e).finish(),
            ClientError::TransportClosed => f.write_str("TransportClosed"),
            ClientError::InvalidPacket(p) => f.debug_tuple("InvalidPacket").field(p).finish(),
            ClientError::HeartbeatTimeout => f.write_str("HeartbeatTimeout"),
        }
    }
}

impl<S: TransportSvc> fmt::Debug for ConnectError<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConnectError::Client(e) => f.debug_tuple("Client").field(e).finish(),
            ConnectError::Config(e) => f.debug_tuple("Config").field(e).finish(),
        }
    }
}

impl<S: TransportSvc> From<PollingError<S>> for ClientError<S> {
    fn from(e: PollingError<S>) -> Self {
        ClientError::Polling(e)
    }
}
impl<S: TransportSvc> From<WsError<S>> for ClientError<S> {
    fn from(e: WsError<S>) -> Self {
        ClientError::Websocket(e)
    }
}

impl<S: TransportSvc> From<ClientError<S>> for ConnectError<S> {
    fn from(value: ClientError<S>) -> Self {
        Self::Client(value)
    }
}
