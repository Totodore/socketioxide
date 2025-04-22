//! Operators are used to select sockets to send a packet to,
//! or to configure the packet that will be emitted.
//!
//! They use the builder pattern to chain operators.
//!
//! There are two types of operators:
//! * [`ConfOperators`]: Chainable operators to configure the message to be sent.
//! * [`BroadcastOperators`]: Chainable operators to select sockets to send a message to and to configure the message to be sent.
use std::{future::Future, sync::Arc, time::Duration};

use serde::Serialize;
use socketioxide_core::Sid;

use crate::{
    BroadcastError, EmitWithAckError, SendError,
    ack::{AckInnerStream, AckStream},
    adapter::{Adapter, LocalAdapter},
    extract::SocketRef,
    ns::Namespace,
    parser::Parser,
    socket::{RemoteSocket, Socket},
};

use socketioxide_core::{
    adapter::{BroadcastFlags, BroadcastOptions, Room, RoomParam},
    packet::Packet,
    parser::{Parse, ParserError},
};

/// Chainable operators to configure the message to be sent.
pub struct ConfOperators<'a, A: Adapter = LocalAdapter> {
    timeout: Option<Duration>,
    socket: &'a Socket<A>,
}
/// Chainable operators to select sockets to send a message to and to configure the message to be sent.
pub struct BroadcastOperators<A: Adapter = LocalAdapter> {
    timeout: Option<Duration>,
    ns: Arc<Namespace<A>>,
    parser: Parser,
    opts: BroadcastOptions,
}

impl<A: Adapter> From<ConfOperators<'_, A>> for BroadcastOperators<A> {
    fn from(conf: ConfOperators<'_, A>) -> Self {
        let opts = BroadcastOptions::new(conf.socket.id);
        Self {
            timeout: conf.timeout,
            ns: conf.socket.ns.clone(),
            parser: conf.socket.parser,
            opts,
        }
    }
}

// ==== impl ConfOperators operations ====
impl<'a, A: Adapter> ConfOperators<'a, A> {
    pub(crate) fn new(sender: &'a Socket<A>) -> Self {
        Self {
            timeout: None,
            socket: sender,
        }
    }

    #[doc = include_str!("../docs/operators/to.md")]
    pub fn to(self, rooms: impl RoomParam) -> BroadcastOperators<A> {
        BroadcastOperators::from(self).to(rooms)
    }

    #[doc = include_str!("../docs/operators/within.md")]
    pub fn within(self, rooms: impl RoomParam) -> BroadcastOperators<A> {
        BroadcastOperators::from(self).within(rooms)
    }

    #[doc = include_str!("../docs/operators/except.md")]
    pub fn except(self, rooms: impl RoomParam) -> BroadcastOperators<A> {
        BroadcastOperators::from(self).except(rooms)
    }

    #[doc = include_str!("../docs/operators/local.md")]
    pub fn local(self) -> BroadcastOperators<A> {
        BroadcastOperators::from(self).local()
    }

    #[doc = include_str!("../docs/operators/broadcast.md")]
    pub fn broadcast(self) -> BroadcastOperators<A> {
        BroadcastOperators::from(self).broadcast()
    }

    #[doc = include_str!("../docs/operators/timeout.md")]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }
}

// ==== impl ConfOperators consume fns ====
impl<A: Adapter> ConfOperators<'_, A> {
    #[doc = include_str!("../docs/operators/emit.md")]
    pub fn emit<T: ?Sized + Serialize>(
        mut self,
        event: impl AsRef<str>,
        data: &T,
    ) -> Result<(), SendError> {
        use crate::SocketError;
        use crate::socket::PermitExt;
        if !self.socket.connected() {
            return Err(SendError::Socket(SocketError::Closed));
        }
        let permit = match self.socket.reserve() {
            Ok(permit) => permit,
            Err(e) => {
                #[cfg(feature = "tracing")]
                tracing::debug!("sending error during emit message: {e:?}");
                return Err(SendError::Socket(e));
            }
        };
        let packet = self.get_packet(event, data)?;
        permit.send(packet, self.socket.parser);

        Ok(())
    }

    #[doc = include_str!("../docs/operators/emit_with_ack.md")]
    pub fn emit_with_ack<T: ?Sized + Serialize, V>(
        mut self,
        event: impl AsRef<str>,
        data: &T,
    ) -> Result<AckStream<V>, SendError> {
        use crate::SocketError;
        if !self.socket.connected() {
            return Err(SendError::Socket(SocketError::Closed));
        }
        let permit = match self.socket.reserve() {
            Ok(permit) => permit,
            Err(e) => {
                #[cfg(feature = "tracing")]
                tracing::debug!("sending error during emit message: {e:?}");
                return Err(SendError::Socket(e));
            }
        };
        let timeout = self
            .timeout
            .unwrap_or_else(|| self.socket.get_io().config().ack_timeout);
        let packet = self.get_packet(event, data)?;
        let rx = self.socket.send_with_ack_permit(packet, permit);
        let stream = AckInnerStream::send(rx, timeout, self.socket.id);
        Ok(AckStream::<V>::new(stream, self.socket.parser))
    }

    #[doc = include_str!("../docs/operators/join.md")]
    pub fn join(self, rooms: impl RoomParam) {
        self.socket.join(rooms)
    }

    #[doc = include_str!("../docs/operators/leave.md")]
    pub async fn leave(self, rooms: impl RoomParam) {
        self.socket.leave(rooms)
    }

    /// Creates a packet with the given event and data.
    fn get_packet<T: ?Sized + Serialize>(
        &mut self,
        event: impl AsRef<str>,
        data: &T,
    ) -> Result<Packet, ParserError> {
        let ns = self.socket.ns.path.clone();
        let event = event.as_ref();
        let data = self.socket.parser.encode_value(&data, Some(event))?;
        Ok(Packet::event(ns, data))
    }
}

