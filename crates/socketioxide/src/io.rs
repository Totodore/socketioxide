use std::{borrow::Cow, fmt, sync::Arc, time::Duration};

use engineioxide::{
    TransportType,
    config::{EngineIoConfig, EngineIoConfigBuilder},
    service::NotFoundService,
};
use serde::Serialize;
use socketioxide_core::{
    Sid, Uid,
    adapter::{DefinedAdapter, Room, RoomParam},
};
use socketioxide_parser_common::CommonParser;
#[cfg(feature = "msgpack")]
use socketioxide_parser_msgpack::MsgPackParser;

use crate::{
    BroadcastError, EmitWithAckError,
    ack::AckStream,
    adapter::{Adapter, LocalAdapter},
    client::Client,
    extract::SocketRef,
    handler::ConnectHandler,
    layer::SocketIoLayer,
    operators::BroadcastOperators,
    parser::Parser,
    service::SocketIoService,
    socket::RemoteSocket,
};

/// The parser to use to encode and decode socket.io packets
///
/// Be sure that the selected parser matches the client parser.
#[derive(Debug, Clone)]
pub struct ParserConfig(Parser);

impl ParserConfig {
    /// Use a [`CommonParser`] to parse incoming and outgoing socket.io packets
    pub fn common() -> Self {
        ParserConfig(Parser::Common(CommonParser))
    }

    /// Use a [`MsgPackParser`] to parse incoming and outgoing socket.io packets
    #[cfg_attr(docsrs, doc(cfg(feature = "msgpack")))]
    #[cfg(feature = "msgpack")]
    pub fn msgpack() -> Self {
        ParserConfig(Parser::MsgPack(MsgPackParser))
    }
}

/// Configuration for Socket.IO & Engine.IO
#[derive(Debug, Clone)]
pub struct SocketIoConfig {
    /// The inner Engine.IO config
    pub engine_config: EngineIoConfig,

    /// The amount of time the server will wait for an acknowledgement from the client before closing the connection.
    ///
    /// Defaults to 5 seconds.
    pub ack_timeout: Duration,

    /// The amount of time before disconnecting a client that has not successfully joined a namespace.
    ///
    /// Defaults to 45 seconds.
    pub connect_timeout: Duration,

    /// The parser to use to encode and decode socket.io packets
    pub(crate) parser: Parser,

    /// A global server identifier
    pub server_id: Uid,
}

impl Default for SocketIoConfig {
    fn default() -> Self {
        Self {
            engine_config: EngineIoConfig {
                req_path: "/socket.io".into(),
                ..Default::default()
            },
            ack_timeout: Duration::from_secs(5),
            connect_timeout: Duration::from_secs(45),
            parser: Parser::default(),
            server_id: Uid::new(),
        }
    }
}

/// A builder to create a [`SocketIo`] instance.
/// It contains everything to configure the socket.io server with a [`SocketIoConfig`].
/// It can be used to build either a Tower [`Layer`](tower_layer::Layer) or a [`Service`](tower_service::Service).
pub struct SocketIoBuilder<A: Adapter = LocalAdapter> {
    config: SocketIoConfig,
    engine_config_builder: EngineIoConfigBuilder,
    adapter_state: A::State,
    #[cfg(feature = "state")]
    state: state::TypeMap![Send + Sync],
}

impl SocketIoBuilder<LocalAdapter> {
    /// Creates a new [`SocketIoBuilder`] with default config
    pub fn new() -> Self {
        Self {
            engine_config_builder: EngineIoConfigBuilder::new().req_path("/socket.io".to_string()),
            config: SocketIoConfig::default(),
            adapter_state: (),
            #[cfg(feature = "state")]
            state: std::default::Default::default(),
        }
    }
}
impl<A: Adapter> SocketIoBuilder<A> {
    /// The path to listen for socket.io requests on.
    ///
    /// Defaults to "/socket.io".
    #[inline]
    pub fn req_path(mut self, req_path: impl Into<Cow<'static, str>>) -> Self {
        self.engine_config_builder = self.engine_config_builder.req_path(req_path);
        self
    }

    /// The interval at which the server will send a ping packet to the client.
    ///
    /// Defaults to 25 seconds.
    #[inline]
    pub fn ping_interval(mut self, ping_interval: Duration) -> Self {
        self.engine_config_builder = self.engine_config_builder.ping_interval(ping_interval);
        self
    }

