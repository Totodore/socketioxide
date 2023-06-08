use tower::Layer;

use crate::{config::EngineIoConfig, handler::EngineIoHandler, service::EngineIoService};
use std::sync::Arc;

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
