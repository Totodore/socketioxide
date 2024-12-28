use std::convert::Infallible;
use std::sync::Arc;

use crate::{
    adapter::{Adapter, LocalAdapter},
    handler::{FromConnectParts, FromDisconnectParts, FromMessageParts},
    socket::{DisconnectReason, Socket},
    SendError, SocketIo,
};
use serde::Serialize;
use socketioxide_core::{errors::SocketError, packet::Packet, parser::Parse, Value};

/// An Extractor that returns a reference to a [`Socket`].
///
/// It is generic over the [`Adapter`] type. If you plan to use it with another adapter than the default,
/// make sure to have a handler that is [generic over the adapter type](crate#adapters).
#[derive(Debug)]
pub struct SocketRef<A: Adapter = LocalAdapter>(Arc<Socket<A>>);

impl<A: Adapter> FromConnectParts<A> for SocketRef<A> {
    type Error = Infallible;
    fn from_connect_parts(s: &Arc<Socket<A>>, _: &Option<Value>) -> Result<Self, Infallible> {
        Ok(SocketRef(s.clone()))
    }
}
impl<A: Adapter> FromMessageParts<A> for SocketRef<A> {
    type Error = Infallible;
    fn from_message_parts(
        s: &Arc<Socket<A>>,
        _: &mut Value,
        _: &Option<i64>,
    ) -> Result<Self, Infallible> {
        Ok(SocketRef(s.clone()))
    }
}
impl<A: Adapter> FromDisconnectParts<A> for SocketRef<A> {
    type Error = Infallible;
    fn from_disconnect_parts(s: &Arc<Socket<A>>, _: DisconnectReason) -> Result<Self, Infallible> {
        Ok(SocketRef(s.clone()))
    }
}

impl<A: Adapter> std::ops::Deref for SocketRef<A> {
    type Target = Socket<A>;
    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl<A: Adapter> PartialEq for SocketRef<A> {
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        self.0.id == other.0.id
    }
}
impl<A: Adapter> From<Arc<Socket<A>>> for SocketRef<A> {
    #[inline(always)]
    fn from(socket: Arc<Socket<A>>) -> Self {
        Self(socket)
    }
}

impl<A: Adapter> Clone for SocketRef<A> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<A: Adapter> SocketRef<A> {
    /// Disconnect the socket from the current namespace,
    ///
    /// It will also call the disconnect handler if it is set.
    #[inline(always)]
    pub fn disconnect(self) -> Result<(), SocketError> {
        self.0.disconnect()
    }
}

/// An Extractor to send an ack response corresponding to the current event.
/// If the client sent a normal message without expecting an ack, the ack callback will do nothing.
///
/// It is generic over the [`Adapter`] type. If you plan to use it with another adapter than the default,
/// make sure to have a handler that is [generic over the adapter type](crate#adapters).
#[derive(Debug)]
pub struct AckSender<A: Adapter = LocalAdapter> {
    socket: Arc<Socket<A>>,
    ack_id: Option<i64>,
}
impl<A: Adapter> FromMessageParts<A> for AckSender<A> {
    type Error = Infallible;
    fn from_message_parts(
        s: &Arc<Socket<A>>,
        _: &mut Value,
        ack_id: &Option<i64>,
    ) -> Result<Self, Infallible> {
        Ok(Self::new(s.clone(), *ack_id))
    }
}
impl<A: Adapter> AckSender<A> {
    pub(crate) fn new(socket: Arc<Socket<A>>, ack_id: Option<i64>) -> Self {
        Self { socket, ack_id }
    }

    /// Send the ack response to the client.
    pub fn send<T: Serialize + ?Sized>(self, data: &T) -> Result<(), SendError> {
        use crate::socket::PermitExt;
        if let Some(ack_id) = self.ack_id {
            let permit = match self.socket.reserve() {
                Ok(permit) => permit,
                Err(e) => {
                    #[cfg(feature = "tracing")]
                    tracing::debug!("sending error during emit message: {e:?}");
                    return Err(SendError::Socket(e));
                }
            };
            let ns = self.socket.ns.path.clone();
            let data = self.socket.parser.encode_value(data, None)?;
            let packet = Packet::ack(ns, data, ack_id);
            permit.send(packet, self.socket.parser);
            Ok(())
        } else {
            Ok(())
        }
    }
}

impl<A: Adapter> FromConnectParts<A> for crate::ProtocolVersion {
    type Error = Infallible;
    fn from_connect_parts(s: &Arc<Socket<A>>, _: &Option<Value>) -> Result<Self, Infallible> {
        Ok(s.protocol())
    }
}
impl<A: Adapter> FromMessageParts<A> for crate::ProtocolVersion {
    type Error = Infallible;
    fn from_message_parts(
        s: &Arc<Socket<A>>,
        _: &mut Value,
        _: &Option<i64>,
    ) -> Result<Self, Infallible> {
        Ok(s.protocol())
    }
}
impl<A: Adapter> FromDisconnectParts<A> for crate::ProtocolVersion {
    type Error = Infallible;
    fn from_disconnect_parts(s: &Arc<Socket<A>>, _: DisconnectReason) -> Result<Self, Infallible> {
        Ok(s.protocol())
    }
}

impl<A: Adapter> FromConnectParts<A> for crate::TransportType {
    type Error = Infallible;
    fn from_connect_parts(s: &Arc<Socket<A>>, _: &Option<Value>) -> Result<Self, Infallible> {
        Ok(s.transport_type())
    }
}
impl<A: Adapter> FromMessageParts<A> for crate::TransportType {
    type Error = Infallible;
    fn from_message_parts(
        s: &Arc<Socket<A>>,
        _: &mut Value,
        _: &Option<i64>,
    ) -> Result<Self, Infallible> {
        Ok(s.transport_type())
    }
}
impl<A: Adapter> FromDisconnectParts<A> for crate::TransportType {
    type Error = Infallible;
    fn from_disconnect_parts(s: &Arc<Socket<A>>, _: DisconnectReason) -> Result<Self, Infallible> {
        Ok(s.transport_type())
    }
}

impl<A: Adapter> FromDisconnectParts<A> for DisconnectReason {
    type Error = Infallible;
    fn from_disconnect_parts(
        _: &Arc<Socket<A>>,
        reason: DisconnectReason,
    ) -> Result<Self, Infallible> {
        Ok(reason)
    }
}

impl<A: Adapter> FromConnectParts<A> for SocketIo<A> {
    type Error = Infallible;

    fn from_connect_parts(s: &Arc<Socket<A>>, _: &Option<Value>) -> Result<Self, Self::Error> {
        Ok(s.get_io().clone())
    }
}
impl<A: Adapter> FromMessageParts<A> for SocketIo<A> {
    type Error = Infallible;

    fn from_message_parts(
        s: &Arc<Socket<A>>,
        _: &mut Value,
        _: &Option<i64>,
    ) -> Result<Self, Self::Error> {
        Ok(s.get_io().clone())
    }
}
impl<A: Adapter> FromDisconnectParts<A> for SocketIo<A> {
    type Error = Infallible;

    fn from_disconnect_parts(s: &Arc<Socket<A>>, _: DisconnectReason) -> Result<Self, Self::Error> {
        Ok(s.get_io().clone())
    }
}
