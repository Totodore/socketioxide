#![cfg_attr(docsrs, feature(doc_cfg))]
#![warn(
    clippy::all,
    clippy::todo,
    clippy::empty_enum,
    clippy::mem_forget,
    clippy::unused_self,
    clippy::filter_map_next,
    clippy::needless_continue,
    clippy::needless_borrow,
    clippy::match_wildcard_for_single_variants,
    clippy::if_let_mutex,
    clippy::mismatched_target_os,
    clippy::await_holding_lock,
    clippy::match_on_vec_items,
    clippy::imprecise_flops,
    clippy::suboptimal_flops,
    clippy::lossy_float_literal,
    clippy::rest_pat_in_fully_bound_structs,
    clippy::fn_params_excessive_bools,
    clippy::exit,
    clippy::inefficient_to_string,
    clippy::linkedlist,
    clippy::macro_use_imports,
    clippy::option_option,
    clippy::verbose_file_reads,
    clippy::unnested_or_patterns,
    rust_2018_idioms,
    future_incompatible,
    nonstandard_style,
    missing_docs
)]

mod pubsub;
use futures::stream::BoxStream;
pub use pubsub::PubSubClient;
use serde::de::DeserializeOwned;
use socketioxide::{
    adapter::{Adapter, BroadcastOptions, Room},
    operators::RoomParam,
    packet::Packet,
    socket::AckResponse,
    Sid,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {}

pub struct RedisAdapter<P: PubSubClient> {
    pubsub: P,
    prefix: String,
}
impl<P: PubSubClient> Adapter for RedisAdapter<P> {
    type Error = crate::Error;

    fn new(ns: std::sync::Weak<Namespace<Self>>) -> Self
    where
        Self: Sized,
    {
        todo!()
    }

    fn init(&self) -> Result<(), Self::Error> {
        todo!()
    }

    fn close(&self) -> Result<(), Self::Error> {
        todo!()
    }

    fn server_count(&self) -> Result<u16, Self::Error> {
        todo!()
    }

    fn add_all(&self, sid: Sid, rooms: impl RoomParam) -> Result<(), Self::Error> {
        todo!()
    }

    fn del(&self, sid: Sid, rooms: impl RoomParam) -> Result<(), Self::Error> {
        todo!()
    }

    fn del_all(&self, sid: Sid) -> Result<(), Self::Error> {
        todo!()
    }

    fn broadcast(&self, packet: Packet<'_>, opts: BroadcastOptions) -> Result<(), BroadcastError> {
        todo!()
    }

    fn broadcast_with_ack<V: DeserializeOwned>(
        &self,
        packet: Packet<'static>,
        opts: BroadcastOptions,
    ) -> Result<BoxStream<'static, Result<AckResponse<V>, socketioxide::AckError>>, BroadcastError>
    {
        todo!()
    }

    fn sockets(&self, rooms: impl RoomParam) -> Result<Vec<Sid>, Self::Error> {
        todo!()
    }

    fn socket_rooms(&self, sid: Sid) -> Result<Vec<Room>, Self::Error> {
        todo!()
    }

    fn fetch_sockets(
        &self,
        opts: BroadcastOptions,
    ) -> Result<Vec<socketioxide::extract::SocketRef<Self>>, Self::Error>
    where
        Self: Sized,
    {
        todo!()
    }

    fn add_sockets(
        &self,
        opts: BroadcastOptions,
        rooms: impl RoomParam,
    ) -> Result<(), Self::Error> {
        todo!()
    }

    fn del_sockets(
        &self,
        opts: BroadcastOptions,
        rooms: impl RoomParam,
    ) -> Result<(), Self::Error> {
        todo!()
    }

    fn disconnect_socket(&self, opts: BroadcastOptions) -> Result<(), BroadcastError> {
        todo!()
    }
}
