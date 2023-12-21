use std::{borrow::Cow, sync::Arc, time::Duration};

use engineioxide::{
    config::{EngineIoConfig, EngineIoConfigBuilder},
    service::NotFoundService,
    sid::Sid,
    TransportType,
};
use futures::stream::BoxStream;
use serde::de::DeserializeOwned;

use crate::{
    adapter::{Adapter, LocalAdapter},
    client::Client,
    extract::SocketRef,
    handler::ConnectHandler,
    layer::SocketIoLayer,
    operators::{Operators, RoomParam},
    service::SocketIoService,
    socket::AckResponse,
    AckError, BroadcastError,
};

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
        }
    }
}

/// A builder to create a [`SocketIo`] instance.
/// It contains everything to configure the socket.io server with a [`SocketIoConfig`].
/// It can be used to build either a Tower [`Layer`](tower::layer::Layer) or a [`Service`](tower::Service).
pub struct SocketIoBuilder<A: Adapter = LocalAdapter> {
    config: SocketIoConfig,
    engine_config_builder: EngineIoConfigBuilder,
    adapter: std::marker::PhantomData<A>,
}

impl<A: Adapter> SocketIoBuilder<A> {
    /// Creates a new [`SocketIoBuilder`] with default config
    pub fn new() -> Self {
        Self {
            config: SocketIoConfig::default(),
            engine_config_builder: EngineIoConfigBuilder::new().req_path("/socket.io".to_string()),
            adapter: std::marker::PhantomData,
        }
    }

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

    /// Sets a custom [`Adapter`] for this [`SocketIoBuilder`]
    pub fn with_adapter<B: Adapter>(self) -> SocketIoBuilder<B> {
        SocketIoBuilder {
            config: self.config,
            engine_config_builder: self.engine_config_builder,
            adapter: std::marker::PhantomData,
        }
    }

