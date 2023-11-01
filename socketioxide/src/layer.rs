use std::sync::Arc;

use tower::Layer;

use crate::{adapter::Adapter, client::Client, service::SocketIoService, SocketIoConfig};

/// A [`Layer`] for [`SocketIoService`], acting as a middleware.
pub struct SocketIoLayer<A: Adapter> {
    client: Arc<Client<A>>,
}

impl<A: Adapter> Clone for SocketIoLayer<A> {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
        }
    }
}

impl<A: Adapter> SocketIoLayer<A> {
    pub(crate) fn from_config(config: Arc<SocketIoConfig>) -> (Self, Arc<Client<A>>) {
        let client = Arc::new(Client::new(config.clone()));
        let layer = Self {
            client: client.clone(),
        };
        (layer, client)
    }

    #[cfg(feature = "hyper-v1")]
    #[inline(always)]
    pub fn with_hyper_v1(self) -> SocketIoHyperLayer<A> {
        SocketIoHyperLayer(self)
    }
}

impl<S: Clone, A: Adapter> Layer<S> for SocketIoLayer<A> {
    type Service = SocketIoService<A, S>;

    fn layer(&self, inner: S) -> Self::Service {
        SocketIoService::with_client(inner, self.client.clone())
    }
}

#[cfg(feature = "hyper-v1")]
pub struct SocketIoHyperLayer<A: Adapter>(SocketIoLayer<A>);

#[cfg(feature = "hyper-v1")]
impl<A: Adapter> Clone for SocketIoHyperLayer<A> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
#[cfg(feature = "hyper-v1")]
impl<S: Clone, A: Adapter> Layer<S> for SocketIoHyperLayer<A> {
    type Service = crate::hyper_v1::SocketIoHyperService<A, S>;

    fn layer(&self, inner: S) -> Self::Service {
        SocketIoService::with_client(inner, self.0.client.clone()).with_hyper_v1()
    }
}
