//! A [`Socket`] represents a client connected to a namespace.
//! The socket struct itself should not be used directly, but through a [`SocketRef`](crate::extract::SocketRef).
use std::{
    borrow::Cow,
    collections::HashMap,
    fmt::{self, Debug},
    sync::{
        atomic::{AtomicBool, AtomicI64, Ordering},
        Arc, Mutex, RwLock,
    },
    time::Duration,
};

use engineioxide::socket::{DisconnectReason as EIoDisconnectReason, Permit};
use serde::Serialize;
use tokio::sync::{
    mpsc::error::TrySendError,
    oneshot::{self, Receiver},
};

#[cfg(feature = "extensions")]
use crate::extensions::Extensions;

use crate::{
    ack::{AckInnerStream, AckResult, AckStream},
    adapter::{Adapter, LocalAdapter},
    client::SocketData,
    errors::Error,
    handler::{
        BoxedDisconnectHandler, BoxedMessageHandler, DisconnectHandler, MakeErasedHandler,
        MessageHandler,
    },
    ns::Namespace,
    operators::{BroadcastOperators, ConfOperators},
    parser::Parser,
    AckError, SendError, SocketError, SocketIo,
};
use socketioxide_core::{
    adapter::{BroadcastOptions, RemoteSocketData, Room, RoomParam},
    errors::{AdapterError, BroadcastError},
    packet::{Packet, PacketData},
    parser::Parse,
    Value,
};

pub use engineioxide::sid::Sid;

/// All the possible reasons for a [`Socket`] to be disconnected from a namespace.
///
/// It can be used as an extractor in the [`on_disconnect`](crate::handler::disconnect) handler.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum DisconnectReason {
    /// The client gracefully closed the connection
    TransportClose,

    /// The client sent multiple polling requests at the same time (it is forbidden according to the engine.io protocol)
    MultipleHttpPollingError,

    /// The client sent a bad request / the packet could not be parsed correctly
    PacketParsingError,

    /// The connection was closed (example: the user has lost connection, or the network was changed from WiFi to 4G)
    TransportError,

    /// The client did not send a PONG packet in the `ping timeout` delay
    HeartbeatTimeout,

    /// The client has manually disconnected the socket using [`socket.disconnect()`](https://socket.io/fr/docs/v4/client-api/#socketdisconnect)
    ClientNSDisconnect,

    /// The socket was forcefully disconnected from the namespace with [`Socket::disconnect`] or with [`SocketIo::delete_ns`](crate::io::SocketIo::delete_ns)
    ServerNSDisconnect,

    /// The server is being closed
    ClosingServer,
}

impl std::fmt::Display for DisconnectReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use DisconnectReason::*;
        let str: &'static str = match self {
            TransportClose => "client gracefully closed the connection",
            MultipleHttpPollingError => "client sent multiple polling requests at the same time",
            PacketParsingError => "client sent a bad request / the packet could not be parsed",
            TransportError => "The connection was abruptly closed",
            HeartbeatTimeout => "client did not send a PONG packet in time",
            ClientNSDisconnect => "client has manually disconnected the socket from the namespace",
            ServerNSDisconnect => "socket was forcefully disconnected from the namespace",
            ClosingServer => "server is being closed",
        };
        f.write_str(str)
    }
}

impl From<EIoDisconnectReason> for DisconnectReason {
    fn from(reason: EIoDisconnectReason) -> Self {
        use DisconnectReason::*;
        match reason {
            EIoDisconnectReason::TransportClose => TransportClose,
            EIoDisconnectReason::TransportError => TransportError,
            EIoDisconnectReason::HeartbeatTimeout => HeartbeatTimeout,
            EIoDisconnectReason::MultipleHttpPollingError => MultipleHttpPollingError,
            EIoDisconnectReason::PacketParsingError => PacketParsingError,
            EIoDisconnectReason::ClosingServer => ClosingServer,
        }
    }
}

pub(crate) trait PermitExt<'a> {
    fn send(self, packet: Packet, parser: Parser);
    fn send_raw(self, value: Value);
}
impl<'a> PermitExt<'a> for Permit<'a> {
    fn send(self, packet: Packet, parser: Parser) {
        match parser.encode(packet) {
            Value::Str(msg, None) => self.emit(msg),
            Value::Str(msg, Some(bin_payloads)) => self.emit_many(msg, bin_payloads),
            Value::Bytes(bin) => self.emit_binary(bin),
        }
    }

    fn send_raw(self, value: Value) {
        match value {
            Value::Str(msg, None) => self.emit(msg),
            Value::Str(msg, Some(bin_payloads)) => self.emit_many(msg, bin_payloads),
            Value::Bytes(bin) => self.emit_binary(bin),
        }
    }
}

