use std::{sync::Arc};

use engineio_server::{engine::EngineIo, service::EngineIoService};
use tower::Layer;

use crate::{client::Client, config::SocketIoConfig, ns::NsHandlers};

#[derive(Clone)]
pub struct SocketIoLayer {
    config: SocketIoConfig,
    ns_handlers: NsHandlers,
}

impl SocketIoLayer {
    pub fn new(ns_handlers: NsHandlers) -> Self {
        Self {
            config: SocketIoConfig::default(),
            ns_handlers,
        }
    }
    pub fn from_config(config: SocketIoConfig, ns_handlers: NsHandlers) -> Self {
        Self {
            config,
            ns_handlers,
        }
    }
}

impl<S> Layer<S> for SocketIoLayer {
    type Service = EngineIoService<S, Client>;

    fn layer(&self, inner: S) -> Self::Service {
        let engine: Arc<EngineIo<Client>> = Arc::new_cyclic(|e| {
            let client = Client::new(
                self.config.clone(),
                e.clone(),
                self.ns_handlers.clone(),
            );
            EngineIo::from_config(client.into(), self.config.engine_config.clone()).into()
        });

        EngineIoService::from_custom_engine(inner, engine)
    }
}
