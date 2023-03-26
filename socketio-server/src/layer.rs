use engineio_server::{service::EngineIoService};
use tower::Layer;

use crate::{handler::Handler, config::SocketIoConfig};

#[derive(Debug, Clone)]
pub struct SocketIoLayer {
    config: SocketIoConfig
}

impl SocketIoLayer {
    pub fn new() -> Self {
        Self {
            config: SocketIoConfig::default(),
        }
    }
    pub fn from_config(config: SocketIoConfig) -> Self {
        Self { config }
    }
}

impl<S> Layer<S> for SocketIoLayer {
    type Service = EngineIoService<S, Handler>;

    fn layer(&self, inner: S) -> Self::Service {
        EngineIoService::from_config(inner, Handler::new(), self.config.engine_config.clone())
    }
}