/// A RemoteSocket is a [`Socket`] that is remotely connected on another server.
/// It implements a subset of the [`Socket`] API.
#[derive(Clone)]
pub struct RemoteSocket<A> {
    adapter: Arc<A>,
    parser: Parser,
    data: RemoteSocketData,
}

impl<A> RemoteSocket<A> {
    pub(crate) fn new(data: RemoteSocketData, adapter: &Arc<A>, parser: Parser) -> Self {
        Self {
            data,
            adapter: adapter.clone(),
            parser,
        }
    }
    /// Consume the [`RemoteSocket`] and return its underlying data
    #[inline]
    pub fn into_data(self) -> RemoteSocketData {
        self.data
    }
    /// Get a ref to the underlying data of the socket
    #[inline]
    pub fn data(&self) -> &RemoteSocketData {
        &self.data
    }
}
impl<A: Adapter> RemoteSocket<A> {
    /// # Emit a message to a client that is remotely connected on another server.
    ///
    /// See [`Socket::emit`] for more info.
    pub async fn emit<T: ?Sized + Serialize>(
        &self,
        event: impl AsRef<str>,
        data: &T,
    ) -> Result<(), RemoteActionError> {
        let opts = self.get_opts();
        let data = self.parser.encode_value(data, Some(event.as_ref()))?;
        let packet = Packet::event(self.data.ns.clone(), data);
        self.adapter.broadcast(packet, opts).await?;
        Ok(())
    }

    /// # Emit a message to a client that is remotely connected on another server and wait for an acknowledgement.
    ///
    /// See [`Socket::emit_with_ack`] for more info.
    pub async fn emit_with_ack<T: ?Sized + Serialize, V: serde::de::DeserializeOwned>(
        &self,
        event: impl AsRef<str>,
        data: &T,
    ) -> Result<AckStream<V, A>, RemoteActionError> {
        let opts = self.get_opts();
        let data = self.parser.encode_value(data, Some(event.as_ref()))?;
        let packet = Packet::event(self.data.ns.clone(), data);
        let stream = self
            .adapter
            .broadcast_with_ack(packet, opts, None)
            .await
            .map_err(Into::<AdapterError>::into)?;
        Ok(AckStream::new(stream, self.parser))
    }

    /// # Get all room names this remote socket is connected to.
    ///
    /// See [`Socket::rooms`] for more info.
    #[inline]
    pub async fn rooms(&self) -> Result<Vec<Room>, A::Error> {
        self.adapter.rooms(self.get_opts()).await
    }

    /// # Add the remote socket to the specified room(s).
    ///
    /// See [`Socket::join`] for more info.
    #[inline]
    pub async fn join(&self, rooms: impl RoomParam) -> Result<(), A::Error> {
        self.adapter.add_sockets(self.get_opts(), rooms).await
    }

    /// # Remove the remote socket from the specified room(s).
    ///
    /// See [`Socket::leave`] for more info.
    #[inline]
    pub async fn leave(&self, rooms: impl RoomParam) -> Result<(), A::Error> {
        self.adapter.del_sockets(self.get_opts(), rooms).await
    }

    /// # Disconnect the remote socket from the current namespace,
    ///
    /// See [`Socket::disconnect`] for more info.
    #[inline]
    pub async fn disconnect(self) -> Result<(), RemoteActionError> {
        self.adapter.disconnect_socket(self.get_opts()).await?;
        Ok(())
    }

    #[inline(always)]
    fn get_opts(&self) -> BroadcastOptions {
        BroadcastOptions::new_remote(&self.data)
    }
}
impl<A> fmt::Debug for RemoteSocket<A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RemoteSocket")
            .field("id", &self.data.id)
            .field("server_id", &self.data.server_id)
            .field("ns", &self.data.ns)
            .finish()
    }
}

