use std::{
    pin::Pin,
    sync::{Arc, Weak},
};

use futures::Stream;

use serde::de::DeserializeOwned;

use crate::{
    errors::{AckError, Error},
    ns::Namespace,
    operators::{BroadcastOptions, RoomParam},
    packet::Packet,
    socket::{AckResponse, Socket},
};

//TODO: Make an AsyncAdapter trait
pub trait Adapter: Send + Sync + 'static {
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
