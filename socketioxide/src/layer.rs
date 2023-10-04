use std::sync::Arc;

use tower::Layer;

use crate::{
    adapter::Adapter, client::Client, config::SocketIoConfig, ns::NsHandlers,
    service::SocketIoService,
};

/// A [`Layer`] for [`SocketIoService`], acting as a middleware.
pub struct SocketIoLayer<A: Adapter> {
    ns_handlers: NsHandlers<A>,
    client: Arc<Client<A>>,
}

impl<A: Adapter> Clone for SocketIoLayer<A> {
    fn clone(&self) -> Self {
        Self {
            ns_handlers: self.ns_handlers.clone(),
            client: self.client.clone(),
        }
    }
}

impl<A: Adapter> SocketIoLayer<A> {
    pub(crate) fn from_config(
        config: Arc<SocketIoConfig>,
        ns_handlers: NsHandlers<A>,
    ) -> (Self, Arc<Client<A>>) {
        let client = Arc::new(Client::new(config.clone(), ns_handlers.clone()));
        let layer = Self {
            ns_handlers,
            client: client.clone(),
        };
        (layer, client)
    }
}

impl<S: Clone, A: Adapter> Layer<S> for SocketIoLayer<A> {
    type Service = SocketIoService<A, S>;

    fn layer(&self, inner: S) -> Self::Service {
        SocketIoService::with_client(inner, self.client.clone())
    }
}
