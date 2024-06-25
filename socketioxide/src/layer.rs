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

use tower::Layer;

use crate::{client::Client, service::SocketIoService, SocketIoConfig};

/// A [`Layer`] for [`SocketIoService`], acting as a middleware.
pub struct SocketIoLayer {
    client: Arc<Client>,
}

impl Clone for SocketIoLayer {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
        }
    }
}

impl SocketIoLayer {
    pub(crate) fn from_config(
        config: SocketIoConfig,
        #[cfg(feature = "state")] state: state::TypeMap![Send + Sync],
    ) -> (Self, Arc<Client>) {
        let client = Arc::new(Client::new(
            config,
            #[cfg(feature = "state")]
            state,
        ));
        let layer = Self {
            client: client.clone(),
        };
        (layer, client)
    }
}

impl<S: Clone> Layer<S> for SocketIoLayer {
    type Service = SocketIoService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        SocketIoService::with_client(inner, self.client.clone())
    }
}
