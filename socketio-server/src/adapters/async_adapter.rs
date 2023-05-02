use std::{sync::{Arc, Weak}, pin::Pin};

use futures::Stream;
use serde::de::DeserializeOwned;

use crate::{
    errors::{AckError, Error},
    operators::{RoomParam, BroadcastOptions},
    packet::Packet,
    socket::AckResponse,
    Namespace, Socket,
};

#[async_trait]
pub trait AsyncAdapter: Send + Sync + 'static {
    fn new(ns: Weak<Namespace<Self>>) -> Self
    where
        Self: Sized;
    async fn init(&self);
    async fn close(&self);

    async fn server_count(&self) -> u16;

    async fn add_all(&self, sid: i64, rooms: impl RoomParam);
    async fn del(&self, sid: i64, rooms: impl RoomParam);
    async fn del_all(&self, sid: i64);

    async fn broadcast(
        &self,
        packet: Packet,
        binary: Option<Vec<Vec<u8>>>,
        opts: BroadcastOptions,
    ) -> Result<(), Error>;

    async fn broadcast_with_ack<V: DeserializeOwned>(
        &self,
        packet: Packet,
        binary: Option<Vec<Vec<u8>>>,
        opts: BroadcastOptions,
    ) -> Pin<Box<dyn Stream<Item = Result<AckResponse<V>, AckError>>>>;

    async fn sockets(&self, rooms: impl RoomParam) -> Vec<i64>;
    async fn socket_rooms(&self, sid: i64) -> Vec<String>;

    async fn fetch_sockets(&self, opts: BroadcastOptions) -> Vec<Arc<Socket<Self>>>
    where
        Self: Sized;
    async fn add_sockets(&self, opts: BroadcastOptions, rooms: impl RoomParam);
    async fn del_sockets(&self, opts: BroadcastOptions, rooms: impl RoomParam);
    async fn disconnect_socket(&self, opts: BroadcastOptions) -> Result<(), Error>;

    //TODO: implement
    // fn server_side_emit(&self, packet: Packet, opts: BroadcastOptions) -> Result<u64, Error>;
    // fn persist_session(&self, sid: i64);
    // fn restore_session(&self, sid: i64) -> Session;
}