    /// The amount of time the server will wait for a ping response from the client before closing the connection.
    ///
    /// Defaults to 20 seconds.
    #[inline]
    pub fn ping_timeout(mut self, ping_timeout: Duration) -> Self {
        self.engine_config_builder = self.engine_config_builder.ping_timeout(ping_timeout);
        self
    }

    /// The maximum number of packets that can be buffered per connection before being emitted to the client.
    /// If the buffer if full the `emit()` method will return an error
    ///
    /// Defaults to 128 packets.
    #[inline]
    pub fn max_buffer_size(mut self, max_buffer_size: usize) -> Self {
        self.engine_config_builder = self.engine_config_builder.max_buffer_size(max_buffer_size);
        self
    }

    /// The maximum size of a payload in bytes.
    /// If a payload is bigger than this value the `emit()` method will return an error.
    ///
    /// Defaults to 100 kb.
    #[inline]
    pub fn max_payload(mut self, max_payload: u64) -> Self {
        self.engine_config_builder = self.engine_config_builder.max_payload(max_payload);
        self
    }

    /// The size of the read buffer for the websocket transport.
    /// You can tweak this value depending on your use case. Defaults to 4KiB.
    ///
    /// Setting it to a higher value will improve performance on heavy read scenarios
    /// but will consume more memory.
    #[inline]
    pub fn ws_read_buffer_size(mut self, ws_read_buffer_size: usize) -> Self {
        self.engine_config_builder = self
            .engine_config_builder
            .ws_read_buffer_size(ws_read_buffer_size);
        self
    }

    /// Allowed transports on this server
    ///
    /// The `transports` array should have a size of 1 or 2
    ///
    /// Defaults to :
    /// `[TransportType::Polling, TransportType::Websocket]`
    #[inline]
    pub fn transports<const N: usize>(mut self, transports: [TransportType; N]) -> Self {
        self.engine_config_builder = self.engine_config_builder.transports(transports);
        self
    }

    /// The amount of time the server will wait for an acknowledgement from the client before closing the connection.
    ///
    /// Defaults to 5 seconds.
    #[inline]
    pub fn ack_timeout(mut self, ack_timeout: Duration) -> Self {
        self.config.ack_timeout = ack_timeout;
        self
    }

    /// The amount of time before disconnecting a client that has not successfully joined a namespace.
    ///
    /// Defaults to 45 seconds.
    #[inline]
    pub fn connect_timeout(mut self, connect_timeout: Duration) -> Self {
        self.config.connect_timeout = connect_timeout;
        self
    }

    /// Sets a custom [`SocketIoConfig`] created previously for this [`SocketIoBuilder`]
    #[inline]
    pub fn with_config(mut self, config: SocketIoConfig) -> Self {
        self.config = config;
        self
    }

    /// Set a custom [`ParserConfig`] for this [`SocketIoBuilder`]
    /// ```
    /// # use socketioxide::{SocketIo, ParserConfig};
    /// let (io, layer) = SocketIo::builder()
    ///     .with_parser(ParserConfig::msgpack())
    ///     .build_layer();
    /// ```
    #[inline]
    pub fn with_parser(mut self, parser: ParserConfig) -> Self {
        self.config.parser = parser.0;
        self
    }

    /// Set a custom [`Adapter`] for this [`SocketIoBuilder`]
    pub fn with_adapter<B: Adapter>(self, adapter_state: B::State) -> SocketIoBuilder<B> {
        SocketIoBuilder {
            config: self.config,
            engine_config_builder: self.engine_config_builder,
            adapter_state,
            #[cfg(feature = "state")]
            state: self.state,
        }
    }

    /// Add a custom global state for the [`SocketIo`] instance.
    /// This state will be accessible from every handler with the [`State`](crate::extract::State) extractor.
    /// You can set any number of states as long as they have different types.
    /// The state must be cloneable, therefore it is recommended to wrap it in an `Arc` if you want shared state.
    #[inline]
    #[cfg_attr(docsrs, doc(cfg(feature = "state")))]
    #[cfg(feature = "state")]
    pub fn with_state<S: Clone + Send + Sync + 'static>(self, state: S) -> Self {
        self.state.set(state);
        self
    }
}