/// A error that can occur when emitting a message to a remote socket.
#[derive(Debug, thiserror::Error)]
pub enum RemoteActionError {
    /// The message data could not be encoded.
    #[error("cannot encode data: {0}")]
    Serialize(#[from] crate::parser::ParserError),
    /// The remote socket is, in fact, a local socket and we should not emit to it.
    #[error("cannot send the message to the local socket: {0}")]
    Socket(crate::SocketError),
    /// The message could not be sent to the remote server.
    #[error("cannot propagate the request to the server: {0}")]
    Adapter(#[from] AdapterError),
}
impl From<BroadcastError> for RemoteActionError {
    fn from(value: BroadcastError) -> Self {
        // This conversion assumes that we broadcast to a single (remote or not) socket.
        match value {
            BroadcastError::Socket(s) if !s.is_empty() => RemoteActionError::Socket(s[0].clone()),
            BroadcastError::Socket(_) => {
                panic!("BroadcastError with an empty socket vec is not permitted")
            }
            BroadcastError::Adapter(e) => e.into(),
            BroadcastError::Serialize(e) => e.into(),
        }
    }
}

/// A Socket represents a client connected to a namespace.
/// It is used to send and receive messages from the client, join and leave rooms, etc.
/// The socket struct itself should not be used directly, but through a [`SocketRef`](crate::extract::SocketRef).
pub struct Socket<A: Adapter = LocalAdapter> {
    pub(crate) ns: Arc<Namespace<A>>,
    message_handlers: RwLock<HashMap<Cow<'static, str>, BoxedMessageHandler<A>>>,
    disconnect_handler: Mutex<Option<BoxedDisconnectHandler<A>>>,
    ack_message: Mutex<HashMap<i64, oneshot::Sender<AckResult<Value>>>>,
    ack_counter: AtomicI64,
    connected: AtomicBool,
    pub(crate) parser: Parser,
    /// The socket id
    pub id: Sid,

    /// A type map of protocol extensions.
    /// It can be used to share data through the lifetime of the socket.
    ///
    /// **Note**: This is not the same data than the `extensions` field on the [`http::Request::extensions()`](http::Request) struct.
    /// If you want to extract extensions from the http request, you should use the [`HttpExtension`](crate::extract::HttpExtension) extractor.
    #[cfg_attr(docsrs, doc(cfg(feature = "extensions")))]
    #[cfg(feature = "extensions")]
    pub extensions: Extensions,
    esocket: Arc<engineioxide::Socket<SocketData<A>>>,
}

impl<A: Adapter> Socket<A> {
    pub(crate) fn new(
        sid: Sid,
        ns: Arc<Namespace<A>>,
        esocket: Arc<engineioxide::Socket<SocketData<A>>>,
        parser: Parser,
    ) -> Self {
        Self {
            ns,
            message_handlers: RwLock::new(HashMap::new()),
            disconnect_handler: Mutex::new(None),
            ack_message: Mutex::new(HashMap::new()),
            ack_counter: AtomicI64::new(0),
            connected: AtomicBool::new(false),
            parser,
            id: sid,
            #[cfg(feature = "extensions")]
            extensions: Extensions::new(),
            esocket,
        }
    }

    /// # Registers a [`MessageHandler`] for the given event.
    ///
    /// * See the [`message`](crate::handler::message) module doc for more details on message handler.
    /// * See the [`extract`](crate::extract) module doc for more details on available extractors.
    ///
    /// _It is recommended for code clarity to define your handler as top level function rather than closures._
    ///
    /// # Simple example with a sync closure and a sync fn:
    /// ```
    /// # use socketioxide::{SocketIo, extract::*};
    /// # use serde::{Serialize, Deserialize};
    /// #[derive(Debug, Serialize, Deserialize)]
    /// struct MyData {
    ///     name: String,
    ///     age: u8,
    /// }
    /// fn handler(socket: SocketRef, Data(data): Data::<MyData>) {
    ///     println!("Received a test message {:?}", data);
    ///     socket.emit("test-test", &MyData { name: "Test".to_string(), age: 8 }).ok(); // Emit a message to the client
    /// }
    ///
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///     // Register a handler for the "test" event and extract the data as a `MyData` struct
    ///     // With the Data extractor, the handler is called only if the data can be deserialized as a `MyData` struct
    ///     // If you want to manage errors yourself you can use the TryData extractor
    ///     socket.on("test", |socket: SocketRef, Data::<MyData>(data)| {
    ///         println!("Received a test message {:?}", data);
    ///         socket.emit("test-test", &MyData { name: "Test".to_string(), age: 8 }).ok(); // Emit a message to the client
    ///     });
    ///     // Do the same thing but with a sync function
    ///     socket.on("test_2", handler);
    /// });
    ///
    /// ```
    ///
    /// # Example with a closure and an fn with an acknowledgement + binary data:
    /// ```
    /// # use socketioxide::{SocketIo, extract::*};
    /// # use serde_json::Value;
    /// # use serde::{Serialize, Deserialize};
    /// #[derive(Debug, Serialize, Deserialize)]
    /// struct MyData {
    ///     name: String,
    ///     age: u8,
    /// }
    /// async fn handler(socket: SocketRef, Data(data): Data::<MyData>, ack: AckSender) {
    ///     println!("Received a test message {:?}", data);
    ///     tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    ///     ack.send(&data).ok(); // The data received is sent back to the client through the ack
    ///     socket.emit("test-test", &MyData { name: "Test".to_string(), age: 8 }).ok(); // Emit a message to the client
    /// }
    ///
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///     // Register an async handler for the "test" event and extract the data as a `MyData` struct
    ///     // Extract the binary payload as a `Vec<Bytes>` with the Bin extractor.
    ///     // It should be the last extractor because it consumes the request
    ///     socket.on("test", |socket: SocketRef, Data::<MyData>(data), ack: AckSender| async move {
    ///         println!("Received a test message {:?}", data);
    ///         tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    ///         ack.send(&data).ok(); // The data received is sent back to the client through the ack
    ///         socket.emit("test-test", &MyData { name: "Test".to_string(), age: 8 }).ok(); // Emit a message to the client
    ///     });
    ///     // Do the same thing but with a sync function
    ///     socket.on("test_2", handler);
    /// });
    /// ```
    pub fn on<H, T>(&self, event: impl Into<Cow<'static, str>>, handler: H)
    where
        H: MessageHandler<A, T>,
        T: Send + Sync + 'static,
    {
        self.message_handlers
            .write()
            .unwrap()
            .insert(event.into(), MakeErasedHandler::new_message_boxed(handler));
    }

    /// # Register a disconnect handler.
    /// You can register only one disconnect handler per socket. If you register multiple handlers, only the last one will be used.
    ///
    /// _It is recommended for code clarity to define your handler as top level function rather than closures._
    ///
    /// * See the [`disconnect`](crate::handler::disconnect) module doc for more details on disconnect handler.
    /// * See the [`extract`](crate::extract) module doc for more details on available extractors.
    ///
    /// The callback will be called when the socket is disconnected from the server or the client or when the underlying connection crashes.
    /// A [`DisconnectReason`] is passed to the callback to indicate the reason for the disconnection.
    ///
    /// # Example
    /// ```
    /// # use socketioxide::{SocketIo, socket::DisconnectReason, extract::*};
    /// # use serde_json::Value;
    /// # use std::sync::Arc;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///     socket.on("test", |socket: SocketRef| async move {
    ///         // Close the current socket
    ///         socket.disconnect().ok();
    ///     });
    ///     socket.on_disconnect(|socket: SocketRef, reason: DisconnectReason| async move {
    ///         println!("Socket {} on ns {} disconnected, reason: {:?}", socket.id, socket.ns(), reason);
    ///     });
    /// });
    pub fn on_disconnect<C, T>(&self, callback: C)
    where
        C: DisconnectHandler<A, T> + Send + Sync + 'static,
        T: Send + Sync + 'static,
    {
        let handler = MakeErasedHandler::new_disconnect_boxed(callback);
        self.disconnect_handler.lock().unwrap().replace(handler);
    }

    #[doc = include_str!("../docs/operators/emit.md")]
    pub fn emit<T: ?Sized + Serialize>(
        &self,
        event: impl AsRef<str>,
        data: &T,
    ) -> Result<(), SendError> {
        if !self.connected() {
            return Err(SendError::Socket(SocketError::Closed));
        }

        let permit = match self.reserve() {
            Ok(permit) => permit,
            Err(e) => {
                #[cfg(feature = "tracing")]
                tracing::debug!("sending error during emit message: {e:?}");
                return Err(SendError::Socket(e));
            }
        };

        let ns = self.ns.path.clone();
        let data = self.parser.encode_value(data, Some(event.as_ref()))?;

        permit.send(Packet::event(ns, data), self.parser);
        Ok(())
    }

    #[doc = include_str!("../docs/operators/emit_with_ack.md")]
    pub fn emit_with_ack<T: ?Sized + Serialize, V>(
        &self,
        event: impl AsRef<str>,
        data: &T,
    ) -> Result<AckStream<V>, SendError> {
        if !self.connected() {
            return Err(SendError::Socket(SocketError::Closed));
        }
        let permit = match self.reserve() {
            Ok(permit) => permit,
            Err(e) => {
                #[cfg(feature = "tracing")]
                tracing::debug!("sending error during emit message: {e:?}");
                return Err(SendError::Socket(e));
            }
        };
        let ns = self.ns.path.clone();
        let data = self.parser.encode_value(data, Some(event.as_ref()))?;
        let packet = Packet::event(ns, data);
        let rx = self.send_with_ack_permit(packet, permit);
        let stream = AckInnerStream::send(rx, self.get_io().config().ack_timeout, self.id);
        Ok(AckStream::<V>::new(stream, self.parser))
    }

    // Room actions

    /// # Add the current socket to the specified room(s).
    ///
    /// # Example
    /// ```rust
    /// # use socketioxide::{SocketIo, extract::*};
    /// async fn handler(socket: SocketRef) {
    ///     // Add all sockets that are in room1 and room3 to room4 and room5
    ///     socket.join(["room4", "room5"]);
    ///     // We should retrieve all the local sockets that are in room3 and room5
    ///     let sockets = socket.within("room4").within("room5").sockets();
    /// }
    ///
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |s: SocketRef| s.on("test", handler));
    /// ```
    pub fn join(&self, rooms: impl RoomParam) {
        self.ns.adapter.get_local().add_all(self.id, rooms)
    }

    /// # Remove the current socket from the specified room(s).
    ///
    /// # Example
    /// ```rust
    /// # use socketioxide::{SocketIo, extract::*};
    /// async fn handler(socket: SocketRef) {
    ///     // Remove all sockets that are in room1 and room3 from room4 and room5
    ///     socket.within("room1").within("room3").leave(["room4", "room5"]);
    /// }
    ///
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |s: SocketRef| s.on("test", handler));
    /// ```
    pub fn leave(&self, rooms: impl RoomParam) {
        self.ns.adapter.get_local().del(self.id, rooms)
    }

    /// # Remove the current socket from all its rooms.
    pub fn leave_all(&self) {
        self.ns.adapter.get_local().del_all(self.id);
    }

    /// # Get all room names this socket is connected to.
    ///
    /// # Example
    /// ```rust
    /// # use socketioxide::{SocketIo, extract::SocketRef};
    /// async fn handler(socket: SocketRef) {
    ///     println!("Socket connected to the / namespace with id: {}", socket.id);
    ///     socket.join(["room1", "room2"]);
    ///     let rooms = socket.rooms();
    ///     println!("All rooms in the / namespace: {:?}", rooms);
    /// }
    ///
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", handler);
    /// ```
    pub fn rooms(&self) -> Vec<Room> {
        self.ns.adapter.get_local().socket_rooms(self.id)
    }

    /// # Return true if the socket is connected to the namespace.
    ///
    /// A socket is considered connected when it has been successfully handshaked with the server
    /// and that all [connect middlewares](crate::handler::connect#middlewares) have been executed.
    pub fn connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    // Socket operators

    #[doc = include_str!("../docs/operators/to.md")]
    pub fn to(&self, rooms: impl RoomParam) -> BroadcastOperators<A> {
        BroadcastOperators::from_sock(self.ns.clone(), self.id, self.parser).to(rooms)
    }

    #[doc = include_str!("../docs/operators/within.md")]
    pub fn within(&self, rooms: impl RoomParam) -> BroadcastOperators<A> {
        BroadcastOperators::from_sock(self.ns.clone(), self.id, self.parser).within(rooms)
    }

    #[doc = include_str!("../docs/operators/except.md")]
    pub fn except(&self, rooms: impl RoomParam) -> BroadcastOperators<A> {
        BroadcastOperators::from_sock(self.ns.clone(), self.id, self.parser).except(rooms)
    }

    #[doc = include_str!("../docs/operators/local.md")]
    pub fn local(&self) -> BroadcastOperators<A> {
        BroadcastOperators::from_sock(self.ns.clone(), self.id, self.parser).local()
    }

    #[doc = include_str!("../docs/operators/timeout.md")]
    pub fn timeout(&self, timeout: Duration) -> ConfOperators<'_, A> {
        ConfOperators::new(self).timeout(timeout)
    }

