use std::sync::Arc;

use engineioxide::{engine::EngineIo, service::EngineIoService};
use tower::Layer;

use crate::{adapter::Adapter, client::Client, config::SocketIoConfig, ns::NsHandlers};

pub struct SocketIoLayer<A: Adapter> {
    config: SocketIoConfig,
    ns_handlers: NsHandlers<A>,
}

impl<A: Adapter> Clone for SocketIoLayer<A> {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            ns_handlers: self.ns_handlers.clone(),
        }
    }
}

impl<A: Adapter> SocketIoLayer<A> {
    pub fn new(ns_handlers: NsHandlers<A>) -> Self {
        Self {
            config: SocketIoConfig::default(),
            ns_handlers,
        }
    }
    pub fn from_config(config: SocketIoConfig, ns_handlers: NsHandlers<A>) -> Self {
        Self {
            config,
            ns_handlers,
        }
    }
}

impl<S, A: Adapter> Layer<S> for SocketIoLayer<A> {
    type Service = EngineIoService<S, Client<A>>;

    fn layer(&self, inner: S) -> Self::Service {
        let engine: Arc<EngineIo<Client<A>>> = Arc::new_cyclic(|e| {
            let client = Client::new(self.config.clone(), e.clone(), self.ns_handlers.clone());
            EngineIo::from_config(client.into(), self.config.engine_config.clone()).into()
        });

        EngineIoService::from_custom_engine(inner, engine)
    }
}