    /// Add a custom global state for the [`SocketIo`] instance.
    /// This state will be accessible from every handler with the [`State`](crate::extract::State) extractor.
    /// You can set any number of states as long as they have different types.
    #[inline]
    #[cfg_attr(docsrs, doc(cfg(feature = "state")))]
    #[cfg(feature = "state")]
    pub fn with_state<S: Send + Sync + 'static>(self, state: S) -> Self {
        crate::state::set_state(state);
        self
    }

    /// Builds a [`SocketIoLayer`] and a [`SocketIo`] instance
    ///
    /// The layer can be used as a tower layer
    pub fn build_layer(mut self) -> (SocketIoLayer<A>, SocketIo<A>) {
        self.config.engine_config = self.engine_config_builder.build();

        let (layer, client) = SocketIoLayer::from_config(Arc::new(self.config));
        (layer, SocketIo(client))
    }

    /// Builds a [`SocketIoService`] and a [`SocketIo`] instance
    ///
    /// This service will be a _standalone_ service that return a 404 error for every non-socket.io request
    /// It can be used as a hyper service
    pub fn build_svc(mut self) -> (SocketIoService<NotFoundService>, SocketIo) {
        self.config.engine_config = self.engine_config_builder.build();

        let (svc, client) =
            SocketIoService::with_config_inner(NotFoundService, Arc::new(self.config));
        (svc, SocketIo(client))
    }

    /// Builds a [`SocketIoService`] and a [`SocketIo`] instance with an inner service
    ///
    /// It can be used as a hyper service
    pub fn build_with_inner_svc<S: Clone>(mut self, svc: S) -> (SocketIoService<S>, SocketIo) {
        self.config.engine_config = self.engine_config_builder.build();

        let (svc, client) = SocketIoService::with_config_inner(svc, Arc::new(self.config));
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
#[derive(Debug)]
pub struct SocketIo<A: Adapter = LocalAdapter>(Arc<Client<A>>);

impl SocketIo<LocalAdapter> {
    /// Creates a new [`SocketIoBuilder`] with a default config
    #[inline(always)]
    pub fn builder() -> SocketIoBuilder {
        SocketIoBuilder::new()
    }

    /// Creates a new [`SocketIoService`] and a [`SocketIo`] instance with a default config.
    /// This service will be a _standalone_ service that return a 404 error for every non-socket.io request.
    /// It can be used as a [`Service`](tower::Service) (see hyper example)
    #[inline(always)]
    pub fn new_svc() -> (SocketIoService<NotFoundService>, SocketIo) {
        Self::builder().build_svc()
    }

    /// Creates a new [`SocketIoService`] and a [`SocketIo`] instance with a default config.
    /// It can be used as a [`Service`](tower::Service) with an inner service
    #[inline(always)]
    pub fn new_inner_svc<S: Clone>(svc: S) -> (SocketIoService<S>, SocketIo) {
        Self::builder().build_with_inner_svc(svc)
    }

    /// Builds a [`SocketIoLayer`] and a [`SocketIo`] instance with a default config.
    /// It can be used as a tower [`Layer`](tower::layer::Layer) (see axum example)
    #[inline(always)]
    pub fn new_layer() -> (SocketIoLayer, SocketIo) {
        Self::builder().build_layer()
    }
}

impl<A: Adapter> SocketIo<A> {
    /// Returns a reference to the [`SocketIoConfig`] used by this [`SocketIo`] instance
    #[inline]
    pub fn config(&self) -> &SocketIoConfig {
        &self.0.config
    }

    /// ### Registers a [`ConnectHandler`] for the given namespace.
    ///
    /// * See the [`connect`](crate::handler::connect) module doc for more details on connect handler.
    /// * See the [`extract`](crate::extract) module doc for more details on available extractors.
    /// #### Simple example with a sync closure:
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
    /// io.ns("/", |socket: SocketRef| {
    ///     // Register a handler for the "test" event and extract the data as a `MyData` struct
    ///     // With the Data extractor, the handler is called only if the data can be deserialized as a `MyData` struct
    ///     // If you want to manage errors yourself you can use the TryData extractor
    ///     socket.on("test", |socket: SocketRef, Data::<MyData>(data)| {
    ///         println!("Received a test message {:?}", data);
    ///         socket.emit("test-test", MyData { name: "Test".to_string(), age: 8 }).ok(); // Emit a message to the client
    ///     });
    /// });
    ///
    /// ```
    ///
    /// #### Example with a closure and an acknowledgement + binary data:
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
    /// io.ns("/", |socket: SocketRef| {
    ///     // Register an async handler for the "test" event and extract the data as a `MyData` struct
    ///     // Extract the binary payload as a `Vec<Vec<u8>>` with the Bin extractor.
    ///     // It should be the last extractor because it consumes the request
    ///     socket.on("test", |socket: SocketRef, Data::<MyData>(data), ack: AckSender, Bin(bin)| async move {
    ///         println!("Received a test message {:?}", data);
    ///         tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    ///         ack.bin(bin).send(data).ok(); // The data received is sent back to the client through the ack
    ///         socket.emit("test-test", MyData { name: "Test".to_string(), age: 8 }).ok(); // Emit a message to the client
    ///     });
    /// });
    /// ```
    /// #### Simple example with a closure:
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
    /// io.ns("/", |socket: SocketRef, Data(auth): Data<MyAuthData>| {
    ///     if auth.token.is_empty() {
    ///         println!("Invalid token, disconnecting");
    ///         socket.disconnect().ok();
    ///         return;
    ///     }
    ///     socket.on("test", |socket: SocketRef, Data::<MyData>(data)| async move {
    ///         println!("Received a test message {:?}", data);
    ///         socket.emit("test-test", MyData { name: "Test".to_string(), age: 8 }).ok(); // Emit a message to the client
    ///     });
    /// });
    ///
    /// ```
    #[inline]
    pub fn ns<C, T>(&self, path: impl Into<Cow<'static, str>>, callback: C)
    where
        C: ConnectHandler<A, T>,
        T: Send + Sync + 'static,
    {
        self.0.add_ns(path.into(), callback);
    }

    /// Deletes the namespace with the given path
    #[inline]
    pub fn delete_ns<'a>(&self, path: impl Into<&'a str>) {
        self.0.delete_ns(path.into());
    }

    /// Gracefully closes all the connections and drops every sockets
    ///
    /// Any `on_disconnect` handler will called with [`DisconnectReason::ClosingServer`](crate::socket::DisconnectReason::ClosingServer)
    #[inline]
    pub async fn close(&self) {
        self.0.close().await;
    }

    // Chaining operators fns

    /// Selects a specific namespace to perform operations on
    ///
    /// ## Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::SocketRef};
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("custom_ns", |socket: SocketRef| {
    ///     println!("Socket connected on /custom_ns namespace with id: {}", socket.id);
    /// });
    ///
    /// // Later in your code you can select the custom_ns namespace
    /// // and show all sockets connected to it
    /// let sockets = io.of("custom_ns").unwrap().sockets().unwrap();
    /// for socket in sockets {
    ///    println!("found socket on /custom_ns namespace with id: {}", socket.id);
    /// }
    #[inline]
    pub fn of<'a>(&self, path: impl Into<&'a str>) -> Option<Operators<A>> {
        self.get_op(path.into())
    }

    /// Selects all sockets in the given rooms on the root namespace.
    ///
    /// Alias for `io.of("/").unwrap().to(rooms)`
    ///
    /// ## Panics
    /// If the **default namespace "/" is not found** this fn will panic!
    ///
    /// ## Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::SocketRef};
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///     println!("Socket connected on / namespace with id: {}", socket.id);
    /// });
    ///
    /// // Later in your code you can select all sockets in the room "room1"
    /// // and for example show all sockets connected to it
    /// let sockets = io.to("room1").sockets().unwrap();
    /// for socket in sockets {
    ///   println!("found socket on / ns in room1 with id: {}", socket.id);
    /// }
    #[inline]
    pub fn to(&self, rooms: impl RoomParam) -> Operators<A> {
        self.get_default_op().to(rooms)
    }

    /// Selects all sockets in the given rooms on the root namespace.
    ///
    /// Alias for :
    /// * `io.of("/").unwrap().within(rooms)`
    /// * `io.to(rooms)`
    ///
    /// ## Panics
    /// If the **default namespace "/" is not found** this fn will panic!
    ///
    /// ## Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::SocketRef};
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///     println!("Socket connected on / namespace with id: {}", socket.id);
    /// });
    ///
    /// // Later in your code you can select all sockets in the room "room1"
    /// // and for example show all sockets connected to it
    /// let sockets = io.within("room1").sockets().unwrap();
    /// for socket in sockets {
    ///   println!("found socket on / ns in room1 with id: {}", socket.id);
    /// }
    #[inline]
    pub fn within(&self, rooms: impl RoomParam) -> Operators<A> {
        self.get_default_op().within(rooms)
    }

    /// Filters out all sockets selected with the previous operators which are in the given rooms.
    ///
    /// Alias for `io.of("/").unwrap().except(rooms)`
    ///
    /// ## Panics
    /// If the **default namespace "/" is not found** this fn will panic!
    ///
    /// ## Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::SocketRef};
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///     println!("Socket connected on / namespace with id: {}", socket.id);
    ///     socket.on("register1", |socket: SocketRef| {
    ///         socket.join("room1");
    ///     });
    ///     socket.on("register2", |socket: SocketRef| {
    ///         socket.join("room2");
    ///     });
    /// });
    ///
    ///
    /// // Later in your code you can select all sockets in the root namespace that are not in the room1
    /// // and for example show all sockets connected to it
    /// let sockets = io.except("room1").sockets().unwrap();
    /// for socket in sockets {
    ///   println!("found socket on / ns in room1 with id: {}", socket.id);
    /// }
    #[inline]
    pub fn except(&self, rooms: impl RoomParam) -> Operators<A> {
        self.get_default_op().except(rooms)
    }

    /// Broadcasts to all sockets only connected on this node (when using multiple nodes).
    /// When using the default in-memory adapter, this operator is a no-op.
    ///
    /// Alias for `io.of("/").unwrap().local()`
    ///
    /// ## Panics
    /// If the **default namespace "/" is not found** this fn will panic!
    ///
    /// ## Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::SocketRef};
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///     println!("Socket connected on / namespace with id: {}", socket.id);
    /// });
    ///
    /// // Later in your code you can select all sockets in the local node and on the root namespace
    /// // and for example show all sockets connected to it
    /// let sockets = io.local().sockets().unwrap();
    /// for socket in sockets {
    ///   println!("found socket on / ns in room1 with id: {}", socket.id);
    /// }
    #[inline]
    pub fn local(&self) -> Operators<A> {
        self.get_default_op().local()
    }

    /// Sets a custom timeout when sending a message with an acknowledgement.
    ///
    /// Alias for `io.of("/").unwrap().timeout(duration)`
    ///
    /// ## Panics
    /// If the **default namespace "/" is not found** this fn will panic!
    ///
    /// ### Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::SocketRef};
    /// # use futures::stream::StreamExt;
    /// # use std::time::Duration;
    /// # use serde_json::Value;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///     println!("Socket connected on / namespace with id: {}", socket.id);
    /// });
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

    /// Adds a binary payload to the message.
    ///
    /// Alias for `io.of("/").unwrap().bin(binary_payload)`
    ///
    /// ## Panics
    /// If the **default namespace "/" is not found** this fn will panic!
    ///
    /// ## Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::SocketRef};
    /// # use serde_json::Value;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///     println!("Socket connected on / namespace with id: {}", socket.id);
    /// });
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

    /// Emits a message to all sockets selected with the previous operators.
    ///
    /// Alias for `io.of("/").unwrap().emit(event, data)`
    ///
    /// ## Panics
    /// If the **default namespace "/" is not found** this fn will panic!
    ///
    /// ## Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::SocketRef};
    /// # use serde_json::Value;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///     println!("Socket connected on / namespace with id: {}", socket.id);
    /// });
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
        event: impl Into<Cow<'static, str>>,
        data: impl serde::Serialize,
    ) -> Result<(), BroadcastError> {
        self.get_default_op().emit(event, data)
    }

    /// Emits a message to all sockets selected with the previous operators and return a stream of acknowledgements.
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
    /// # use socketioxide::{SocketIo, extract::SocketRef};
    /// # use futures::stream::StreamExt;
    /// # use serde_json::Value;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///     println!("Socket connected on / namespace with id: {}", socket.id);
    /// });
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
        event: impl Into<Cow<'static, str>>,
        data: impl serde::Serialize,
    ) -> Result<BoxStream<'static, Result<AckResponse<V>, AckError>>, BroadcastError> {
        self.get_default_op().emit_with_ack(event, data)
    }

    /// Gets all sockets selected with the previous operators.
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
    /// # use socketioxide::{SocketIo, extract::SocketRef};
    /// # use serde_json::Value;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///     println!("Socket connected on / namespace with id: {}", socket.id);
    /// });
    ///
    /// // Later in your code you can select all sockets in the room "room1"
    /// // and for example show all sockets connected to it
    /// let sockets = io.within("room1").sockets().unwrap();
    /// for socket in sockets {
    ///   println!("found socket on / ns in room1 with id: {}", socket.id);
    /// }
    #[inline]
    pub fn sockets(&self) -> Result<Vec<SocketRef<A>>, A::Error> {
        self.get_default_op().sockets()
    }

    /// Disconnects all sockets selected with the previous operators.
    ///
    /// Alias for `io.of("/").unwrap().disconnect()`
    ///
    /// ## Panics
    /// If the **default namespace "/" is not found** this fn will panic!
    ///
    /// ## Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::SocketRef};
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///     println!("Socket connected on / namespace with id: {}", socket.id);
    /// });
    ///
    /// // Later in your code you can disconnect all sockets in the root namespace
    /// io.disconnect();
    #[inline]
    pub fn disconnect(&self) -> Result<(), BroadcastError> {
        self.get_default_op().disconnect()
    }

    /// Makes all sockets selected with the previous operators join the given room(s).
    ///
    /// Alias for `io.of("/").unwrap().join(rooms)`
    ///
    /// ## Panics
    /// If the **default namespace "/" is not found** this fn will panic!
    ///
    /// ### Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::SocketRef};
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///     println!("Socket connected on / namespace with id: {}", socket.id);
    /// });
    ///
    /// // Later in your code you can for example add all sockets on the root namespace to the room1 and room3
    /// io.join(["room1", "room3"]).unwrap();
    #[inline]
    pub fn join(self, rooms: impl RoomParam) -> Result<(), A::Error> {
        self.get_default_op().join(rooms)
    }

    /// Makes all sockets selected with the previous operators leave the given room(s).
    ///
    /// Alias for `io.of("/").unwrap().join(rooms)`
    ///
    /// ## Panics
    /// If the **default namespace "/" is not found** this fn will panic!
    ///
    /// ### Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::SocketRef};
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///     println!("Socket connected on / namespace with id: {}", socket.id);
    /// });
    ///
    /// // Later in your code you can for example remove all sockets on the root namespace from the room1 and room3
    /// io.leave(["room1", "room3"]).unwrap();
    #[inline]
    pub fn leave(self, rooms: impl RoomParam) -> Result<(), A::Error> {
        self.get_default_op().leave(rooms)
    }

    /// Gets a [`SocketRef`] by the specified [`Sid`].
    #[inline]
    pub fn get_socket(&self, sid: Sid) -> Option<SocketRef<A>> {
        self.get_default_op().get_socket(sid)
    }

    /// Returns a new operator on the given namespace
    #[inline(always)]
    fn get_op(&self, path: &str) -> Option<Operators<A>> {
        self.0
            .get_ns(path)
            .map(|ns| Operators::new(ns, None).broadcast())
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

    #[test]
    fn get_default_op() {
        let (_, io) = SocketIo::builder().build_svc();
        io.ns("/", || {});
        let _ = io.get_default_op();
    }

    #[test]
    #[should_panic(expected = "default namespace not found")]
    fn get_default_op_panic() {
        let (_, io) = SocketIo::builder().build_svc();
        let _ = io.get_default_op();
    }

    #[test]
    fn get_op() {
        let (_, io) = SocketIo::builder().build_svc();
        io.ns("test", || {});
        assert!(io.get_op("test").is_some());
        assert!(io.get_op("test2").is_none());
    }

    #[test]
    fn get_socket_by_sid() {
        use engineioxide::Socket;
        let sid = Sid::new();
        let (_, io) = SocketIo::builder().build_svc();
        io.ns("/", || {});

        let socket = Socket::new_dummy(sid, Box::new(|_, _| {})).into();
        let config = SocketIoConfig::default().into();
        io.0.get_ns("/")
            .unwrap()
            .connect(sid, socket, None, config)
            .unwrap();

        assert!(io.get_socket(sid).is_some());
        assert!(io.get_socket(Sid::new()).is_none());
    }

    #[test]
    fn every_op_should_be_broadcast() {
        let (_, io) = SocketIo::builder().build_svc();
        io.ns("/", || {});
        assert!(io.get_default_op().is_broadcast());
        assert!(io.to("room1").is_broadcast());
        assert!(io.within("room1").is_broadcast());
        assert!(io.except("room1").is_broadcast());
        assert!(io.local().is_broadcast());
        assert!(io.timeout(Duration::from_secs(5)).is_broadcast());
        assert!(io.bin(vec![vec![1, 2, 3, 4]]).is_broadcast());
        assert!(io.of("/").unwrap().is_broadcast());
    }
}
