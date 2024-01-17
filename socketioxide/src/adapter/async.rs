use std::sync::Weak;

use engineioxide::sid::Sid;
use futures::Future;

use super::{BroadcastOptions, Room, sync};
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
        packet: Packet<'_>,
        opts: BroadcastOptions,
    ) -> impl Future<Output = Result<(), BroadcastError>> + Send;

    /// Broadcasts the packet to the sockets that match the [`BroadcastOptions`] and return a stream of ack responses.
    fn broadcast_with_ack(
        &self,
        packet: Packet<'static>,
        opts: BroadcastOptions,
    ) -> impl Future<Output = AckInnerStream> + Send;

    /// Returns the sockets ids that match the [`BroadcastOptions`].
    fn sockets(
        &self,
        rooms: impl RoomParam,
    ) -> impl Future<Output = Result<Vec<Sid>, Self::Error>> + Send;

    /// Returns the rooms of the socket.
    fn socket_rooms(&self, sid: Sid) -> impl Future<Output = Result<Vec<Room>, Self::Error>>;

    /// Returns the sockets that match the [`BroadcastOptions`].
    fn fetch_sockets(
        &self,
        opts: BroadcastOptions,
    ) -> impl Future<Output = Result<Vec<SocketRef<Self>>, Self::Error>> + Send
    where
        Self: Sized;

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

    //TODO: implement
    // fn server_side_emit(&self, packet: Packet, opts: BroadcastOptions) -> Result<u64, Error>;
    // fn persist_session(&self, sid: i64);
    // fn restore_session(&self, sid: i64) -> Session;
}

impl<T: sync::Adapter> Adapter for T {
    type Error = T::Error;

    #[inline]
    fn new(ns: Weak<Namespace<Self>>) -> Self
    where
        Self: Sized,
    {
        Self::new(ns)
    }

    #[inline]
    fn init(&self) -> impl Future<Output = Result<(), Self::Error>> + Send {
        async move { sync::Adapter::init(self) }
    }

    #[inline]
    fn close(&self) -> impl Future<Output = Result<(), Self::Error>> + Send {
        async move { sync::Adapter::close(self) }
    }

    #[inline]
    fn server_count(&self) -> impl Future<Output = Result<u16, Self::Error>> + Send {
        async move { sync::Adapter::server_count(self) }
    }

    #[inline]
    fn add_all(
        &self,
        sid: Sid,
        rooms: impl RoomParam,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        async move { sync::Adapter::add_all(self, sid, rooms) }
    }

    #[inline]
    fn del(
        &self,
        sid: Sid,
        rooms: impl RoomParam,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        async move { sync::Adapter::del(self, sid, rooms) }
    }

    #[inline]
    fn del_all(&self, sid: Sid) -> impl Future<Output = Result<(), Self::Error>> + Send {
        async move { sync::Adapter::del_all(self, sid) }
    }

    #[inline]
    fn broadcast(
        &self,
        packet: Packet<'_>,
        opts: BroadcastOptions,
    ) -> impl Future<Output = Result<(), BroadcastError>> + Send {
        async move { sync::Adapter::broadcast(self, packet, opts) }
    }

    #[inline]
    fn broadcast_with_ack(
        &self,
        packet: Packet<'static>,
        opts: BroadcastOptions,
    ) -> impl Future<Output = AckInnerStream> + Send {
        async move { sync::Adapter::broadcast_with_ack(self, packet, opts) }
    }

    #[inline]
    fn sockets(
        &self,
        rooms: impl RoomParam,
    ) -> impl Future<Output = Result<Vec<Sid>, Self::Error>> + Send {
        async move { sync::Adapter::sockets(self, rooms) }
    }

    #[inline]
    fn socket_rooms(&self, sid: Sid) -> impl Future<Output = Result<Vec<Room>, Self::Error>> {
        async move { sync::Adapter::socket_rooms(self, sid) }
    }

    #[inline]
    fn fetch_sockets(
        &self,
        opts: BroadcastOptions,
    ) -> impl Future<Output = Result<Vec<SocketRef<Self>>, Self::Error>> + Send
    where
        Self: Sized,
    {
        async move { sync::Adapter::fetch_sockets(self, opts) }
    }

    #[inline]
    fn add_sockets(
        &self,
        opts: BroadcastOptions,
        rooms: impl RoomParam,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        async move { sync::Adapter::add_sockets(self, opts, rooms) }
    }

    #[inline]
    fn del_sockets(
        &self,
        opts: BroadcastOptions,
        rooms: impl RoomParam,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        async move { sync::Adapter::del_sockets(self, opts, rooms) }
    }

    #[inline]
    fn disconnect_socket(
        &self,
        opts: BroadcastOptions,
    ) -> impl Future<Output = Result<(), Vec<DisconnectError>>> + Send {
        async move { sync::Adapter::disconnect_socket(self, opts) }
    }
}
