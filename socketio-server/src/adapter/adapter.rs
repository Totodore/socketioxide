use crate::{
    errors::{AckError, Error},
    ns::Namespace,
    packet::Packet,
    socket::{AckResponse, Socket},
};
use futures::Stream;
use serde::de::DeserializeOwned;
use std::{
    collections::HashSet,
    pin::Pin,
    sync::{Arc, Weak},
    time::Duration,
};

#[cfg(feature = "remote_adapter")]
use futures_core::Future;

/// A trait for types that can be used as a room parameter.
pub trait RoomParam: Send + 'static {
    type IntoIter: Iterator<Item = Room>;
    fn into_room_iter(self) -> Self::IntoIter;
}

impl RoomParam for Room {
    type IntoIter = std::iter::Once<Room>;
    fn into_room_iter(self) -> Self::IntoIter {
        std::iter::once(self)
    }
}
impl RoomParam for Vec<Room> {
    type IntoIter = std::vec::IntoIter<Room>;
    fn into_room_iter(self) -> Self::IntoIter {
        self.into_iter()
    }
}
impl RoomParam for &'static str {
    type IntoIter = std::iter::Once<Room>;
    fn into_room_iter(self) -> Self::IntoIter {
        std::iter::once(self.to_string())
    }
}
impl<const COUNT: usize> RoomParam for [&'static str; COUNT] {
    type IntoIter =
        std::iter::Map<std::array::IntoIter<&'static str, COUNT>, fn(&'static str) -> Room>;

    fn into_room_iter(self) -> Self::IntoIter {
        self.into_iter().map(|s| s.to_string().to_string())
    }
}

pub type Room = String;

#[derive(Hash, PartialEq, Eq)]
pub enum BroadcastFlags {
    Local,
    Broadcast,
    Timeout(Duration),
}
pub struct BroadcastOptions {
    pub flags: HashSet<BroadcastFlags>,
    pub rooms: Vec<Room>,
    pub except: Vec<Room>,
    pub sid: i64,
}
impl BroadcastOptions {
    pub fn new(sid: i64) -> Self {
        Self {
            flags: HashSet::new(),
            rooms: Vec::new(),
            except: Vec::new(),
            sid,
        }
    }
}

//TODO: Make an AsyncAdapter trait
#[cfg(not(feature = "remote_adapter"))]
pub trait SyncAdapter: Send + Sync + 'static {
    fn new(ns: Weak<Namespace<Self>>) -> Self
    where
        Self: Sized;
    fn init(&self);
    fn close(&self);

    fn server_count(&self) -> u16;

    fn add_all(&self, sid: i64, rooms: impl RoomParam);
    fn del(&self, sid: i64, rooms: impl RoomParam);
    fn del_all(&self, sid: i64);

    fn broadcast(
        &self,
        packet: Packet,
        binary: Option<Vec<Vec<u8>>>,
        opts: BroadcastOptions,
    ) -> Result<(), Error>;

    fn broadcast_with_ack<V: DeserializeOwned>(
        &self,
        packet: Packet,
        binary: Option<Vec<Vec<u8>>>,
        opts: BroadcastOptions,
    ) -> Pin<Box<dyn Stream<Item = Result<AckResponse<V>, AckError>>>>;

    fn sockets(&self, rooms: impl RoomParam) -> Vec<i64>;
    fn socket_rooms(&self, sid: i64) -> Vec<String>;

    fn fetch_sockets(&self, opts: BroadcastOptions) -> Vec<Arc<Socket<Self>>>
    where
        Self: Sized;
    fn add_sockets(&self, opts: BroadcastOptions, rooms: impl RoomParam);
    fn del_sockets(&self, opts: BroadcastOptions, rooms: impl RoomParam);
    fn disconnect_socket(&self, opts: BroadcastOptions) -> Result<(), Error>;

    //TODO: implement
    // fn server_side_emit(&self, packet: Packet, opts: BroadcastOptions) -> Result<u64, Error>;
    // fn persist_session(&self, sid: i64);
    // fn restore_session(&self, sid: i64) -> Session;
}



#[cfg(feature = "remote_adapter")]
/// A custom AdapterFuture rather than async_trait because future needs to be `Sync`
pub type AdapterFuture<T = ()> =
    Pin<Box<dyn Future<Output = Result<T, Error>> + Send + Sync + 'static>>;

#[cfg(feature = "remote_adapter")]
pub trait AsyncAdapter: Send + Sync + 'static {
    fn new(ns: Weak<Namespace<Self>>) -> Self
    where
        Self: Sized;
    fn init(&self) -> AdapterFuture;
    fn close(&self) -> AdapterFuture;

    fn server_count(&self) -> AdapterFuture<u16>;

    fn add_all(&self, sid: i64, rooms: impl RoomParam) -> AdapterFuture;
    fn del(&self, sid: i64, rooms: impl RoomParam) -> AdapterFuture;
    fn del_all(&self, sid: i64) -> AdapterFuture;

    fn broadcast(
        &self,
        packet: Packet,
        binary: Option<Vec<Vec<u8>>>,
        opts: BroadcastOptions,
    ) -> AdapterFuture;

    fn broadcast_with_ack<V: DeserializeOwned>(
        &self,
        packet: Packet,
        binary: Option<Vec<Vec<u8>>>,
        opts: BroadcastOptions,
    ) -> AdapterFuture<Pin<Box<dyn Stream<Item = Result<AckResponse<V>, AckError>>>>>;

    fn sockets(&self, rooms: impl RoomParam) -> AdapterFuture<Vec<i64>>;
    fn socket_rooms(&self, sid: i64) -> AdapterFuture<Vec<String>>;

    fn fetch_sockets(&self, opts: BroadcastOptions) -> AdapterFuture<Vec<Arc<Socket<Self>>>>
    where
        Self: Sized;
    fn add_sockets(&self, opts: BroadcastOptions, rooms: impl RoomParam) -> AdapterFuture;
    fn del_sockets(&self, opts: BroadcastOptions, rooms: impl RoomParam) -> AdapterFuture;
    fn disconnect_socket(&self, opts: BroadcastOptions) -> AdapterFuture;

    //TODO: implement
    // fn server_side_emit(&self, packet: Packet, opts: BroadcastOptions) -> Result<u64, Error>;
    // fn persist_session(&self, sid: i64);
    // fn restore_session(&self, sid: i64) -> Session;
}
