//! The adapter module contains the [`CoreAdapter`] trait and other related types.
//!
//! It is used to implement communication between socket.io servers to share messages and state.
use std::{
    borrow::Cow, collections::HashSet, error::Error as StdError, future::Future, time::Duration,
};

use engineioxide::{sid::Sid, Str};
use futures_core::Stream;

use crate::{
    errors::{AdapterError, DisconnectError, SocketError},
    packet::Packet,
    parser::Parse,
    Value,
};

/// A room identifier
pub type Room = Cow<'static, str>;

/// Flags that can be used to modify the behavior of the broadcast methods.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum BroadcastFlags {
    /// Broadcast only to the current server
    Local,
    /// Broadcast to all clients except the sender
    Broadcast,
}

/// Options that can be used to modify the behavior of the broadcast methods.
#[derive(Clone, Debug, Default)]
pub struct BroadcastOptions {
    /// The flags to apply to the broadcast.
    pub flags: HashSet<BroadcastFlags>,
    /// The rooms to broadcast to.
    pub rooms: HashSet<Room>,
    /// The rooms to exclude from the broadcast.
    pub except: HashSet<Room>,
    /// The socket id of the sender.
    pub sid: Option<Sid>,
}

/// A trait for types that can be used as a room parameter.
///
/// [`String`], [`Vec<String>`], [`Vec<&str>`], [`&'static str`](str) and const arrays are implemented by default.
pub trait RoomParam: Send + 'static {
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

/// The [`SocketEmitter`] will be implmented by the socketioxide library.
/// It is simply used as an abstraction to allow the adapter to communicate
/// with the socket server without the need to depend on the socketioxide lib.
pub trait SocketEmitter: Send + Sync + 'static {
    /// An error that can occur when sending data an acknowledgment.
    type AckError: StdError + Send + 'static;
    /// A stream that emits the acknowledgments of multiple sockets.
    type AckStream: Stream<Item = (Sid, Result<Value, Self::AckError>)> + Send + 'static;

    /// Get all the socket ids in the namespace.
    fn get_all_sids(&self) -> Vec<Sid>;
    /// Send data to the list of socket ids.
    fn send_many(&self, sids: Vec<Sid>, data: Value) -> Result<(), Vec<SocketError>>;
    /// Send data to the list of socket ids and get a stream of acks.
    fn send_many_with_ack(
        &self,
        sids: Vec<Sid>,
        packet: Packet,
        timeout: Option<Duration>,
    ) -> Self::AckStream;
    /// Disconnect all the sockets in the list.
    fn disconnect_many(&self, sid: Vec<Sid>) -> Result<(), Vec<DisconnectError>>;
    /// Get the path of the namespace.
    fn path(&self) -> Str;
    /// Get the parser of the namespace.
    fn parser(&self) -> impl Parse;
}

/// An adapter is responsible for managing the state of the namespace.
/// This adapter can be implemented to share the state between multiple servers.
/// The default adapter is the [`LocalAdapter`], which stores the state in memory.
pub trait CoreAdapter<E: SocketEmitter>: Sized + Send + Sync + 'static {
    /// An error that can occur when using the adapter. The default [`LocalAdapter`] has an [`Infallible`] error.
    type Error: StdError + Into<AdapterError> + Send + 'static;
    /// A shared state between all the namespace [`CoreAdapter`].
    /// This can be used to share a connection for example.
    type State: Send + Sync + 'static;

    /// Creates a new adapter with the given state and socket server.
    fn new(state: &Self::State, sockets: E) -> Self;

    /// Initializes the adapter.
    fn init(&self) -> impl Future<Output = Result<(), Self::Error>> + Send;
    /// Closes the adapter.
    fn close(&self) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Returns the number of servers.
    fn server_count(&self) -> impl Future<Output = Result<u16, Self::Error>> + Send;

    /// Adds the socket to all the rooms.
    fn add_all(
        &self,
        sid: Sid,
        rooms: impl RoomParam,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
    /// Removes the socket from the rooms.
    fn del(
        &self,
        sid: Sid,
        rooms: impl RoomParam,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
    /// Removes the socket from all the rooms.
    fn del_all(&self, sid: Sid) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Broadcasts the packet to the sockets that match the [`BroadcastOptions`].
    fn broadcast(
        &self,
        packet: Packet,
        opts: BroadcastOptions,
    ) -> impl Future<Output = Result<(), Vec<SocketError>>> + Send;

    /// Broadcasts the packet to the sockets that match the [`BroadcastOptions`] and return a stream of ack responses.
    fn broadcast_with_ack(
        &self,
        packet: Packet,
        opts: BroadcastOptions,
        timeout: Option<Duration>,
    ) -> impl Future<Output = Result<E::AckStream, Self::Error>> + Send;

    /// Returns the sockets ids that match the [`BroadcastOptions`].
    fn sockets(
        &self,
        opts: BroadcastOptions,
    ) -> impl Future<Output = Result<Vec<Sid>, Self::Error>> + Send;

    /// Returns the rooms of the socket.
    fn socket_rooms(&self, sid: Sid)
        -> impl Future<Output = Result<Vec<Room>, Self::Error>> + Send;

    /// Adds the sockets that match the [`BroadcastOptions`] to the rooms.
    fn add_sockets(
        &self,
        opts: BroadcastOptions,
        rooms: impl RoomParam,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Removes the sockets that match the [`BroadcastOptions`] from the rooms.
    fn del_sockets(
        &self,
        opts: BroadcastOptions,
        rooms: impl RoomParam,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Disconnects the sockets that match the [`BroadcastOptions`].
    fn disconnect_socket(
        &self,
        opts: BroadcastOptions,
    ) -> impl Future<Output = Result<(), Vec<DisconnectError>>> + Send;

    /// Returns all the rooms for this adapter.
    fn rooms(&self) -> impl Future<Output = Result<Vec<Room>, Self::Error>> + Send;

    //TODO: implement
    // fn server_side_emit(&self, packet: Packet, opts: BroadcastOptions) -> Result<u64, Error>;
    // fn persist_session(&self, sid: i64);
    // fn restore_session(&self, sid: i64) -> Session;
}
