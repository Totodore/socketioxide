use std::convert::Infallible;
use std::sync::Arc;

use crate::handler::{FromConnectParts, FromDisconnectParts, FromMessageParts};
use crate::{
    errors::{DisconnectError, SendError},
    packet::Packet,
    socket::{DisconnectReason, Socket},
    SocketIo,
};
use bytes::Bytes;
use serde::Serialize;

/// An Extractor that returns a reference to a [`Socket`].
#[derive(Debug)]
pub struct SocketRef(Arc<Socket>);

impl FromConnectParts for SocketRef {
    type Error = Infallible;
    fn from_connect_parts(s: &Arc<Socket>, _: &Option<String>) -> Result<Self, Infallible> {
        Ok(SocketRef(s.clone()))
    }
}
impl FromMessageParts for SocketRef {
    type Error = Infallible;
    fn from_message_parts(
        s: &Arc<Socket>,
        _: &mut serde_json::Value,
        _: &mut Vec<Bytes>,
        _: &Option<i64>,
    ) -> Result<Self, Infallible> {
        Ok(SocketRef(s.clone()))
    }
}
impl FromDisconnectParts for SocketRef {
    type Error = Infallible;
    fn from_disconnect_parts(s: &Arc<Socket>, _: DisconnectReason) -> Result<Self, Infallible> {
        Ok(SocketRef(s.clone()))
    }
}

impl std::ops::Deref for SocketRef {
    type Target = Socket;
    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl PartialEq for SocketRef {
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        self.0.id == other.0.id
    }
}
impl From<Arc<Socket>> for SocketRef {
    #[inline(always)]
    fn from(socket: Arc<Socket>) -> Self {
        Self(socket)
    }
}

impl Clone for SocketRef {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl SocketRef {
    /// Disconnect the socket from the current namespace,
    ///
    /// It will also call the disconnect handler if it is set.
    #[inline(always)]
    pub fn disconnect(self) -> Result<(), DisconnectError> {
        self.0.disconnect()
    }
}

/// An Extractor to send an ack response corresponding to the current event.
/// If the client sent a normal message without expecting an ack, the ack callback will do nothing.
#[derive(Debug)]
pub struct AckSender {
    binary: Vec<Bytes>,
    socket: Arc<Socket>,
    ack_id: Option<i64>,
}
impl FromMessageParts for AckSender {
    type Error = Infallible;
    fn from_message_parts(
        s: &Arc<Socket>,
        _: &mut serde_json::Value,
        _: &mut Vec<Bytes>,
        ack_id: &Option<i64>,
    ) -> Result<Self, Infallible> {
        Ok(Self::new(s.clone(), *ack_id))
    }
}
impl AckSender {
    pub(crate) fn new(socket: Arc<Socket>, ack_id: Option<i64>) -> Self {
        Self {
            binary: vec![],
            socket,
            ack_id,
        }
    }

    /// Add binary data to the ack response.
    pub fn bin(mut self, bin: impl IntoIterator<Item = impl Into<Bytes>>) -> Self {
        self.binary = bin.into_iter().map(Into::into).collect();
        self
    }

    /// Send the ack response to the client.
    pub fn send<T: Serialize>(self, data: T) -> Result<(), SendError<T>> {
        use crate::socket::PermitExt;
        if let Some(ack_id) = self.ack_id {
            let permit = match self.socket.reserve() {
                Ok(permit) => permit,
                Err(e) => {
                    #[cfg(feature = "tracing")]
                    tracing::debug!("sending error during emit message: {e:?}");
                    return Err(e.with_value(data).into());
                }
            };
            let ns = self.socket.ns.path.clone();
            let data = serde_json::to_value(data)?;
            let packet = if self.binary.is_empty() {
                Packet::ack(ns, data, ack_id)
            } else {
                Packet::bin_ack(ns, data, self.binary, ack_id)
            };
            permit.send(packet);
            Ok(())
        } else {
            Ok(())
        }
    }
}

impl FromConnectParts for crate::ProtocolVersion {
    type Error = Infallible;
    fn from_connect_parts(s: &Arc<Socket>, _: &Option<String>) -> Result<Self, Infallible> {
        Ok(s.protocol())
    }
}
impl FromMessageParts for crate::ProtocolVersion {
    type Error = Infallible;
    fn from_message_parts(
        s: &Arc<Socket>,
        _: &mut serde_json::Value,
        _: &mut Vec<Bytes>,
        _: &Option<i64>,
    ) -> Result<Self, Infallible> {
        Ok(s.protocol())
    }
}
impl FromDisconnectParts for crate::ProtocolVersion {
    type Error = Infallible;
    fn from_disconnect_parts(s: &Arc<Socket>, _: DisconnectReason) -> Result<Self, Infallible> {
        Ok(s.protocol())
    }
}

impl FromConnectParts for crate::TransportType {
    type Error = Infallible;
    fn from_connect_parts(s: &Arc<Socket>, _: &Option<String>) -> Result<Self, Infallible> {
        Ok(s.transport_type())
    }
}
impl FromMessageParts for crate::TransportType {
    type Error = Infallible;
    fn from_message_parts(
        s: &Arc<Socket>,
        _: &mut serde_json::Value,
        _: &mut Vec<Bytes>,
        _: &Option<i64>,
    ) -> Result<Self, Infallible> {
        Ok(s.transport_type())
    }
}
impl FromDisconnectParts for crate::TransportType {
    type Error = Infallible;
    fn from_disconnect_parts(s: &Arc<Socket>, _: DisconnectReason) -> Result<Self, Infallible> {
        Ok(s.transport_type())
    }
}

impl FromDisconnectParts for DisconnectReason {
    type Error = Infallible;
    fn from_disconnect_parts(
        _: &Arc<Socket>,
        reason: DisconnectReason,
    ) -> Result<Self, Infallible> {
        Ok(reason)
    }
}

impl FromConnectParts for SocketIo {
    type Error = Infallible;

    fn from_connect_parts(s: &Arc<Socket>, _: &Option<String>) -> Result<Self, Self::Error> {
        Ok(s.get_io().clone())
    }
}
impl FromMessageParts for SocketIo {
    type Error = Infallible;

    fn from_message_parts(
        s: &Arc<Socket>,
        _: &mut serde_json::Value,
        _: &mut Vec<Bytes>,
        _: &Option<i64>,
    ) -> Result<Self, Self::Error> {
        Ok(s.get_io().clone())
    }
}
impl FromDisconnectParts for SocketIo {
    type Error = Infallible;

    fn from_disconnect_parts(s: &Arc<Socket>, _: DisconnectReason) -> Result<Self, Self::Error> {
        Ok(s.get_io().clone())
    }
}