impl<A: Adapter> SocketIoBuilder<A> {
    /// Build a [`SocketIoLayer`] and a [`SocketIo`] instance that can be used as a [`tower_layer::Layer`].
    pub fn build_layer(mut self) -> (SocketIoLayer<A>, SocketIo<A>) {
        self.config.engine_config = self.engine_config_builder.build();

        let (layer, client) = SocketIoLayer::from_config(
            self.config,
            self.adapter_state,
            #[cfg(feature = "state")]
            self.state,
        );
        (layer, SocketIo(client))
    }

    /// Build a [`SocketIoService`] and a [`SocketIo`] instance that
    /// can be used as a [`hyper::service::Service`] or a [`tower_service::Service`].
    ///
    /// This service will be a _standalone_ service that return a 404 error for every non-socket.io request
    pub fn build_svc(mut self) -> (SocketIoService<NotFoundService, A>, SocketIo<A>) {
        self.config.engine_config = self.engine_config_builder.build();
        let (svc, client) = SocketIoService::with_config_inner(
            NotFoundService,
            self.config,
            self.adapter_state,
            #[cfg(feature = "state")]
            self.state,
        );
        (svc, SocketIo(client))
    }

    /// Build a [`SocketIoService`] and a [`SocketIo`] instance with an inner service that
    /// can be used as a [`hyper::service::Service`] or a [`tower_service::Service`].
    pub fn build_with_inner_svc<S: Clone>(
        mut self,
        svc: S,
    ) -> (SocketIoService<S, A>, SocketIo<A>) {
        self.config.engine_config = self.engine_config_builder.build();

        let (svc, client) = SocketIoService::with_config_inner(
            svc,
            self.config,
            self.adapter_state,
            #[cfg(feature = "state")]
            self.state,
        );
        (svc, SocketIo(client))
    }
}

impl Default for SocketIoBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// The [`SocketIo`] instance can be cheaply cloned and moved around everywhere in your program.
/// It can be used as the main handle to access the whole socket.io context.
///
/// You can also use it as an extractor for all your [`handlers`](crate::handler).
///
/// It is generic over the [`Adapter`] type. If you plan to use it with another adapter than the default,
/// make sure to have a handler that is [generic over the adapter type](crate#adapters).
pub struct SocketIo<A: Adapter = LocalAdapter>(Arc<Client<A>>);

impl SocketIo<LocalAdapter> {
    /// Create a new [`SocketIoBuilder`] with a default config
    #[inline(always)]
    pub fn builder() -> SocketIoBuilder {
        SocketIoBuilder::new()
    }

    /// Create a new [`SocketIoService`] and a [`SocketIo`] instance with a default config.
    /// This service will be a _standalone_ service that return a 404 error for every non-socket.io request.
    /// It can be used as a [`Service`](tower_service::Service) (see hyper example)
    #[inline(always)]
    pub fn new_svc() -> (SocketIoService<NotFoundService>, SocketIo) {
        Self::builder().build_svc()
    }

    /// Create a new [`SocketIoService`] and a [`SocketIo`] instance with a default config.
    /// It can be used as a [`Service`](tower_service::Service) with an inner service
    #[inline(always)]
    pub fn new_inner_svc<S: Clone>(svc: S) -> (SocketIoService<S>, SocketIo) {
        Self::builder().build_with_inner_svc(svc)
    }

    /// Build a [`SocketIoLayer`] and a [`SocketIo`] instance with a default config.
    /// It can be used as a tower [`Layer`](tower_layer::Layer) (see axum example)
    #[inline(always)]
    pub fn new_layer() -> (SocketIoLayer, SocketIo) {
        Self::builder().build_layer()
    }
}

impl<A: Adapter> SocketIo<A> {
    /// Return a reference to the [`SocketIoConfig`] used by this [`SocketIo`] instance
    #[inline]
    pub fn config(&self) -> &SocketIoConfig {
        &self.0.config
    }

