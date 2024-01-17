//! Adapters are responsible for managing the state of the server.
//! When a socket joins or leaves a room, the adapter is responsible for updating the state.
//! The default adapter is the [`LocalAdapter`], which stores the state in memory.
//! Other adapters can be made to share the state between multiple servers.

use std::sync::Weak;

use engineioxide::sid::Sid;

use super::{BroadcastOptions, Room};
use crate::{
    ack::AckInnerStream,
    errors::{AdapterError, BroadcastError},
    extract::SocketRef,
    ns::Namespace,
    operators::RoomParam,
    packet::Packet,
    DisconnectError,
};

//TODO: Make an AsyncAdapter trait
/// An adapter is responsible for managing the state of the server.
/// This adapter can be implemented to share the state between multiple servers.
/// The default adapter is the [`LocalAdapter`], which stores the state in memory.
pub trait Adapter: std::fmt::Debug + Send + Sync + 'static {
    /// An error that can occur when using the adapter. The default [`LocalAdapter`] has an [`Infallible`] error.
    type Error: std::error::Error + Into<AdapterError> + Send + 'static;

    /// Create a new adapter and give the namespace ref to retrieve sockets.
    fn new(ns: Weak<Namespace<Self>>) -> Self
    where
        Self: Sized;

    /// Initializes the adapter.
    fn init(&self) -> Result<(), Self::Error>;
    /// Closes the adapter.
    fn close(&self) -> Result<(), Self::Error>;

    /// Returns the number of servers.
    fn server_count(&self) -> Result<u16, Self::Error>;

    /// Adds the socket to all the rooms.
    fn add_all(&self, sid: Sid, rooms: impl RoomParam) -> Result<(), Self::Error>;
    /// Removes the socket from the rooms.
    fn del(&self, sid: Sid, rooms: impl RoomParam) -> Result<(), Self::Error>;
    /// Removes the socket from all the rooms.
    fn del_all(&self, sid: Sid) -> Result<(), Self::Error>;

    /// Broadcasts the packet to the sockets that match the [`BroadcastOptions`].
    fn broadcast(&self, packet: Packet<'_>, opts: BroadcastOptions) -> Result<(), BroadcastError>;

    /// Broadcasts the packet to the sockets that match the [`BroadcastOptions`] and return a stream of ack responses.
    fn broadcast_with_ack(&self, packet: Packet<'static>, opts: BroadcastOptions)
        -> AckInnerStream;

    /// Returns the sockets ids that match the [`BroadcastOptions`].
    fn sockets(&self, rooms: impl RoomParam) -> Result<Vec<Sid>, Self::Error>;

    /// Returns the rooms of the socket.
    fn socket_rooms(&self, sid: Sid) -> Result<Vec<Room>, Self::Error>;

    /// Returns the sockets that match the [`BroadcastOptions`].
    fn fetch_sockets(&self, opts: BroadcastOptions) -> Result<Vec<SocketRef<Self>>, Self::Error>
    where
        Self: Sized;

    /// Adds the sockets that match the [`BroadcastOptions`] to the rooms.
    fn add_sockets(&self, opts: BroadcastOptions, rooms: impl RoomParam)
        -> Result<(), Self::Error>;
    /// Removes the sockets that match the [`BroadcastOptions`] from the rooms.
    fn del_sockets(&self, opts: BroadcastOptions, rooms: impl RoomParam)
        -> Result<(), Self::Error>;

    /// Disconnects the sockets that match the [`BroadcastOptions`].
    fn disconnect_socket(&self, opts: BroadcastOptions) -> Result<(), Vec<DisconnectError>>;

    //TODO: implement
    // fn server_side_emit(&self, packet: Packet, opts: BroadcastOptions) -> Result<u64, Error>;
    // fn persist_session(&self, sid: i64);
    // fn restore_session(&self, sid: i64) -> Session;
}
