//! Operators are used to select sockets to send a packet to,
//! or to configure the packet that will be emitted.
//!
//! They use the builder pattern to chain operators.
//!
//! There is two types of operators:
//! * [`ConfOperators`]: Chainable operators to configure the message to be sent.
//! * [`BroadcastOperators`]: Chainable operators to select sockets to send a message to and to configure the message to be sent.
use std::borrow::Cow;
use std::{sync::Arc, time::Duration};

use engineioxide::sid::Sid;
use serde::Serialize;
use socketioxide_core::parser::Parse;

use crate::ack::{AckInnerStream, AckStream};
use crate::adapter::LocalAdapter;
use crate::errors::{BroadcastError, DisconnectError};
use crate::extract::SocketRef;
use crate::parser::{self, Parser};
use crate::socket::Socket;
use crate::SendError;
use crate::{
    adapter::{Adapter, BroadcastFlags, BroadcastOptions, Room},
    ns::Namespace,
    packet::Packet,
};

/// A trait for types that can be used as a room parameter.
///
/// [`String`], [`Vec<String>`], [`Vec<&str>`], [`&'static str`](str) and const arrays are implemented by default.
pub trait RoomParam: 'static {
    /// The type of the iterator returned by `into_room_iter`.
    type IntoIter: Iterator<Item = Room>;

    /// Convert `self` into an iterator of rooms.
    fn into_room_iter(self) -> Self::IntoIter;
}

impl RoomParam for Room {
    type IntoIter = std::iter::Once<Room>;
    #[inline(always)]
    fn into_room_iter(self) -> Self::IntoIter {
        std::iter::once(self)
    }
}
impl RoomParam for String {
    type IntoIter = std::iter::Once<Room>;
    #[inline(always)]
    fn into_room_iter(self) -> Self::IntoIter {
        std::iter::once(Cow::Owned(self))
    }
}
impl RoomParam for Vec<String> {
    type IntoIter = std::iter::Map<std::vec::IntoIter<String>, fn(String) -> Room>;
    #[inline(always)]
    fn into_room_iter(self) -> Self::IntoIter {
        self.into_iter().map(Cow::Owned)
    }
}
impl RoomParam for Vec<&'static str> {
    type IntoIter = std::iter::Map<std::vec::IntoIter<&'static str>, fn(&'static str) -> Room>;
    #[inline(always)]
    fn into_room_iter(self) -> Self::IntoIter {
        self.into_iter().map(Cow::Borrowed)
    }
}