    #[doc = include_str!("../docs/operators/broadcast.md")]
    pub fn broadcast(&self) -> BroadcastOperators<A> {
        BroadcastOperators::from_sock(self.ns.clone(), self.id, self.parser).broadcast()
    }

    /// # Get the [`SocketIo`] context related to this socket
    ///
    /// # Panics
    /// Because [`SocketData::io`] should be immediately set at the creation of the socket.
    /// this should never panic.
    pub(crate) fn get_io(&self) -> &SocketIo<A> {
        self.esocket.data.io.get().unwrap()
    }

    /// # Disconnect the socket from the current namespace,
    ///
    /// It will also call the disconnect handler if it is set with a [`DisconnectReason::ServerNSDisconnect`].
    pub fn disconnect(self: Arc<Self>) -> Result<(), SocketError> {
        let res = self.send(Packet::disconnect(self.ns.path.clone()));
        if let Err(SocketError::InternalChannelFull) = res {
            return Err(SocketError::InternalChannelFull);
        }
        self.close(DisconnectReason::ServerNSDisconnect);
        Ok(())
    }

    /// # Get the request info made by the client to connect.
    ///
    /// It might be used to retrieve the [`http::Extensions`]
    pub fn req_parts(&self) -> &http::request::Parts {
        &self.esocket.req_parts
    }