    /// # Register a [`ConnectHandler`] for the given dynamic namespace.
    ///
    /// You can specify dynamic parts in the path by using the `{name}` syntax.
    /// Note that any static namespace will take precedence over a dynamic one.
    ///
    /// For more info about namespace routing, see the [matchit] router documentation.
    ///
    /// The dynamic namespace will create a child namespace for any path that matches the given pattern
    /// with the given handler.
    ///
    /// * See the [`connect`](crate::handler::connect) module doc for more details on connect handler.
    /// * See the [`extract`](crate::extract) module doc for more details on available extractors.
    ///
    /// ## Errors
    /// If the pattern is invalid, a [`NsInsertError`](crate::NsInsertError) will be returned.
    ///
    /// ## Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::SocketRef};
    /// let (_, io) = SocketIo::new_svc();
    /// io.dyn_ns("/client/{client_id}", async |socket: SocketRef| {
    ///     println!("Socket connected on dynamic namespace with namespace path: {}", socket.ns());
    /// }).unwrap();
    ///
    /// ```
    /// ```
    /// # use socketioxide::{SocketIo, extract::SocketRef};
    /// let (_, io) = SocketIo::new_svc();
    /// io.dyn_ns("/client/{*remaining_path}", async |socket: SocketRef| {
    ///     println!("Socket connected on dynamic namespace with namespace path: {}", socket.ns());
    /// }).unwrap();
    ///
    /// ```
    #[inline]
    pub fn dyn_ns<C, T>(
        &self,
        path: impl Into<String>,
        callback: C,
    ) -> Result<(), crate::NsInsertError>
    where
        C: ConnectHandler<A, T>,
        T: Send + Sync + 'static,
    {
        self.0.add_dyn_ns(path.into(), callback)
    }

    /// # Delete the namespace with the given path.
    ///
    /// This will disconnect all sockets connected to this
    /// namespace in a deferred way.
    ///
    /// # Panics
    /// If the v4 protocol (legacy) is enabled and the namespace to delete is the default namespace "/".
    /// For v4, the default namespace cannot be deleted.
    /// See [official doc](https://socket.io/docs/v3/namespaces/#main-namespace) for more informations.
    #[inline]
    pub fn delete_ns(&self, path: impl AsRef<str>) {
        self.0.delete_ns(path.as_ref());
    }

    /// # Gracefully close all the connections and drop every sockets
    ///
    /// Any `on_disconnect` handler will called with
    /// [`DisconnectReason::ClosingServer`](crate::socket::DisconnectReason::ClosingServer)
    #[inline]
    pub async fn close(&self) {
        self.0.close().await;
    }

    /// # Get all the namespaces to perform operations on.
    ///
    /// This will return a vector of [`BroadcastOperators`] for each namespace.
    ///
    /// # Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::SocketRef};
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/custom_ns", async |socket: SocketRef| {
    ///     println!("Socket connected on /custom_ns namespace with id: {}", socket.id);
    /// });
    ///
    /// // Later in your code you can get all the namespaces
    /// for ns in io.nsps() {
    ///     assert_eq!(ns.ns_path(), "/custom_ns");
    /// }
    /// ```
    #[inline]
    pub fn nsps(&self) -> Vec<BroadcastOperators<A>> {
        let parser = self.0.parser();
        self.0
            .nsps
            .read()
            .unwrap()
            .values()
            .map(|ns| BroadcastOperators::new(ns.clone(), parser).broadcast())
            .collect()
    }

    // Chaining operators fns

