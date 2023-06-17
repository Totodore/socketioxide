use tower::Layer;

use crate::{adapter::Adapter, config::SocketIoConfig, ns::NsHandlers, SocketIoService};

/// A [`Layer`] for [`SocketIoService`], acting as a middleware.
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

impl<S: Clone, A: Adapter> Layer<S> for SocketIoLayer<A> {
    type Service = SocketIoService<A, S>;

    fn layer(&self, inner: S) -> Self::Service {
        SocketIoService::with_config_inner(inner, self.ns_handlers.clone(), self.config.clone())
    }
}
