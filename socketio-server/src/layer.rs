use engineio_server::{service::EngineIoService};
use tower::Layer;

use crate::{socket::Socket, config::SocketIoConfig};

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
    type Service = EngineIoService<S, Socket>;

    fn layer(&self, inner: S) -> Self::Service {
        EngineIoService::from_config(inner, Socket::new(), self.config.engine_config.clone())
    }
}
