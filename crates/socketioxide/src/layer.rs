//! ## A tower [`Layer`] for socket.io so it can be used as a middleware with frameworks supporting layers.
//!
//! #### Example with axum :
//! ```rust
//! # use socketioxide::SocketIo;
//! # use axum::routing::get;
//! // Create a socket.io layer
//! let (layer, io) = SocketIo::new_layer();
//!
//! // Add io namespaces and events...
//!
//! let app = axum::Router::<()>::new()
//!     .route("/", get(|| async { "Hello, World!" }))
//!     .layer(layer);
//!
//! // Spawn axum server
//!
//! ```
use std::sync::Arc;

use tower_layer::Layer;

use crate::{
    SocketIoConfig,
    adapter::{Adapter, LocalAdapter},
    client::Client,
    service::SocketIoService,
};

/// A [`Layer`] for [`SocketIoService`], acting as a middleware.
pub struct SocketIoLayer<A: Adapter = LocalAdapter> {
    client: Arc<Client<A>>,
}

impl<A: Adapter> Clone for SocketIoLayer<A> {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
        }
    }
}

impl<A: Adapter> SocketIoLayer<A> {
    pub(crate) fn from_config(
        config: SocketIoConfig,
        adapter_state: A::State,
        #[cfg(feature = "state")] state: state::TypeMap![Send + Sync],
    ) -> (Self, Arc<Client<A>>) {
        let client = Arc::new(Client::new(
            config,
            adapter_state,
            #[cfg(feature = "state")]
            state,
        ));
        let layer = Self {
            client: client.clone(),
        };
        (layer, client)
    }
}

impl<S: Clone, A: Adapter> Layer<S> for SocketIoLayer<A> {
    type Service = SocketIoService<S, A>;

    fn layer(&self, inner: S) -> Self::Service {
        SocketIoService::with_client(inner, self.client.clone())
    }
}
