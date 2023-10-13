use std::{collections::HashMap, sync::Arc, time::Duration};

use engineioxide::{
    config::EngineIoConfigBuilder,
    service::{NotFoundService, TransportType},
};
use futures::Future;

use crate::{
    adapter::{Adapter, LocalAdapter},
    client::Client,
    layer::SocketIoLayer,
    ns::NsHandlers,
    service::SocketIoService,
    Socket, SocketIoConfig,
};

/// A builder to create a [`SocketIo`] instance
///
/// It contains everything to configure the socket.io server
///
/// It can be used to build either a Tower [`Layer`](https://docs.rs/tower/latest/tower/trait.Layer.html) or a [`Service`](https://docs.rs/tower/latest/tower/trait.Service.html)
pub struct SocketIoBuilder<A: Adapter = LocalAdapter> {
    config: SocketIoConfig,
    engine_config_builder: EngineIoConfigBuilder,
    req_path: String,

    ns_handlers: NsHandlers<A>,
}

impl<A: Adapter> SocketIoBuilder<A> {
    /// Create a new [`SocketIoBuilder`] with default config
    pub fn new() -> Self {
        Self {
            config: SocketIoConfig::default(),
            engine_config_builder: EngineIoConfigBuilder::new(),
            req_path: "/socket.io".to_string(),
            ns_handlers: HashMap::new(),
        }
    }

    /// The path to listen for socket.io requests on.
    ///
    /// Defaults to "/socket.io".
    pub fn req_path(mut self, req_path: String) -> Self {
        self.req_path = req_path;
        self
    }

    /// The interval at which the server will send a ping packet to the client.
    ///
    /// Defaults to 25 seconds.
    pub fn ping_interval(mut self, ping_interval: Duration) -> Self {
        self.engine_config_builder = self.engine_config_builder.ping_interval(ping_interval);
        self
    }

    /// The amount of time the server will wait for a ping response from the client before closing the connection.
    ///
    /// Defaults to 20 seconds.
    pub fn ping_timeout(mut self, ping_timeout: Duration) -> Self {
        self.engine_config_builder = self.engine_config_builder.ping_timeout(ping_timeout);
        self
    }

    /// The maximum number of packets that can be buffered per connection before being emitted to the client.
    /// If the buffer if full the `emit()` method will return an error
    ///
    /// Defaults to 128 packets.
    pub fn max_buffer_size(mut self, max_buffer_size: usize) -> Self {
        self.engine_config_builder = self.engine_config_builder.max_buffer_size(max_buffer_size);
        self
    }

    /// The maximum size of a payload in bytes.
    /// If a payload is bigger than this value the `emit()` method will return an error.
    ///
    /// Defaults to 100 kb.
    pub fn max_payload(mut self, max_payload: u64) -> Self {
        self.engine_config_builder = self.engine_config_builder.max_payload(max_payload);
        self
    }

    /// Allowed transports on this server
    ///
    /// The `transports` array should have a size of 1 or 2
    ///
    /// Defaults to :
    /// `[TransportType::Polling, TransportType::Websocket]`
    pub fn transports<const N: usize>(mut self, transports: [TransportType; N]) -> Self {
        self.engine_config_builder = self.engine_config_builder.transports(transports);
        self
    }

    /// The amount of time the server will wait for an acknowledgement from the client before closing the connection.
    ///
    /// Defaults to 5 seconds.
    pub fn ack_timeout(mut self, ack_timeout: Duration) -> Self {
        self.config.ack_timeout = ack_timeout;
        self
    }

    /// The amount of time before disconnecting a client that has not successfully joined a namespace.
    ///
    /// Defaults to 45 seconds.
    pub fn connect_timeout(mut self, connect_timeout: Duration) -> Self {
        self.config.connect_timeout = connect_timeout;
        self
    }

    /// Register a new namespace handler
    pub fn ns<C, F>(mut self, path: impl Into<String>, callback: C) -> Self
    where
        C: Fn(Arc<Socket<A>>) -> F + Send + Sync + 'static,
        F: Future<Output = ()> + Send + 'static,
    {
        let handler = Arc::new(move |socket| Box::pin(callback(socket)) as _);
        self.ns_handlers.insert(path.into(), handler);
        self
    }

    /// Register a new namespace handler for multiple paths
    pub fn ns_many<C, F>(mut self, paths: Vec<impl Into<String>>, callback: C) -> Self
    where
        C: Fn(Arc<Socket<A>>) -> F + Send + Sync + 'static,
        F: Future<Output = ()> + Send + 'static,
    {
        let handler = Arc::new(move |socket| Box::pin(callback(socket)) as _);
        for path in paths {
            self.ns_handlers.insert(path.into(), handler.clone());
        }
        self
    }

    /// Build a [`SocketIoLayer`] and a [`SocketIo`] instance
    ///
    /// The layer can be used as a tower layer
    pub fn build_layer(mut self) -> (SocketIoLayer<A>, SocketIo<A>) {
        self.config.engine_config = self.engine_config_builder.req_path(self.req_path).build();

        let (layer, client) = SocketIoLayer::from_config(Arc::new(self.config), self.ns_handlers);
        (layer, SocketIo(client))
    }

    /// Build a [`SocketIoService`] and a [`SocketIo`] instance
    ///
    /// This service will be a _standalone_ service that return a 404 error for every non-socket.io request
    /// It can be used as a hyper service
    pub fn build_svc(mut self) -> (SocketIoService<A, NotFoundService>, SocketIo<A>) {
        self.config.engine_config = self.engine_config_builder.req_path(self.req_path).build();

        let (svc, client) = SocketIoService::with_config_inner(
            NotFoundService,
            self.ns_handlers,
            Arc::new(self.config),
        );
        (svc, SocketIo(client))
    }

    /// Build a [`SocketIoService`] and a [`SocketIo`] instance with an inner service
    ///
    /// It can be used as a hyper service
    pub fn build_with_inner_svc<S: Clone>(
        mut self,
        svc: S,
    ) -> (SocketIoService<A, S>, SocketIo<A>) {
        self.config.engine_config = self.engine_config_builder.req_path(self.req_path).build();

        let (svc, client) =
            SocketIoService::with_config_inner(svc, self.ns_handlers, Arc::new(self.config));
        (svc, SocketIo(client))
    }
}

impl<A: Adapter> Default for SocketIoBuilder<A> {
    fn default() -> Self {
        Self::new()
    }
}

/// The [`SocketIo`] instance can be cheaply cloned and moved around everywhere in your program
///
/// It can be used as the main handle to access the whole socket.io context.
pub struct SocketIo<A: Adapter = LocalAdapter>(Arc<Client<A>>);

impl SocketIo<LocalAdapter> {
    pub fn builder() -> SocketIoBuilder<LocalAdapter> {
        SocketIoBuilder::new()
    }
}
impl<A: Adapter> SocketIo<A> {
    /// Returns a reference to the [`SocketIoConfig`] used by this [`SocketIo`] instance
    #[inline]
    pub fn config(&self) -> &SocketIoConfig {
        &self.0.config
    }

    /// Gracefully closes all the connections and drop every sockets
    /// Any `on_disconnect` handler will called with the reason `ServerClosing`
    #[inline]
    pub async fn close(&self) {
        self.0.close().await
    }
}

impl<A: Adapter> Clone for SocketIo<A> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
