use std::{collections::HashMap, sync::Arc, time::Duration};

use engineioxide::{
    config::EngineIoConfigBuilder,
    service::{NotFoundService, TransportType},
};
use futures::{stream::BoxStream, Future};
use serde::de::DeserializeOwned;

use crate::{
    adapter::{Adapter, LocalAdapter},
    client::Client,
    handler::AckResponse,
    layer::SocketIoLayer,
    ns::NsHandlers,
    operators::{Operators, RoomParam},
    service::SocketIoService,
    AckError, BroadcastError, Socket, SocketIoConfig,
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

    /// Gracefully closes all the connections and drops every sockets
    ///
    /// Any `on_disconnect` handler will called with [`DisconnectReason::ClosingServer`](crate::DisconnectReason::ClosingServer)
    #[inline]
    pub async fn close(&self) {
        self.0.close().await
    }

    // Chaining operators fns

    /// Select a specific namespace to perform operations on
    ///
    /// ## Example
    /// ```
    /// # use socketioxide::SocketIo;
    /// let (_, io) = SocketIo::builder().ns("custom_ns", |socket| async move {
    ///     println!("Socket connected on /custom_ns namespace with id: {}", socket.sid);
    /// }).build_svc();
    ///
    /// // Later in your code you can select the custom_ns namespace
    /// // and show all sockets connected to it
    /// let sockets = io.of("custom_ns").unwrap().sockets().unwrap();
    /// for socket in sockets {
    ///    println!("found socket on /custom_ns namespace with id: {}", socket.sid);
    /// }
    #[inline]
    pub fn of<'a>(&self, path: impl Into<&'a str>) -> Option<Operators<A>> {
        self.get_op(path.into())
    }

    /// Select all sockets in the given rooms on the root namespace.
    ///
    /// Alias for `io.of("/").unwrap().to(rooms)`
    ///
    /// ## Panics
    /// If the **default namespace "/" is not found** this fn will panic!
    ///
    /// ## Example
    /// ```
    /// # use socketioxide::SocketIo;
    /// let (_, io) = SocketIo::builder().ns("/", |socket| async move {
    ///     println!("Socket connected on / namespace with id: {}", socket.sid);
    /// }).build_svc();
    ///
    /// // Later in your code you can select all sockets in the room "room1"
    /// // and for example show all sockets connected to it
    /// let sockets = io.to("room1").sockets().unwrap();
    /// for socket in sockets {
    ///   println!("found socket on / ns in room1 with id: {}", socket.sid);
    /// }
    #[inline]
    pub fn to(&self, rooms: impl RoomParam) -> Operators<A> {
        self.get_default_op().to(rooms)
    }

    /// Select all sockets in the given rooms on the root namespace.
    ///
    /// Alias for `io.of("/").unwrap().within(rooms)`
    /// Alias for `io.to(rooms)`
    ///
    /// ## Panics
    /// If the **default namespace "/" is not found** this fn will panic!
    ///
    /// ## Example
    /// ```
    /// # use socketioxide::SocketIo;
    /// let (_, io) = SocketIo::builder().ns("/", |socket| async move {
    ///     println!("Socket connected on / namespace with id: {}", socket.sid);
    /// }).build_svc();
    ///
    /// // Later in your code you can select all sockets in the room "room1"
    /// // and for example show all sockets connected to it
    /// let sockets = io.within("room1").sockets().unwrap();
    /// for socket in sockets {
    ///   println!("found socket on / ns in room1 with id: {}", socket.sid);
    /// }
    #[inline]
    pub fn within(&self, rooms: impl RoomParam) -> Operators<A> {
        self.get_default_op().within(rooms)
    }

    /// Filter out all sockets selected with the previous operators which are in the given rooms.
    ///
    /// Alias for `io.of("/").unwrap().except(rooms)`
    ///
    /// ## Panics
    /// If the **default namespace "/" is not found** this fn will panic!
    ///
    /// ## Example
    /// ```
    /// # use socketioxide::SocketIo;
    /// let (_, io) = SocketIo::builder().ns("/", |socket| async move {
    ///     println!("Socket connected on / namespace with id: {}", socket.sid);
    ///     socket.on("register1", |socket, data: (), _, _| async move {
    ///         socket.join("room1");
    ///     });
    ///     socket.on("register2", |socket, data: (), _, _| async move {
    ///         socket.join("room2");
    ///     });
    /// }).build_svc();
    ///
    ///
    /// // Later in your code you can select all sockets in the root namespace that are not in the room1
    /// // and for example show all sockets connected to it
    /// let sockets = io.except("room1").sockets().unwrap();
    /// for socket in sockets {
    ///   println!("found socket on / ns in room1 with id: {}", socket.sid);
    /// }
    #[inline]
    pub fn except(&self, rooms: impl RoomParam) -> Operators<A> {
        self.get_default_op().except(rooms)
    }

    /// Broadcast to all sockets only connected on this node (when using multiple nodes).
    /// When using the default in-memory adapter, this operator is a no-op.
    ///
    /// Alias for `io.of("/").unwrap().local()`
    ///
    /// ## Panics
    /// If the **default namespace "/" is not found** this fn will panic!
    ///
    /// ## Example
    /// ```
    /// # use socketioxide::SocketIo;
    /// let (_, io) = SocketIo::builder().ns("/", |socket| async move {
    ///     println!("Socket connected on / namespace with id: {}", socket.sid);
    /// }).build_svc();
    ///
    /// // Later in your code you can select all sockets in the local node and on the root namespace
    /// // and for example show all sockets connected to it
    /// let sockets = io.local().sockets().unwrap();
    /// for socket in sockets {
    ///   println!("found socket on / ns in room1 with id: {}", socket.sid);
    /// }
    #[inline]
    pub fn local(&self) -> Operators<A> {
        self.get_default_op().local()
    }

    /// Set a custom timeout when sending a message with an acknowledgement.
    ///
    /// Alias for `io.of("/").unwrap().timeout(duration)`
    ///
    /// ## Panics
    /// If the **default namespace "/" is not found** this fn will panic!
    ///
    /// ### Example
    /// ```
    /// # use socketioxide::SocketIo;
    /// # use serde_json::Value;
    /// # use futures::stream::StreamExt;
    /// # use std::time::Duration;
    /// let (_, io) = SocketIo::builder().ns("/", |socket| async move {
    ///     println!("Socket connected on / namespace with id: {}", socket.sid);
    /// }).build_svc();
    ///
    /// // Later in your code you can emit a test message on the root namespace in the room1 and room3 rooms,
    /// // except for the room2 and wait for 5 seconds for an acknowledgement
    /// io.to("room1")
    ///   .to("room3")
    ///   .except("room2")
    ///   .timeout(Duration::from_secs(5))
    ///   .emit_with_ack::<Value>("message-back", "I expect an ack in 5s!")
    ///   .unwrap()
    ///   .for_each(|ack| async move {
    ///      match ack {
    ///          Ok(ack) => println!("Ack received {:?}", ack),
    ///          Err(err) => println!("Ack error {:?}", err),
    ///      }
    ///   });
    #[inline]
    pub fn timeout(&self, timeout: Duration) -> Operators<A> {
        self.get_default_op().timeout(timeout)
    }

    /// Add a binary payload to the message.
    ///
    /// Alias for `io.of("/").unwrap().bin(binary_payload)`
    ///
    /// ## Panics
    /// If the **default namespace "/" is not found** this fn will panic!
    ///
    /// ## Example
    /// ```
    /// # use socketioxide::SocketIo;
    /// # use serde_json::Value;
    /// let (_, io) = SocketIo::builder().ns("/", |socket| async move {
    ///     println!("Socket connected on / namespace with id: {}", socket.sid);
    /// }).build_svc();
    ///
    /// // Later in your code you can emit a test message on the root namespace in the room1 and room3 rooms,
    /// // except for the room2 with a binary payload
    /// io.to("room1")
    ///   .to("room3")
    ///   .except("room2")
    ///   .bin(vec![vec![1, 2, 3, 4]])
    ///   .emit("test", ());
    #[inline]
    pub fn bin(&self, binary: Vec<Vec<u8>>) -> Operators<A> {
        self.get_default_op().bin(binary)
    }

    /// Emit a message to all sockets selected with the previous operators.
    ///
    /// Alias for `io.of("/").unwrap().emit(event, data)`
    ///
    /// ## Panics
    /// If the **default namespace "/" is not found** this fn will panic!
    ///
    /// ## Example
    /// ```
    /// # use socketioxide::SocketIo;
    /// # use serde_json::Value;
    /// let (_, io) = SocketIo::builder().ns("/", |socket| async move {
    ///     println!("Socket connected on / namespace with id: {}", socket.sid);
    /// }).build_svc();
    ///
    /// // Later in your code you can emit a test message on the root namespace in the room1 and room3 rooms,
    /// // except for the room2
    /// io.to("room1")
    ///   .to("room3")
    ///   .except("room2")
    ///   .emit("Hello World!", ());
    #[inline]
    pub fn emit(
        &self,
        event: impl Into<String>,
        data: impl serde::Serialize,
    ) -> Result<(), serde_json::Error> {
        self.get_default_op().emit(event, data)
    }

    /// Emit a message to all sockets selected with the previous operators and return a stream of acknowledgements.
    ///
    /// Each acknowledgement has a timeout specified in the config (5s by default) or with the `timeout()` operator.
    ///
    /// Alias for `io.of("/").unwrap().emit_with_ack(event, data)`
    ///
    /// ## Panics
    /// If the **default namespace "/" is not found** this fn will panic!
    ///
    /// ## Example
    /// ```
    /// # use socketioxide::SocketIo;
    /// # use serde_json::Value;
    /// # use futures::stream::StreamExt;
    /// let (_, io) = SocketIo::builder().ns("/", |socket| async move {
    ///     println!("Socket connected on / namespace with id: {}", socket.sid);
    /// }).build_svc();
    ///
    /// // Later in your code you can emit a test message on the root namespace in the room1 and room3 rooms,
    /// // except for the room2
    /// io.to("room1")
    ///   .to("room3")
    ///   .except("room2")
    ///   .emit_with_ack::<Value>("message-back", "I expect an ack!").unwrap().for_each(|ack| async move {
    ///      match ack {
    ///          Ok(ack) => println!("Ack received {:?}", ack),
    ///          Err(err) => println!("Ack error {:?}", err),
    ///      }
    ///   });
    #[inline]
    pub fn emit_with_ack<V: DeserializeOwned + Send>(
        &self,
        event: impl Into<String>,
        data: impl serde::Serialize,
    ) -> Result<BoxStream<'static, Result<AckResponse<V>, AckError>>, BroadcastError> {
        self.get_default_op().emit_with_ack(event, data)
    }

    /// Get all sockets selected with the previous operators.
    ///
    /// It can be used to retrieve any extension data from the sockets or to make some sockets join other rooms.
    ///
    /// Alias for `io.of("/").unwrap().sockets()`
    ///
    /// ## Panics
    /// If the **default namespace "/" is not found** this fn will panic!
    ///
    /// ## Example
    /// ```
    /// # use socketioxide::SocketIo;
    /// # use serde_json::Value;
    /// let (_, io) = SocketIo::builder().ns("/", |socket| async move {
    ///     println!("Socket connected on / namespace with id: {}", socket.sid);
    /// }).build_svc();
    ///
    /// // Later in your code you can select all sockets in the room "room1"
    /// // and for example show all sockets connected to it
    /// let sockets = io.within("room1").sockets().unwrap();
    /// for socket in sockets {
    ///   println!("found socket on / ns in room1 with id: {}", socket.sid);
    /// }
    #[inline]
    pub fn sockets(&self) -> Result<Vec<Arc<Socket<A>>>, A::Error> {
        self.get_default_op().sockets()
    }

    /// Disconnect all sockets selected with the previous operators.
    ///
    /// Alias for `io.of("/").unwrap().disconnect()`
    ///
    /// ## Panics
    /// If the **default namespace "/" is not found** this fn will panic!
    ///
    /// ## Example
    /// ```
    /// # use socketioxide::SocketIo;
    /// let (_, io) = SocketIo::builder().ns("/", |socket| async move {
    ///     println!("Socket connected on / namespace with id: {}", socket.sid);
    /// }).build_svc();
    ///
    /// // Later in your code you can disconnect all sockets in the root namespace
    /// io.disconnect();
    #[inline]
    pub fn disconnect(&self) -> Result<(), BroadcastError> {
        self.get_default_op().disconnect()
    }

    /// Make all sockets selected with the previous operators join the given room(s).
    ///
    /// Alias for `io.of("/").unwrap().join(rooms)`
    ///
    /// ## Panics
    /// If the **default namespace "/" is not found** this fn will panic!
    ///
    /// ### Example
    /// ```
    /// # use socketioxide::SocketIo;
    /// let (_, io) = SocketIo::builder().ns("/", |socket| async move {
    ///     println!("Socket connected on / namespace with id: {}", socket.sid);
    /// }).build_svc();
    ///
    /// // Later in your code you can for example add all sockets on the root namespace to the room1 and room3
    /// io.join(["room1", "room3"]).unwrap();
    #[inline]
    pub fn join(self, rooms: impl RoomParam) -> Result<(), A::Error> {
        self.get_default_op().join(rooms)
    }

    /// Make all sockets selected with the previous operators leave the given room(s).
    ///
    /// Alias for `io.of("/").unwrap().join(rooms)`
    ///
    /// ## Panics
    /// If the **default namespace "/" is not found** this fn will panic!
    ///
    /// ### Example
    /// ```
    /// # use socketioxide::SocketIo;
    /// let (_, io) = SocketIo::builder().ns("/", |socket| async move {
    ///     println!("Socket connected on / namespace with id: {}", socket.sid);
    /// }).build_svc();
    ///
    /// // Later in your code you can for example remove all sockets on the root namespace from the room1 and room3
    /// io.leave(["room1", "room3"]).unwrap();
    #[inline]
    pub fn leave(self, rooms: impl RoomParam) -> Result<(), A::Error> {
        self.get_default_op().leave(rooms)
    }

    /// Returns a new operator on the given namespace
    #[inline(always)]
    fn get_op(&self, path: &str) -> Option<Operators<A>> {
        self.0.get_ns(path).map(|ns| Operators::new(ns, None))
    }

    /// Returns a new operator on the default namespace "/" (root namespace)
    ///
    /// # Panics
    ///
    /// If the **default namespace "/" is not found** this fn will panic!
    #[inline(always)]
    fn get_default_op(&self) -> Operators<A> {
        self.get_op("/").expect("default namespace not found")
    }
}

impl<A: Adapter> Clone for SocketIo<A> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn get_default_op() {
        let (_, io) = SocketIo::builder().ns("/", |_| async move {}).build_svc();
        let _ = io.get_default_op();
    }

    #[tokio::test]
    #[should_panic(expected = "default namespace not found")]
    async fn get_default_op_panic() {
        let (_, io) = SocketIo::builder()
            .ns("test", |_| async move {})
            .build_svc();
        let _ = io.get_default_op();
    }

    #[tokio::test]
    async fn get_op() {
        let (_, io) = SocketIo::builder()
            .ns("test", |_| async move {})
            .build_svc();
        assert!(io.get_op("test").is_some());
        assert!(io.get_op("test2").is_none());
    }
}
