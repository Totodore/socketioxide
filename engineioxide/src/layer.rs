use tower::Layer;

use crate::{
    config::EngineIoConfig,
    handler::EngineIoHandler,
    service::{self, EngineIoService},
};

#[derive(Debug, Clone)]
pub struct EngineIoLayer<H: EngineIoHandler> {
    config: EngineIoConfig,
    handler: H,
}

impl<H: EngineIoHandler> EngineIoLayer<H> {
    pub fn new(handler: H) -> Self {
        Self {
            config: EngineIoConfig::default(),
            handler,
        }
    }
    pub fn from_config(handler: H, config: EngineIoConfig) -> Self {
        Self { config, handler }
    }

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

#[cfg(feature = "hyper-v1")]
#[derive(Debug, Clone)]
pub struct EngineIoHyperLayer<H: EngineIoHandler>(EngineIoLayer<H>);

#[cfg(feature = "hyper-v1")]
impl<S: Clone, H: EngineIoHandler + Clone> Layer<S> for EngineIoHyperLayer<H> {
    type Service = service::hyper_v1::EngineIoHyperService<H, S>;

    fn layer(&self, inner: S) -> Self::Service {
        EngineIoService::with_config_inner(inner, self.0.handler.clone(), self.0.config.clone())
            .with_hyper_v1()
    }
}
