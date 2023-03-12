use tower::Layer;

use crate::{service::EngineIoService, engine::EngineIoConfig};

#[derive(Debug, Clone)]
pub struct EngineIoLayer {
	config: EngineIoConfig,
}

impl EngineIoLayer {
    pub fn new() -> Self {
        Self {
			config: EngineIoConfig::default(),
		}
    }
	pub fn from_config(config: EngineIoConfig) -> Self {
		Self {
			config,
		}
	}
}

impl<S> Layer<S> for EngineIoLayer {
    type Service = EngineIoService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        EngineIoService::from_config(inner, self.config.clone())
    }
}