impl RoomParam for Vec<Room> {
    type IntoIter = std::vec::IntoIter<Room>;
    #[inline(always)]
    fn into_room_iter(self) -> Self::IntoIter {
        self.into_iter()
    }
}
impl RoomParam for &'static str {
    type IntoIter = std::iter::Once<Room>;
    #[inline(always)]
    fn into_room_iter(self) -> Self::IntoIter {
        std::iter::once(Cow::Borrowed(self))
    }
}
impl<const COUNT: usize> RoomParam for [&'static str; COUNT] {
    type IntoIter =
        std::iter::Map<std::array::IntoIter<&'static str, COUNT>, fn(&'static str) -> Room>;

    #[inline(always)]
    fn into_room_iter(self) -> Self::IntoIter {
        self.into_iter().map(Cow::Borrowed)
    }
}
impl<const COUNT: usize> RoomParam for [String; COUNT] {
    type IntoIter = std::iter::Map<std::array::IntoIter<String, COUNT>, fn(String) -> Room>;
    #[inline(always)]
    fn into_room_iter(self) -> Self::IntoIter {
        self.into_iter().map(Cow::Owned)
    }
}
impl RoomParam for Sid {
    type IntoIter = std::iter::Once<Room>;
    #[inline(always)]
    fn into_room_iter(self) -> Self::IntoIter {
        std::iter::once(Cow::Owned(self.to_string()))
    }
}

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
        let opts = BroadcastOptions {
            sid: Some(conf.socket.id),
            ..Default::default()
        };
        Self {
            timeout: conf.timeout,
            ns: conf.socket.ns.clone(),
            parser: conf.socket.parser(),
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
        use crate::errors::SocketError;
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
        permit.send(packet, self.socket.parser());

        Ok(())
    }

    #[doc = include_str!("../docs/operators/emit_with_ack.md")]
    pub fn emit_with_ack<T: ?Sized + Serialize, V>(
        mut self,
        event: impl AsRef<str>,
        data: &T,
    ) -> Result<AckStream<V>, SendError> {
        use crate::errors::SocketError;
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
        Ok(AckStream::<V>::new(stream, self.socket.parser()))
    }

    #[doc = include_str!("../docs/operators/join.md")]
    pub fn join(self, rooms: impl RoomParam) -> Result<(), A::Error> {
        self.socket.join(rooms)
    }

    #[doc = include_str!("../docs/operators/leave.md")]
    pub fn leave(self, rooms: impl RoomParam) -> Result<(), A::Error> {
        self.socket.leave(rooms)
    }

    /// Gets all room names for a given namespace
    pub fn rooms(self) -> Result<Vec<Room>, A::Error> {
        self.socket.rooms()
    }

    /// Creates a packet with the given event and data.
    fn get_packet<T: ?Sized + Serialize>(
        &mut self,
        event: impl AsRef<str>,
        data: &T,
    ) -> Result<Packet, parser::EncodeError> {
        let ns = self.socket.ns.path.clone();
        let event = event.as_ref();
        let data = self.socket.parser().encode_value(&data, Some(event))?;
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
            opts: BroadcastOptions {
                sid: Some(sid),
                ..Default::default()
            },
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
        self.opts.flags.insert(BroadcastFlags::Local);
        self
    }

    #[doc = include_str!("../docs/operators/broadcast.md")]
    pub fn broadcast(mut self) -> Self {
        self.opts.flags.insert(BroadcastFlags::Broadcast);
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
    ) -> Result<(), BroadcastError> {
        let packet = self.get_packet(event, data)?;
        if let Err(e) = self.ns.adapter.broadcast(packet, self.opts) {
            #[cfg(feature = "tracing")]
            tracing::debug!("broadcast error: {e:?}");
            return Err(e);
        }
        Ok(())
    }

    #[doc = include_str!("../docs/operators/emit_with_ack.md")]
    pub fn emit_with_ack<T: ?Sized + Serialize, V>(
        mut self,
        event: impl AsRef<str>,
        data: &T,
    ) -> Result<AckStream<V>, parser::EncodeError> {
        let packet = self.get_packet(event, data)?;
        let stream = self
            .ns
            .adapter
            .broadcast_with_ack(packet, self.opts, self.timeout);
        Ok(AckStream::new(stream, self.parser))
    }

    #[doc = include_str!("../docs/operators/sockets.md")]
    pub fn sockets(self) -> Result<Vec<SocketRef<A>>, A::Error> {
        self.ns.adapter.fetch_sockets(self.opts)
    }

    #[doc = include_str!("../docs/operators/disconnect.md")]
    pub fn disconnect(self) -> Result<(), Vec<DisconnectError>> {
        self.ns.adapter.disconnect_socket(self.opts)
    }

    #[doc = include_str!("../docs/operators/join.md")]
    pub fn join(self, rooms: impl RoomParam) -> Result<(), A::Error> {
        self.ns.adapter.add_sockets(self.opts, rooms)
    }

    #[doc = include_str!("../docs/operators/leave.md")]
    pub fn leave(self, rooms: impl RoomParam) -> Result<(), A::Error> {
        self.ns.adapter.del_sockets(self.opts, rooms)
    }

    #[doc = include_str!("../docs/operators/rooms.md")]
    pub fn rooms(self) -> Result<Vec<Room>, A::Error> {
        self.ns.adapter.rooms()
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
    ) -> Result<Packet, parser::EncodeError> {
        let ns = self.ns.path.clone();
        let data = self.parser.encode_value(data, Some(event.as_ref()))?;
        Ok(Packet::event(ns, data))
    }
}
