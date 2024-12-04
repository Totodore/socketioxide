//! Adapters are responsible for managing the internal state of the server (rooms, sockets, etc...).
//! When a socket joins or leaves a room, the adapter is responsible for updating the state.
//! The default adapter is the [`LocalAdapter`], which stores the state in memory.
//! Other adapters can be made to share the state between multiple servers.

use crate::ns::Emitter;
use socketioxide_core::adapter::{CoreAdapter, CoreLocalAdapter};
use std::convert::Infallible;

/// An adapter is responsible for managing the state of the namespace.
/// This adapter can be implemented to share the state between multiple servers.
/// The default adapter is the [`LocalAdapter`], which stores the state in memory.
pub trait Adapter: CoreAdapter<Emitter<Self>> + Sized {}
impl<T: CoreAdapter<Emitter<T>>> Adapter for T {}

// === LocalAdapter impls ===

/// The default adapter. Store the state in memory.
pub struct LocalAdapter(CoreLocalAdapter<Emitter<Self>>);

impl CoreAdapter<Emitter<Self>> for LocalAdapter {
    type Error = Infallible;
    type State = ();

    fn new(_state: &Self::State, local: CoreLocalAdapter<Emitter<Self>>) -> Self {
        Self(local)
    }

    fn get_local(&self) -> &CoreLocalAdapter<Emitter<Self>> {
        &self.0
    }
}
