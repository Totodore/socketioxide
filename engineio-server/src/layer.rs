use async_trait::async_trait;
use tower::Layer;

use crate::{service::EngineIoService, socket::Socket};
use std::{sync::Arc, time::Duration};

/// An handler for engine.io events for each sockets.
#[async_trait]
pub trait EngineIoHandler: Send + Sync + 'static {

    /// Data associated with the socket.
    type Data: Default + Send + Sync + 'static;

    /// Called when a new socket is connected.
    fn on_connect(self: Arc<Self>, socket: &Socket<Self>);

    /// Called when a socket is disconnected.
    fn on_disconnect(self: Arc<Self>, socket: &Socket<Self>);

    /// Called when a message is received from the client.
    async fn on_message(self: Arc<Self>, msg: String, socket: &Socket<Self>);

    /// Called when a binary message is received from the client.
    async fn on_binary(self: Arc<Self>, data: Vec<u8>, socket: &Socket<Self>);
}

#[derive(Debug, Clone)]
pub struct EngineIoConfig {
    /// The path to listen for engine.io requests on.
    /// Defaults to "/engine.io".
    pub req_path: String,

    /// The interval at which the server will send a ping packet to the client.
    /// Defaults to 25 seconds.
    pub ping_interval: Duration,

    /// The amount of time the server will wait for a ping response from the client before closing the connection.
    /// Defaults to 20 seconds.
    pub ping_timeout: Duration,

    /// The maximum number of packets that can be buffered per connection before being emitted to the client.
    ///
    /// If the buffer if full the `emit()` method will return an error
    /// ```
    /// use engineio_server::{
    ///     layer::{EngineIoHandler, EngineIoLayer},
    ///     socket::Socket,
    /// };
    /// use std::sync::Arc;
    /// #[derive(Clone)]
    /// struct MyHandler;
    ///
    /// #[engineio_server::async_trait]
    /// impl EngineIoHandler for MyHandler {
    ///
    ///     type Data = ();
    ///     fn on_connect(self: Arc<Self>, socket: &Socket<Self>) {
    ///         println!("socket connect {}", socket.sid);
    ///     }
    ///     fn on_disconnect(self: Arc<Self>, socket: &Socket<Self>) {
    ///         println!("socket disconnect {}", socket.sid);
    ///     }
    ///
    ///     async fn on_message(self: Arc<Self>, msg: String, socket: &Socket<Self>) {
    ///         println!("Ping pong message {:?}", msg);
    ///         socket.emit(msg).unwrap();
    ///     }
    ///
    ///     async fn on_binary(self: Arc<Self>, data: Vec<u8>, socket: &Socket<Self>) {
    ///         println!("Ping pong binary message {:?}", data);
    ///         socket.emit_binary(data).unwrap();
    ///     }
    /// }
    /// ```
    pub max_buffer_size: usize,

    /// The maximum number of bytes that can be received per http request.
    /// Defaults to 100kb.
    pub max_payload: u64,
}

impl Default for EngineIoConfig {
    fn default() -> Self {
        Self {
            req_path: "/engine.io".to_string(),
            ping_interval: Duration::from_millis(25000),
            ping_timeout: Duration::from_millis(20000),
            max_buffer_size: 128,
            max_payload: 1e5 as u64, // 100kb
        }
    }
}

impl EngineIoConfig {
    pub fn builder() -> EngineIoConfigBuilder {
        EngineIoConfigBuilder::new()
    }
}
pub struct EngineIoConfigBuilder {
    config: EngineIoConfig,
}

impl EngineIoConfigBuilder {
    pub fn new() -> Self {
        Self {
            config: EngineIoConfig::default(),
        }
    }
    pub fn req_path(mut self, req_path: String) -> Self {
        self.config.req_path = req_path;
        self
    }
    pub fn ping_interval(mut self, ping_interval: Duration) -> Self {
        self.config.ping_interval = ping_interval;
        self
    }
    pub fn ping_timeout(mut self, ping_timeout: Duration) -> Self {
        self.config.ping_timeout = ping_timeout;
        self
    }
    pub fn max_buffer_size(mut self, max_buffer_size: usize) -> Self {
        self.config.max_buffer_size = max_buffer_size;
        self
    }
    pub fn max_payload(mut self, max_payload: u64) -> Self {
        self.config.max_payload = max_payload;
        self
    }
    pub fn build(self) -> EngineIoConfig {
        self.config
    }
}

#[derive(Debug, Clone)]
pub struct EngineIoLayer<H>
where
    H: EngineIoHandler,
{
    config: EngineIoConfig,
    handler: Arc<H>,
}

impl<H> EngineIoLayer<H>
where
    H: EngineIoHandler,
{
    pub fn new(handler: H) -> Self {
        Self {
            config: EngineIoConfig::default(),
            handler: handler.into(),
        }
    }
    pub fn from_config(handler: H, config: EngineIoConfig) -> Self {
        Self {
            config,
            handler: handler.into(),
        }
    }
}

impl<S, H> Layer<S> for EngineIoLayer<H>
where
    H: EngineIoHandler,
{
    type Service = EngineIoService<S, H>;

    fn layer(&self, inner: S) -> Self::Service {
        EngineIoService::from_config(inner, self.handler.clone(), self.config.clone())
    }
}
