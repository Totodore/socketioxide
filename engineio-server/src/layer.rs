use async_trait::async_trait;
use tower::Layer;

use crate::{errors::Error, service::EngineIoService, socket::Socket};

#[async_trait]
pub trait EngineIoHandler: Send + Sync + Clone + 'static {
    fn on_connect(&self, _socket: &Socket<Self>) {}
    fn on_disconnect(&self, _socket: &Socket<Self>) {}

    async fn on_message(&self, _msg: String, _socket: &Socket<Self>) -> Result<(), Error> {
        Ok(())
    }

    async fn on_binary(&self, _data: Vec<u8>, _socket: &Socket<Self>) -> Result<(), Error> {
        Ok(())
    }
}

use std::time::Duration;

#[derive(Debug, Clone)]
pub struct EngineIoConfig {
    pub req_path: String,
    pub ping_interval: Duration,
    pub ping_timeout: Duration,
    pub max_buffer_size: usize,
}

impl Default for EngineIoConfig {
    fn default() -> Self {
        Self {
            req_path: "/engine.io".to_string(),
            ping_interval: Duration::from_millis(300),
            ping_timeout: Duration::from_millis(200),
            max_buffer_size: 128,
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
    pub fn build(self) -> EngineIoConfig {
        self.config
    }
}

#[derive(Debug, Clone)]
pub struct EngineIoLayer<H>
where
    H: EngineIoHandler + ?Sized + Clone,
{
    config: EngineIoConfig,
    handler: H,
}

impl<H> EngineIoLayer<H>
where
    H: EngineIoHandler + ?Sized + Clone,
{
    pub fn new(handler: H) -> Self {
        Self {
            config: EngineIoConfig::default(),
            handler,
        }
    }
    pub fn from_config(handler: H, config: EngineIoConfig) -> Self {
        Self { config, handler }
    }
}

impl<S, H> Layer<S> for EngineIoLayer<H>
where
    H: EngineIoHandler + ?Sized + Clone,
{
    type Service = EngineIoService<S, H>;

    fn layer(&self, inner: S) -> Self::Service {
        EngineIoService::from_config(inner, self.handler.clone(), self.config.clone())
    }
}
