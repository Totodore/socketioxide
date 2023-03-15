use async_trait::async_trait;
use tower::Layer;

use crate::{engine::EngineIoConfig, errors::Error, service::EngineIoService, socket::Socket};

#[async_trait]
pub trait EngineIoHandler: Send + Sync + 'static {
    async fn handle<H>(&self, msg: String, socket: &mut Socket) -> Result<(), Error>
    where
        H: EngineIoHandler;
}
#[derive(Debug, Clone)]
pub struct EngineIoLayer<H>
where
    H: EngineIoHandler + Clone,
{
    config: EngineIoConfig,
    handler: H,
}

impl<H> EngineIoLayer<H>
where
    H: EngineIoHandler + Clone,
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
    H: EngineIoHandler + Clone,
{
    type Service = EngineIoService<S, H>;

    fn layer(&self, inner: S) -> Self::Service {
        EngineIoService::from_config(inner, self.handler.clone(), self.config.clone())
    }
}