    /// # Get the [`TransportType`](crate::TransportType) used by the client to connect with this [`Socket`].
    ///
    /// It can also be accessed as an extractor
    /// # Example
    /// ```
    /// # use socketioxide::{SocketIo, TransportType, extract::*};
    ///
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef, transport: TransportType| {
    ///     assert_eq!(socket.transport_type(), transport);
    /// });
    pub fn transport_type(&self) -> crate::TransportType {
        self.esocket.transport_type()
    }

    /// Get the socket.io [`ProtocolVersion`](crate::ProtocolVersion) used by the client to connect with this [`Socket`].
    ///
    /// It can also be accessed as an extractor:
    /// # Example
    /// ```
    /// # use socketioxide::{SocketIo, ProtocolVersion, extract::*};
    ///
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef, v: ProtocolVersion| {
    ///     assert_eq!(socket.protocol(), v);
    /// });
    pub fn protocol(&self) -> crate::ProtocolVersion {
        self.esocket.protocol.into()
    }

    /// # Get the socket namespace path.
    #[inline]
    pub fn ns(&self) -> &str {
        &self.ns.path
    }

    /// # Close the engine.io connection if it is not already closed.
    ///
    /// Return a future that resolves when the underlying transport is closed.
    pub(crate) async fn close_underlying_transport(&self) {
        if !self.esocket.is_closed() {
            #[cfg(feature = "tracing")]
            tracing::debug!("closing underlying transport for socket: {}", self.id);
            self.esocket.close(EIoDisconnectReason::ClosingServer);
        }
        self.esocket.closed().await;
    }

    pub(crate) fn set_connected(&self, connected: bool) {
        self.connected.store(connected, Ordering::SeqCst);
    }

    pub(crate) fn reserve(&self) -> Result<Permit<'_>, SocketError> {
        match self.esocket.reserve() {
            Ok(permit) => Ok(permit),
            Err(TrySendError::Full(_)) => Err(SocketError::InternalChannelFull),
            Err(TrySendError::Closed(_)) => Err(SocketError::Closed),
        }
    }

    pub(crate) fn send(&self, packet: Packet) -> Result<(), SocketError> {
        let permit = self.reserve()?;
        permit.send(packet, self.parser);
        Ok(())
    }
    pub(crate) fn send_raw(&self, value: Value) -> Result<(), SocketError> {
        let permit = self.reserve()?;
        permit.send_raw(value);
        Ok(())
    }

    pub(crate) fn send_with_ack_permit(
        &self,
        mut packet: Packet,
        permit: Permit<'_>,
    ) -> Receiver<AckResult<Value>> {
        let (tx, rx) = oneshot::channel();

        let ack = self.ack_counter.fetch_add(1, Ordering::SeqCst) + 1;
        packet.inner.set_ack_id(ack);
        permit.send(packet, self.parser);
        self.ack_message.lock().unwrap().insert(ack, tx);
        rx
    }

    pub(crate) fn send_with_ack(&self, mut packet: Packet) -> Receiver<AckResult<Value>> {
        let (tx, rx) = oneshot::channel();

        let ack = self.ack_counter.fetch_add(1, Ordering::SeqCst) + 1;
        packet.inner.set_ack_id(ack);
        match self.send(packet) {
            Ok(()) => {
                self.ack_message.lock().unwrap().insert(ack, tx);
            }
            Err(e) => {
                tx.send(Err(AckError::Socket(e))).ok();
            }
        }
        rx
    }

    /// Called when the socket is gracefully disconnected from the server or the client
    ///
    /// It maybe also close when the underlying transport is closed or failed.
    pub(crate) fn close(self: Arc<Self>, reason: DisconnectReason) {
        self.set_connected(false);

        let handler = { self.disconnect_handler.lock().unwrap().take() };
        if let Some(handler) = handler {
            #[cfg(feature = "tracing")]
            tracing::trace!(?reason, ?self.id, "spawning disconnect handler");

            handler.call(self.clone(), reason);
        }

        self.ns.remove_socket(self.id);
    }

    /// Receive data from client
    pub(crate) fn recv(self: Arc<Self>, packet: PacketData) -> Result<(), Error> {
        match packet {
            PacketData::Event(d, ack) | PacketData::BinaryEvent(d, ack) => self.recv_event(d, ack),
            PacketData::EventAck(d, ack) | PacketData::BinaryAck(d, ack) => self.recv_ack(d, ack),
            PacketData::Disconnect => {
                self.close(DisconnectReason::ClientNSDisconnect);
                Ok(())
            }
            _ => unreachable!(),
        }
    }

    fn recv_event(self: Arc<Self>, data: Value, ack: Option<i64>) -> Result<(), Error> {
        let event = self.parser.read_event(&data).map_err(|_e| {
            #[cfg(feature = "tracing")]
            tracing::debug!(?_e, "failed to read event");
            Error::InvalidEventName
        })?;
        #[cfg(feature = "tracing")]
        tracing::debug!(?event, "reading");
        if let Some(handler) = self.message_handlers.read().unwrap().get(event) {
            handler.call(self.clone(), data, ack);
        }
        Ok(())
    }

    fn recv_ack(self: Arc<Self>, data: Value, ack: i64) -> Result<(), Error> {
        if let Some(tx) = self.ack_message.lock().unwrap().remove(&ack) {
            tx.send(Ok(data)).ok();
        }
        Ok(())
    }
}

impl<A: Adapter> Debug for Socket<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Socket")
            .field("ns", &self.ns())
            .field("ack_message", &self.ack_message)
            .field("ack_counter", &self.ack_counter)
            .field("sid", &self.id)
            .finish()
    }
}
impl<A: Adapter> PartialEq for Socket<A> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

