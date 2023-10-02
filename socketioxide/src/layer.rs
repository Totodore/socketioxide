use std::sync::Arc;

use engineioxide::layer::EngineIoLayer;
use tower::Layer;

use crate::{
    adapter::Adapter, client::Client, config::SocketIoConfig, ns::NsHandlers, SocketIoService,
};

/// A [`Layer`] for [`SocketIoService`], acting as a middleware.
pub struct SocketIoLayer<A: Adapter> {
    config: Arc<SocketIoConfig>,
    ns_handlers: NsHandlers<A>,
    eio_layer: EngineIoLayer<Client<A>>,
}

impl<A: Adapter> Clone for SocketIoLayer<A> {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            ns_handlers: self.ns_handlers.clone(),
            eio_layer: self.eio_layer.clone(),
        }
    }
}

impl<A: Adapter> SocketIoLayer<A> {
    pub(crate) fn from_config(config: Arc<SocketIoConfig>, ns_handlers: NsHandlers<A>) -> Self {
        Self {
            config,
            ns_handlers,
            eio_layer: EngineIoLayer::new(Client::new(config.clone(), ns_handlers.clone())),
        }
    }
}

impl<S: Clone, A: Adapter> Layer<S> for SocketIoLayer<A> {
    type Service = SocketIoService<A, S>;

    fn layer(&self, inner: S) -> Self::Service {
        SocketIoService::with_config_inner(inner, self.ns_handlers.clone(), self.config.clone())
    }
}
