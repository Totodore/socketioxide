//! Adapters are responsible for managing the internal state of the server (rooms, sockets, etc...).
//! When a socket joins or leaves a room, the adapter is responsible for updating the state.
//! The default adapter is the [`LocalAdapter`], which stores the state in memory.
//! Other adapters can be made to share the state between multiple servers.

use socketioxide_core::{
    adapter::{BroadcastOptions, CoreAdapter, CoreLocalAdapter, DefinedAdapter, SocketEmitter},
    packet::Packet,
};
use std::{convert::Infallible, sync::Arc, time::Duration};

pub use crate::ns::Emitter;
pub use socketioxide_core::errors::AdapterError;
/// An adapter is responsible for managing the state of the namespace.
/// This adapter can be implemented to share the state between multiple servers.
/// The default adapter is the [`LocalAdapter`], which stores the state in memory.
pub trait Adapter: CoreAdapter<Emitter> + Sized {}
impl<T: CoreAdapter<Emitter>> Adapter for T {}

// === LocalAdapter impls ===

/// The default adapter. Store the state in memory.
pub struct LocalAdapter(CoreLocalAdapter<Emitter>);

impl CoreAdapter<Emitter> for LocalAdapter {
    type Error = Infallible;
    type State = ();
    type AckStream = <Emitter as SocketEmitter>::AckStream;
    type InitRes = ();

    fn new(_state: &Self::State, local: CoreLocalAdapter<Emitter>) -> Self {
        Self(local)
    }

    fn init(self: Arc<Self>, on_success: impl FnOnce() + Send + 'static) -> Self::InitRes {
        on_success();
    }

    async fn broadcast_with_ack(
        &self,
        packet: Packet,
        opts: BroadcastOptions,
        timeout: Option<Duration>,
    ) -> Result<Self::AckStream, Self::Error> {
        Ok(self.get_local().broadcast_with_ack(packet, opts, timeout).0)
    }

    fn get_local(&self) -> &CoreLocalAdapter<Emitter> {
        &self.0
    }
}
impl DefinedAdapter for LocalAdapter {}
