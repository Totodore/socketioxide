//! ## A tower [`Layer`] for engine.io so it can be used as a middleware with frameworks supporting layers
//! 
//! #### Example with axum : 
//! ```rust
//! # use engineioxide::layer::EngineIoLayer;
//! # use engineioxide::handler::EngineIoHandler;
//! # use engineioxide::{Socket, DisconnectReason};
//! #[derive(Debug, Clone)]
//! struct MyHandler;
//!
//! impl EngineIoHandler for MyHandler {
//!     type Data = ();
//!     fn on_connect(&self, socket: Arc<Socket<()>>) { }
//!     fn on_disconnect(&self, socket: Arc<Socket<()>>, reason: DisconnectReason) { }
//!     fn on_message(&self, msg: String, socket: Arc<Socket<()>>) { }
//!     fn on_binary(&self, data: Vec<u8>, socket: Arc<Socket<()>>) { }
//! }
//! // Create a new engineio layer
//! let layer = EngineIoLayer::new(MyHandler);
//! 
//! let app = axum::Router::new()
//!     .route("/", get(|| async { "Hello, World!" }))
//!     .layer(layer);
//! 
//! axum::Server::bind(&"127.0.0.1:3000".parse().unwrap())
//!     .serve(app.into_make_service())
//! ```
//! 
//! #### Example with salvo (with the `hyper-v1` feature flag) : 
//! ```rust
//! # use engineioxide::layer::EngineIoLayer;
//! # use engineioxide::handler::EngineIoHandler;
//! # use engineioxide::{Socket, DisconnectReason};
//! #[derive(Debug, Clone)]
//! struct MyHandler;
//!
//! impl EngineIoHandler for MyHandler {
//!     type Data = ();
//!     fn on_connect(&self, socket: Arc<Socket<()>>) { }
//!     fn on_disconnect(&self, socket: Arc<Socket<()>>, reason: DisconnectReason) { }
//!     fn on_message(&self, msg: String, socket: Arc<Socket<()>>) { }
//!     fn on_binary(&self, data: Vec<u8>, socket: Arc<Socket<()>>) { }
//! }
//! // Create a new engineio layer
//! let layer = EngineIoLayer::new(MyHandler)
//!     .with_hyper_v1()   // Enable the hyper-v1 compatibility layer for salvo
//!     .compat()          // Enable the salvo compatibility layer
//! 
//! let router = Router::with_path("/engine.io").hoop(layer).goal(hello);
//! let acceptor = TcpListener::new("127.0.0.1:3000").bind().await;
//! Server::new(acceptor).serve(router);
//! ```

use tower::Layer;

use crate::{config::EngineIoConfig, handler::EngineIoHandler, service::EngineIoService};

/// A tower [`Layer`] for engine.io so it can be used as a middleware
#[derive(Debug, Clone)]
pub struct EngineIoLayer<H: EngineIoHandler> {
    config: EngineIoConfig,
    handler: H,
}

impl<H: EngineIoHandler> EngineIoLayer<H> {
    /// Create a new [`EngineIoLayer`] with a given [`Handler`](crate::handler::EngineIoHandler)
    /// and a default [`EngineIoConfig`]
    pub fn new(handler: H) -> Self {
        Self {
            config: EngineIoConfig::default(),
            handler,
        }
    }

    /// Create a new [`EngineIoLayer`] with a given [`Handler`](crate::handler::EngineIoHandler) 
    /// and a custom [`EngineIoConfig`]
    pub fn from_config(handler: H, config: EngineIoConfig) -> Self {
        Self { config, handler }
    }

    /// Create a `hyper-v1` compatible [`Layer`]
    #[cfg(feature = "hyper-v1")]
    #[inline(always)]
    pub fn with_hyper_v1(self) -> EngineIoHyperLayer<H> {
        EngineIoHyperLayer(self)
    }
}

impl<S: Clone, H: EngineIoHandler + Clone> Layer<S> for EngineIoLayer<H> {
    type Service = EngineIoService<H, S>;

    fn layer(&self, inner: S) -> Self::Service {
        EngineIoService::with_config_inner(inner, self.handler.clone(), self.config.clone())
    }
}

/// Wrapper [`Layer`] for [`EngineIoLayer`] so it works with `hyper-v1`
/// 
/// It is only available through the feature flag `hyper-v1`
#[cfg(feature = "hyper-v1")]
#[derive(Debug, Clone)]
pub struct EngineIoHyperLayer<H: EngineIoHandler>(EngineIoLayer<H>);

#[cfg(feature = "hyper-v1")]
impl<S: Clone, H: EngineIoHandler + Clone> Layer<S> for EngineIoHyperLayer<H> {
    type Service = crate::service::hyper_v1::EngineIoHyperService<H, S>;

    fn layer(&self, inner: S) -> Self::Service {
        EngineIoService::with_config_inner(inner, self.0.handler.clone(), self.0.config.clone())
            .with_hyper_v1()
    }
}
