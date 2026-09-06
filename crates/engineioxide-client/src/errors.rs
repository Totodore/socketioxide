use core::fmt;

use engineioxide_core::Packet;
use http::uri;
use thiserror::Error;

use crate::{
    flavors::TransportSvc,
    transport::{PollingError, WsError},
};

/// Errors that can occur during a connection attempt.
#[derive(Error)]
pub enum ConnectError<S: TransportSvc> {
    /// Client error
    #[error(transparent)]
    Client(ClientError<S>),

    /// Invalid config
    #[error("failed to build client: {0}")]
    Config(#[from] ConfigError),
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("unsupported transport: {0}")]
    UnsupportedTransport(engineioxide_core::TransportType),
    #[error("invalid uri: {0}")]
    InvalidUri(#[from] uri::InvalidUri),
}

/// Errors returned by the client stream
#[derive(Error)]
pub enum ClientError<S: TransportSvc> {
    /// Polling transport error
    #[error("polling transport error: {0}")]
    Polling(PollingError<S>),
    /// Websocket transport error
    #[error("websocket transport error: {0}")]
    Websocket(WsError<S>),

    /// Heartbeat timeout, closing connection
    #[error("heartbeat timeout, closing connection")]
    HeartbeatTimeout,

    /// Transport closed, it is not possible to send or receive data
    #[error("transport closed, it is not possible to send or receive data")]
    TransportClosed,

    /// Invalid packet received from server
    #[error("invalid packet received from server: {got:?}, expected: {expected:?}")]
    InvalidPacket {
        /// Expected packet type
        expected: Option<Box<Packet>>,
        /// Received packet type
        got: Box<Packet>,
    },
}

impl<S: TransportSvc> ClientError<S> {
    pub(crate) fn should_close(&self) -> bool {
        match self {
            ClientError::Polling(e) => e.should_close(),
            ClientError::Websocket(e) => e.should_close(),
            ClientError::TransportClosed => false, // we are already closed, no need to close again
            ClientError::HeartbeatTimeout | ClientError::InvalidPacket { .. } => true,
        }
    }
    pub(crate) fn expected_packet(expected: Packet, got: Packet) -> Self {
        Self::InvalidPacket {
            expected: Some(Box::new(expected)),
            got: Box::new(got),
        }
    }

    pub(crate) fn invalid_packet(got: Packet) -> Self {
        Self::InvalidPacket {
            expected: None,
            got: Box::new(got),
        }
    }
}

impl<S: TransportSvc> fmt::Debug for ClientError<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClientError::Polling(e) => f.debug_tuple("Polling").field(e).finish(),
            ClientError::Websocket(e) => f.debug_tuple("Websocket").field(e).finish(),
            ClientError::TransportClosed => f.write_str("TransportClosed"),
            ClientError::InvalidPacket { expected, got } => f
                .debug_struct("InvalidPacket")
                .field("expected", expected)
                .field("got", got)
                .finish(),
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