    /// # Select a specific namespace to perform operations on.
    ///
    /// Currently you cannot select a dynamic namespace with this method.
    ///
    /// # Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::SocketRef};
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("custom_ns", async |socket: SocketRef| {
    ///     println!("Socket connected on /custom_ns namespace with id: {}", socket.id);
    /// });
    ///
    /// // Later in your code you can select the custom_ns namespace
    /// // and show all sockets connected to it
    /// async fn test(io: SocketIo) {
    ///     let sockets = io.of("custom_ns").unwrap().sockets();
    ///     for socket in sockets {
    ///        println!("found socket on /custom_ns namespace with id: {}", socket.id);
    ///     }
    /// }
    /// ```
    #[inline]
    pub fn of(&self, path: impl AsRef<str>) -> Option<BroadcastOperators<A>> {
        self.get_op(path.as_ref())
    }

    /// _Alias for `io.of("/").unwrap().to()`_. If the **default namespace "/" is not found** this fn will panic!
    #[doc = include_str!("../docs/operators/to.md")]
    #[inline]
    pub fn to(&self, rooms: impl RoomParam) -> BroadcastOperators<A> {
        self.get_default_op().to(rooms)
    }

    /// _Alias for `io.of("/").unwrap().within()`_. If the **default namespace "/" is not found** this fn will panic!
    #[doc = include_str!("../docs/operators/within.md")]
    #[inline]
    pub fn within(&self, rooms: impl RoomParam) -> BroadcastOperators<A> {
        self.get_default_op().within(rooms)
    }

    /// _Alias for `io.of("/").unwrap().except()`_. If the **default namespace "/" is not found** this fn will panic!
    #[doc = include_str!("../docs/operators/except.md")]
    #[inline]
    pub fn except(&self, rooms: impl RoomParam) -> BroadcastOperators<A> {
        self.get_default_op().except(rooms)
    }

    /// _Alias for `io.of("/").unwrap().local()`_. If the **default namespace "/" is not found** this fn will panic!
    #[doc = include_str!("../docs/operators/local.md")]
    #[inline]
    pub fn local(&self) -> BroadcastOperators<A> {
        self.get_default_op().local()
    }

    /// _Alias for `io.of("/").unwrap().timeout()`_. If the **default namespace "/" is not found** this fn will panic!
    #[doc = include_str!("../docs/operators/timeout.md")]
    #[inline]
    pub fn timeout(&self, timeout: Duration) -> BroadcastOperators<A> {
        self.get_default_op().timeout(timeout)
    }

    /// _Alias for `io.of("/").unwrap().emit()`_. If the **default namespace "/" is not found** this fn will panic!
    #[doc = include_str!("../docs/operators/emit.md")]
    #[inline]
    pub async fn emit<T: ?Sized + Serialize>(
        &self,
        event: impl AsRef<str>,
        data: &T,
    ) -> Result<(), BroadcastError> {
        self.get_default_op().emit(event, data).await
    }

    /// _Alias for `io.of("/").unwrap().emit_with_ack()`_. If the **default namespace "/" is not found** this fn will panic!
    #[doc = include_str!("../docs/operators/emit_with_ack.md")]
    #[inline]
    pub async fn emit_with_ack<T: ?Sized + Serialize, V>(
        &self,
        event: impl AsRef<str>,
        data: &T,
    ) -> Result<AckStream<V, A>, EmitWithAckError> {
        self.get_default_op().emit_with_ack(event, data).await
    }

    /// _Alias for `io.of("/").unwrap().sockets()`_. If the **default namespace "/" is not found** this fn will panic!
    #[doc = include_str!("../docs/operators/sockets.md")]
    #[inline]
    pub fn sockets(&self) -> Vec<SocketRef<A>> {
        self.get_default_op().sockets()
    }

    /// _Alias for `io.of("/").unwrap().fetch_sockets()`_. If the **default namespace "/" is not found** this fn will panic!
    #[doc = include_str!("../docs/operators/fetch_sockets.md")]
    #[inline]
    pub async fn fetch_sockets(&self) -> Result<Vec<RemoteSocket<A>>, A::Error> {
        self.get_default_op().fetch_sockets().await
    }

    /// _Alias for `io.of("/").unwrap().disconnect()`_. If the **default namespace "/" is not found** this fn will panic!
    #[doc = include_str!("../docs/operators/disconnect.md")]
    #[inline]
    pub async fn disconnect(&self) -> Result<(), BroadcastError> {
        self.get_default_op().disconnect().await
    }

    /// _Alias for `io.of("/").unwrap().join()`_. If the **default namespace "/" is not found** this fn will panic!
    #[doc = include_str!("../docs/operators/join.md")]
    #[inline]
    pub async fn join(self, rooms: impl RoomParam) -> Result<(), A::Error> {
        self.get_default_op().join(rooms).await
    }

    /// _Alias for `io.of("/").unwrap().rooms()`_. If the **default namespace "/" is not found** this fn will panic!
    #[doc = include_str!("../docs/operators/rooms.md")]
    pub async fn rooms(&self) -> Result<Vec<Room>, A::Error> {
        self.get_default_op().rooms().await
    }

    /// _Alias for `io.of("/").unwrap().rooms()`_. If the **default namespace "/" is not found** this fn will panic!
    #[doc = include_str!("../docs/operators/leave.md")]
    #[inline]
    pub async fn leave(self, rooms: impl RoomParam) -> Result<(), A::Error> {
        self.get_default_op().leave(rooms).await
    }

    /// _Alias for `io.of("/").unwrap().get_socket()`_. If the **default namespace "/" is not found** this fn will panic!
    #[doc = include_str!("../docs/operators/get_socket.md")]
    #[inline]
    pub fn get_socket(&self, sid: Sid) -> Option<SocketRef<A>> {
        self.get_default_op().get_socket(sid)
    }

    /// _Alias for `io.of("/").unwrap().broadcast()`_. If the **default namespace "/" is not found** this fn will panic!
    #[doc = include_str!("../docs/operators/broadcast.md")]
    #[inline]
    pub fn broadcast(&self) -> BroadcastOperators<A> {
        self.get_default_op()
    }

    #[cfg(feature = "state")]
    pub(crate) fn get_state<T: Clone + 'static>(&self) -> Option<T> {
        self.0.state.try_get::<T>().cloned()
    }

    /// Returns a new operator on the given namespace
    #[inline(always)]
    fn get_op(&self, path: &str) -> Option<BroadcastOperators<A>> {
        let parser = self.config().parser;
        self.0
            .get_ns(path)
            .map(|ns| BroadcastOperators::new(ns, parser).broadcast())
    }

    /// Returns a new operator on the default namespace "/" (root namespace)
    ///
    /// # Panics
    ///
    /// If the **default namespace "/" is not found** this fn will panic!
    #[inline(always)]
    fn get_default_op(&self) -> BroadcastOperators<A> {
        self.get_op("/").expect("default namespace not found")
    }
}

// This private impl is used to ensure that the following methods
// are only available on a *defined* adapter.
#[allow(private_bounds)]
impl<A: DefinedAdapter + Adapter> SocketIo<A> {
    /// # Register a [`ConnectHandler`] for the given namespace
    ///
    /// * See the [`connect`](crate::handler::connect) module doc for more details on connect handler.
    /// * See the [`extract`](crate::extract) module doc for more details on available extractors.
    ///
    /// # Simple example with a sync closure:
    /// ```
    /// # use socketioxide::{SocketIo, extract::*};
    /// # use serde::{Serialize, Deserialize};
    /// #[derive(Debug, Serialize, Deserialize)]
    /// struct MyData {
    ///     name: String,
    ///     age: u8,
    /// }
    ///
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", async |socket: SocketRef| {
    ///     // Register a handler for the "test" event and extract the data as a `MyData` struct
    ///     // With the Data extractor, the handler is called only if the data can be deserialized as a `MyData` struct
    ///     // If you want to manage errors yourself you can use the TryData extractor
    ///     socket.on("test", async |socket: SocketRef, Data::<MyData>(data)| {
    ///         println!("Received a test message {:?}", data);
    ///         socket.emit("test-test", &MyData { name: "Test".to_string(), age: 8 }).ok(); // Emit a message to the client
    ///     });
    /// });
    ///
    /// ```
    ///
    /// # Example with a closure and an acknowledgement + binary data:
    /// ```
    /// # use socketioxide::{SocketIo, extract::*};
    /// # use serde_json::Value;
    /// # use serde::{Serialize, Deserialize};
    /// #[derive(Debug, Serialize, Deserialize)]
    /// struct MyData {
    ///     name: String,
    ///     age: u8,
    /// }
    ///
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", async |socket: SocketRef| {
    ///     // Register an async handler for the "test" event and extract the data as a `MyData` struct
    ///     // Extract the binary payload as a `Vec<Bytes>` with the Bin extractor.
    ///     // It should be the last extractor because it consumes the request
    ///     socket.on("test", async |socket: SocketRef, Data::<MyData>(data), ack: AckSender| {
    ///         println!("Received a test message {:?}", data);
    ///         tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    ///         ack.send(&data).ok(); // The data received is sent back to the client through the ack
    ///         socket.emit("test-test", &MyData { name: "Test".to_string(), age: 8 }).ok(); // Emit a message to the client
    ///     });
    /// });
    /// ```
    /// # Example with a closure and an authentication process:
    /// ```
    /// # use socketioxide::{SocketIo, extract::{SocketRef, Data}};
    /// # use serde::{Serialize, Deserialize};
    /// #[derive(Debug, Deserialize)]
    /// struct MyAuthData {
    ///     token: String,
    /// }
    /// #[derive(Debug, Serialize, Deserialize)]
    /// struct MyData {
    ///     name: String,
    ///     age: u8,
    /// }
    ///
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", async |socket: SocketRef, Data(auth): Data<MyAuthData>| {
    ///     if auth.token.is_empty() {
    ///         println!("Invalid token, disconnecting");
    ///         socket.disconnect().ok();
    ///         return;
    ///     }
    ///     socket.on("test", async |socket: SocketRef, Data::<MyData>(data)| {
    ///         println!("Received a test message {:?}", data);
    ///         socket.emit("test-test", &MyData { name: "Test".to_string(), age: 8 }).ok(); // Emit a message to the client
    ///     });
    /// });
    ///
    /// ```
    ///
    /// # With remote adapters, this method is only available on a defined adapter:
    /// ```compile_fail
    /// # use socketioxide::{SocketIo};
    /// // The SocketIo instance is generic over the adapter type.
    /// async fn test<A: Adapter>(io: SocketIo<A>) {
    ///     io.ns("/", async || ());
    /// }
    /// ```
    /// ```
    /// # use socketioxide::{SocketIo, adapter::LocalAdapter};
    /// // The SocketIo instance is not generic over the adapter type.
    /// async fn test(io: SocketIo<LocalAdapter>) {
    ///     io.ns("/", async || ());
    /// }
    /// async fn test_default_adapter(io: SocketIo) {
    ///     io.ns("/", async || ());
    /// }
    /// ```
    pub fn ns<C, T>(&self, path: impl Into<Cow<'static, str>>, callback: C) -> A::InitRes
    where
        C: ConnectHandler<A, T>,
        T: Send + Sync + 'static,
    {
        self.0.clone().add_ns(path.into(), callback)
    }
}

impl<A: Adapter> fmt::Debug for SocketIo<A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SocketIo").field("client", &self.0).finish()
    }
}
impl<A: Adapter> Clone for SocketIo<A> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<A: Adapter> From<Arc<Client<A>>> for SocketIo<A> {
    fn from(client: Arc<Client<A>>) -> Self {
        SocketIo(client)
    }
}

