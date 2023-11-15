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
//!
//! #### Example with salvo (with the `hyper-v1` feature flag) :
//! ```rust
//! # use salvo::prelude::*;
//! # use socketioxide::SocketIo;
//! // Create a socket.io layer
//! let (layer, io) = SocketIo::new_layer();
//!
//! // Add io namespaces and events...
//!
//! let layer = layer
//!     .with_hyper_v1();   // Enable the hyper_v1 compatibility layer
//!     // .compat();       // Enable the salvo compatibility layer
//!
//! // Spawn salvo server
//! ```

use std::sync::Arc;

use tower::Layer;

use crate::{
    adapter::{Adapter, LocalAdapter},
    client::Client,
    service::SocketIoService,
    SocketIoConfig,
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
    pub(crate) fn from_config(config: Arc<SocketIoConfig>) -> (Self, Arc<Client<A>>) {
        let client = Arc::new(Client::new(config.clone()));
        let layer = Self {
            client: client.clone(),
        };
        (layer, client)
    }

    /// Convert this [`Layer`] into a [`SocketIoHyperLayer`] to use with hyper v1 and its dependent frameworks.
    ///
    /// This is only available when the `hyper-v1` feature is enabled.
    #[cfg(feature = "hyper-v1")]
    #[inline(always)]
    pub fn with_hyper_v1(self) -> SocketIoHyperLayer<A> {
        SocketIoHyperLayer(self)
    }
}

impl<S: Clone, A: Adapter> Layer<S> for SocketIoLayer<A> {
    type Service = SocketIoService<S, A>;

    fn layer(&self, inner: S) -> Self::Service {
        SocketIoService::with_client(inner, self.client.clone())
    }
}

/// A [`Layer`] for [`SocketIoService`] that works with hyper v1 and its dependent frameworks.
#[cfg(feature = "hyper-v1")]
pub struct SocketIoHyperLayer<A: Adapter>(SocketIoLayer<A>);

#[cfg(feature = "hyper-v1")]
impl<A: Adapter> Clone for SocketIoHyperLayer<A> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
#[cfg(feature = "hyper-v1")]
impl<S: Clone, A: Adapter> Layer<S> for SocketIoHyperLayer<A> {
    type Service = crate::hyper_v1::SocketIoHyperService<S, A>;

    fn layer(&self, inner: S) -> Self::Service {
        SocketIoService::with_client(inner, self.0.client.clone()).with_hyper_v1()
    }
}