impl<A: Adapter> BroadcastOperators<A> {
    pub(crate) fn new(ns: Arc<Namespace<A>>, parser: Parser) -> Self {
        Self {
            timeout: None,
            ns,
            parser,
            opts: BroadcastOptions::default(),
        }
    }
    pub(crate) fn from_sock(ns: Arc<Namespace<A>>, sid: Sid, parser: Parser) -> Self {
        Self {
            timeout: None,
            ns,
            parser,
            opts: BroadcastOptions::new(sid),
        }
    }

    #[doc = include_str!("../docs/operators/to.md")]
    pub fn to(mut self, rooms: impl RoomParam) -> Self {
        self.opts.rooms.extend(rooms.into_room_iter());
        self.broadcast()
    }

    #[doc = include_str!("../docs/operators/within.md")]
    pub fn within(mut self, rooms: impl RoomParam) -> Self {
        self.opts.rooms.extend(rooms.into_room_iter());
        self
    }

    #[doc = include_str!("../docs/operators/except.md")]
    pub fn except(mut self, rooms: impl RoomParam) -> Self {
        self.opts.except.extend(rooms.into_room_iter());
        self.broadcast()
    }

    #[doc = include_str!("../docs/operators/local.md")]
    pub fn local(mut self) -> Self {
        self.opts.add_flag(BroadcastFlags::Local);
        self
    }

    #[doc = include_str!("../docs/operators/broadcast.md")]
    pub fn broadcast(mut self) -> Self {
        self.opts.add_flag(BroadcastFlags::Broadcast);
        self
    }

    #[doc = include_str!("../docs/operators/timeout.md")]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }
}

// ==== impl BroadcastOperators consume fns ====
impl<A: Adapter> BroadcastOperators<A> {
    #[doc = include_str!("../docs/operators/emit.md")]
    pub fn emit<T: ?Sized + Serialize>(
        mut self,
        event: impl AsRef<str>,
        data: &T,
    ) -> impl Future<Output = Result<(), BroadcastError>> + Send {
        let packet = self.get_packet(event, data);
        async move {
            self.ns
                .adapter
                .broadcast(packet?, self.opts)
                .await
                .map_err(|e| {
                    #[cfg(feature = "tracing")]
                    tracing::debug!("broadcast error: {e}");
                    e
                })?;
            Ok(())
        }
    }

    #[doc = include_str!("../docs/operators/emit_with_ack.md")]
    pub fn emit_with_ack<T: ?Sized + Serialize, V>(
        mut self,
        event: impl AsRef<str>,
        data: &T,
    ) -> impl Future<Output = Result<AckStream<V, A>, EmitWithAckError>> + Send {
        let packet = self.get_packet(event, data);
        async move {
            let stream = self
                .ns
                .adapter
                .broadcast_with_ack(packet?, self.opts, self.timeout)
                .await
                .map_err(|e| EmitWithAckError::Adapter(Box::new(e)))?;
            Ok(AckStream::new(stream, self.parser))
        }
    }

    #[doc = include_str!("../docs/operators/sockets.md")]
    pub fn sockets(self) -> Vec<SocketRef<A>> {
        let ids = self.ns.adapter.get_local().sockets(self.opts);

        ids.into_iter()
            .filter_map(|id| self.ns.get_socket(id).ok())
            .map(SocketRef::from)
            .collect()
    }

    #[doc = include_str!("../docs/operators/fetch_sockets.md")]
    pub async fn fetch_sockets(self) -> Result<Vec<RemoteSocket<A>>, A::Error> {
        let sockets = self
            .ns
            .adapter
            .fetch_sockets(self.opts)
            .await?
            .into_iter()
            .map(|data| RemoteSocket::new(data, &self.ns.adapter, self.parser))
            .collect();
        Ok(sockets)
    }

    #[doc = include_str!("../docs/operators/disconnect.md")]
    pub async fn disconnect(self) -> Result<(), BroadcastError> {
        self.ns.adapter.disconnect_socket(self.opts).await
    }

    #[doc = include_str!("../docs/operators/join.md")]
    #[allow(clippy::manual_async_fn)] // related to issue: https://github.com/rust-lang/rust-clippy/issues/12664
    pub fn join(self, rooms: impl RoomParam) -> impl Future<Output = Result<(), A::Error>> + Send {
        async move { self.ns.adapter.add_sockets(self.opts, rooms).await }
    }

    #[doc = include_str!("../docs/operators/leave.md")]
    #[allow(clippy::manual_async_fn)] // related to issue: https://github.com/rust-lang/rust-clippy/issues/12664
    pub fn leave(self, rooms: impl RoomParam) -> impl Future<Output = Result<(), A::Error>> + Send {
        async move { self.ns.adapter.del_sockets(self.opts, rooms).await }
    }

    #[doc = include_str!("../docs/operators/rooms.md")]
    pub async fn rooms(self) -> Result<Vec<Room>, A::Error> {
        self.ns.adapter.rooms(self.opts).await
    }

    #[doc = include_str!("../docs/operators/get_socket.md")]
    pub fn get_socket(&self, sid: Sid) -> Option<SocketRef<A>> {
        self.ns.get_socket(sid).map(SocketRef::from).ok()
    }

    /// Creates a packet with the given event and data.
    fn get_packet<T: ?Sized + Serialize>(
        &mut self,
        event: impl AsRef<str>,
        data: &T,
    ) -> Result<Packet, ParserError> {
        let ns = self.ns.path.clone();
        let data = self.parser.encode_value(data, Some(event.as_ref()))?;
        Ok(Packet::event(ns, data))
    }
}