#[doc(hidden)]
#[cfg(feature = "__test_harness")]
impl<A: Adapter> SocketIo<A> {
    /// Create a dummy socket for testing purpose with a
    /// receiver to get the packets sent to the client
    pub async fn new_dummy_sock(
        &self,
        ns: &'static str,
        auth: impl serde::Serialize,
    ) -> (
        tokio::sync::mpsc::Sender<engineioxide::Packet>,
        tokio::sync::mpsc::Receiver<engineioxide::Packet>,
    ) {
        self.0.clone().new_dummy_sock(ns, auth).await
    }
}

#[cfg(test)]
mod tests {

    use crate::client::SocketData;

    use super::*;

    #[test]
    fn get_default_op() {
        let (_, io) = SocketIo::new_svc();
        io.ns("/", async || {});
        let _ = io.get_default_op();
    }

    #[test]
    #[should_panic(expected = "default namespace not found")]
    fn get_default_op_panic() {
        let (_, io) = SocketIo::new_svc();
        let _ = io.get_default_op();
    }

    #[test]
    fn get_op() {
        let (_, io) = SocketIo::new_svc();
        io.ns("test", async || {});
        assert!(io.get_op("test").is_some());
        assert!(io.get_op("test2").is_none());
    }

    #[tokio::test]
    async fn get_socket_by_sid() {
        use engineioxide::Socket;
        let sid = Sid::new();
        let (_, io) = SocketIo::new_svc();
        io.ns("/", async || {});
        let socket = Socket::<SocketData<LocalAdapter>>::new_dummy(sid, Box::new(|_, _| {}));
        socket.data.io.set(io.clone()).unwrap();
        io.0.get_ns("/")
            .unwrap()
            .connect(sid, socket, None)
            .await
            .ok();

        assert!(io.get_socket(sid).is_some());
        assert!(io.get_socket(Sid::new()).is_none());
    }
}
