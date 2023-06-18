use tower::Layer;

use crate::{config::EngineIoConfig, handler::EngineIoHandler, service::EngineIoService};

#[derive(Debug, Clone)]
pub struct EngineIoLayer<H: EngineIoHandler>
{
    config: EngineIoConfig,
    handler: H,
}

impl<H: EngineIoHandler> EngineIoLayer<H>
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

impl<S: Clone, H: EngineIoHandler> Layer<S> for EngineIoLayer<H>
{
    type Service = EngineIoService<H, S>;

    fn layer(&self, inner: S) -> Self::Service {
        EngineIoService::with_config_inner(inner, self.handler.clone(), self.config.clone())
    }
}
