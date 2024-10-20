//! ## A tower [`Layer`] for engine.io so it can be used as a middleware with frameworks supporting layers
//!
//! #### Example with axum :
//! ```rust
//! # use bytes::Bytes;
//! # use engineioxide::layer::EngineIoLayer;
//! # use engineioxide::handler::EngineIoHandler;
//! # use engineioxide::{Socket, DisconnectReason, Str};
//! # use std::sync::Arc;
//! # use axum::routing::get;
//! #[derive(Debug, Clone)]
//! struct MyHandler;
//!
//! impl EngineIoHandler for MyHandler {
//!     type Data = ();
//!     fn on_connect(self: Arc<Self>, socket: Arc<Socket<()>>) { }
//!     fn on_disconnect(&self, socket: Arc<Socket<()>>, reason: DisconnectReason) { }
//!     fn on_message(&self, msg: Str, socket: Arc<Socket<()>>) { }
//!     fn on_binary(&self, data: Bytes, socket: Arc<Socket<()>>) { }
//! }
//! // Create a new engineio layer
//! let layer = EngineIoLayer::new(Arc::new(MyHandler));
//!
//! let app = axum::Router::<()>::new()
//!     .route("/", get(|| async { "Hello, World!" }))
//!     .layer(layer);
//! // Spawn the axum server
//! ```
use std::sync::Arc;
use tower_layer::Layer;

use crate::{config::EngineIoConfig, handler::EngineIoHandler, service::EngineIoService};

/// A tower [`Layer`] for engine.io so it can be used as a middleware
#[derive(Debug, Clone)]
pub struct EngineIoLayer<H: EngineIoHandler> {
    config: EngineIoConfig,
    handler: Arc<H>,
}

impl<H: EngineIoHandler> EngineIoLayer<H> {
    /// Create a new [`EngineIoLayer`] with a given [`Handler`](crate::handler::EngineIoHandler)
    /// and a default [`EngineIoConfig`]
    pub fn new(handler: Arc<H>) -> Self {
        Self {
            config: EngineIoConfig::default(),
            handler,
        }
    }

    /// Create a new [`EngineIoLayer`] with a given [`Handler`](crate::handler::EngineIoHandler)
    /// and a custom [`EngineIoConfig`]
    pub fn from_config(handler: Arc<H>, config: EngineIoConfig) -> Self {
        Self { config, handler }
    }
}

impl<S: Clone, H: EngineIoHandler + Clone> Layer<S> for EngineIoLayer<H> {
    type Service = EngineIoService<H, S>;

    fn layer(&self, inner: S) -> Self::Service {
        EngineIoService::with_config_inner(inner, self.handler.clone(), self.config.clone())
    }
}
