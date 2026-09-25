//! Errors that can occur when using this crate.

use engineioxide_core::{Packet, PacketParseError};
use http::uri;
use thiserror::Error;

use crate::{
    flavors::{PollingSvc, TransportSvc, WsSvc},
    transport::{PollingError, WsError},
};

pub use crate::transport::ProtocolError;

/// Errors that can occur during a connection attempt.
#[derive(Debug, Error)]
pub enum ConnectError {
    /// Client error
    #[error(transparent)]
    Client(ClientError),

    /// Invalid config
    #[error("failed to build client: {0}")]
    Config(#[from] ConfigError),
}

/// Errors that can occur during configuration.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// The user requested a transport type that is not supported by the flavor.
    #[error("unsupported transport: {0}")]
    UnsupportedTransport(engineioxide_core::TransportType),

    /// The user provided an invalid URI.
    #[error("invalid uri: {0}")]
    InvalidUri(#[from] uri::InvalidUri),
}

/// Errors returned by the client stream
#[derive(Debug, Error)]
#[error("client error: {kind:?}")]
pub struct ClientError {
    #[source]
    source: Option<Box<dyn std::error::Error + Send + Sync + 'static>>,
    kind: ClientErrorKind,
    should_close: bool,
}

/// The category of a [`ClientError`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ClientErrorKind {
    /// Polling transport error
    TransportPolling,
    /// Websocket transport error
    TransportWebsocket,

    /// Heartbeat timeout, closing connection
    HeartbeatTimeout,

    /// Transport closed, it is not possible to send or receive data
    TransportClosed,

    /// Invalid packet received from server
    InvalidPacket,
}

#[derive(Debug, Error)]
#[error("invalid packet, expected: {expected:?}, got: {got:?}")]
struct InvalidPacketError {
    /// Expected packet type
    expected: Option<Box<Packet>>,
    /// Received packet type
    got: Box<Packet>,
}

impl ClientError {
    fn new(kind: ClientErrorKind, should_close: bool) -> Self {
        Self {
            kind,
            should_close,
            source: None,
        }
    }

    fn new_with_source(
        kind: ClientErrorKind,
        should_close: bool,
        err: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            kind,
            should_close,
            source: Some(Box::new(err)),
        }
    }

    /// Returns the category of this error.
    pub fn kind(&self) -> ClientErrorKind {
        self.kind
    }

    /// Returns the protocol error if this is a transport error caused by
    /// an invalid protocol request
    pub fn as_protocol_error(&self) -> Option<&ProtocolError> {
        match self.kind {
            ClientErrorKind::TransportPolling | ClientErrorKind::TransportWebsocket => {
                self.source.as_ref()?.downcast_ref::<ProtocolError>()
            }
            _ => None,
        }
    }

    /// Returns the packet parse error if this is a transport error caused by
    /// an undecodable packet
    pub fn as_packet_error(&self) -> Option<&PacketParseError> {
        match self.kind {
            ClientErrorKind::TransportPolling | ClientErrorKind::TransportWebsocket => {
                self.source.as_ref()?.downcast_ref::<PacketParseError>()
            }
            _ => None,
        }
    }

    pub(crate) fn should_close(&self) -> bool {
        self.should_close
    }
    pub(crate) fn expected_packet(expected: Packet, got: Packet) -> Self {
        let err = InvalidPacketError {
            expected: Some(Box::new(expected)),
            got: Box::new(got),
        };
        Self::new_with_source(ClientErrorKind::InvalidPacket, false, err)
    }

    pub(crate) fn invalid_packet(got: Packet) -> Self {
        let err = InvalidPacketError {
            expected: None,
            got: Box::new(got),
        };
        Self::new_with_source(ClientErrorKind::InvalidPacket, false, err)
    }

    pub(crate) fn heartbeat_timeout() -> Self {
        Self::new(ClientErrorKind::HeartbeatTimeout, true)
    }
    pub(crate) fn transport_closed() -> Self {
        Self::new(ClientErrorKind::TransportClosed, true)
    }

    pub(crate) fn polling<S: PollingSvc + 'static>(e: PollingError<S>) -> Self {
        let should_close = e.should_close();
        match e {
            // unneest a protocol error from the inner transport so we can downcast it
            PollingError::Protocol(err) => {
                Self::new_with_source(ClientErrorKind::TransportPolling, should_close, err)
            }
            PollingError::Packet(err) => {
                Self::new_with_source(ClientErrorKind::TransportPolling, should_close, err)
            }
            PollingError::Closed => Self::new(ClientErrorKind::TransportClosed, should_close),
            _ => Self::new_with_source(ClientErrorKind::TransportPolling, should_close, e),
        }
    }
    pub(crate) fn websocket<S: WsSvc + 'static>(e: WsError<S>) -> Self {
        let should_close = e.should_close();
        match e {
            WsError::Packet(err) => {
                Self::new_with_source(ClientErrorKind::TransportWebsocket, should_close, err)
            }
            WsError::Closed => Self::new(ClientErrorKind::TransportClosed, should_close),
            _ => Self::new_with_source(ClientErrorKind::TransportWebsocket, should_close, e),
        }
    }
}

impl<S: TransportSvc> From<PollingError<S>> for ClientError {
    fn from(e: PollingError<S>) -> Self {
        ClientError::polling(e)
    }
}
impl<S: TransportSvc> From<WsError<S>> for ClientError {
    fn from(e: WsError<S>) -> Self {
        ClientError::websocket(e)
    }
}

impl From<ClientError> for ConnectError {
    fn from(value: ClientError) -> Self {
        Self::Client(value)
    }
}