#[doc(hidden)]
#[cfg(feature = "__test_harness")]
impl Socket<LocalAdapter> {
    /// Creates a dummy socket for testing purposes
    pub fn new_dummy(sid: Sid, ns: Arc<Namespace<LocalAdapter>>) -> Socket<LocalAdapter> {
        use crate::client::Client;
        use crate::io::SocketIoConfig;

        let close_fn = Box::new(move |_, _| ());
        let config = SocketIoConfig::default();
        let io = SocketIo::from(Arc::new(Client::new(
            config,
            (),
            #[cfg(feature = "state")]
            std::default::Default::default(),
        )));
        let s = Socket::new(
            sid,
            ns,
            engineioxide::Socket::new_dummy(sid, close_fn),
            Parser::default(),
        );
        s.esocket.data.io.set(io).unwrap();
        s.set_connected(true);
        s
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    async fn send_with_ack_error() {
        let sid = Sid::new();
        let ns = Namespace::<LocalAdapter>::new_dummy([sid]);
        let socket: Arc<Socket> = Socket::new_dummy(sid, ns).into();
        let parser = Parser::default();
        // Saturate the channel
        for _ in 0..1024 {
            socket
                .send(Packet::event(
                    "test",
                    parser.encode_value(&(), Some("test")).unwrap(),
                ))
                .unwrap();
        }

        let ack = socket.emit_with_ack::<_, ()>("test", &());
        assert!(matches!(
            ack,
            Err(SendError::Socket(SocketError::InternalChannelFull))
        ));
    }
}
