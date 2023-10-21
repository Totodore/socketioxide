use std::{
    collections::HashMap,
    fmt::Debug,
    sync::Mutex,
    sync::{
        atomic::{AtomicI64, Ordering},
        Arc, RwLock,
    },
    time::Duration,
};

use engineioxide::{sid::Sid, socket::DisconnectReason as EIoDisconnectReason, ProtocolVersion};
use futures::{future::BoxFuture, Future};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use tokio::sync::oneshot;
use tracing::debug;

use crate::{
    adapter::{Adapter, Room},
    errors::{AckError, Error},
    extensions::Extensions,
    handler::{AckResponse, AckSender, BoxedMessageHandler, CallbackHandler},
    ns::Namespace,
    operators::{Operators, RoomParam},
    packet::{BinaryPacket, Packet, PacketData},
    SocketIoConfig,
};
use crate::{
    client::SocketData,
    errors::{AdapterError, SendError},
};

pub type DisconnectCallback<A> = Box<
    dyn FnOnce(Arc<Socket<A>>, DisconnectReason) -> BoxFuture<'static, ()> + Send + Sync + 'static,
>;

/// All the possible reasons for a [`Socket`] to be disconnected.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum DisconnectReason {
    /// The client gracefully closed the connection
    TransportClose,

    /// The client sent multiple polling requests at the same time (it is forbidden according to the engine.io protocol)
    MultipleHttpPollingError,

    /// The client sent a bad request / the packet could not be parsed correctly
    PacketParsingError,

    /// The connection was closed (example: the user has lost connection, or the network was changed from WiFi to 4G)
    TransportError,

    /// The client did not send a PONG packet in the [ping timeout](crate::SocketIoConfigBuilder) delay
    HeartbeatTimeout,

    /// The client has manually disconnected the socket using [`socket.disconnect()`](https://socket.io/fr/docs/v4/client-api/#socketdisconnect)
    ClientNSDisconnect,

    /// The socket was forcefully disconnected from the namespace with [`Socket::disconnect`]
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

/// A Socket represents a client connected to a namespace.
/// It is used to send and receive messages from the client, join and leave rooms, etc.
pub struct Socket<A: Adapter> {
    config: Arc<SocketIoConfig>,
    ns: Arc<Namespace<A>>,
    message_handlers: RwLock<HashMap<String, BoxedMessageHandler<A>>>,
    disconnect_handler: Mutex<Option<DisconnectCallback<A>>>,
    ack_message: Mutex<HashMap<i64, oneshot::Sender<AckResponse<Value>>>>,
    ack_counter: AtomicI64,
    pub id: Sid,
    pub extensions: Extensions,
    esocket: Arc<engineioxide::Socket<SocketData>>,
}

impl<A: Adapter> Socket<A> {
    pub(crate) fn new(
        ns: Arc<Namespace<A>>,
        esocket: Arc<engineioxide::Socket<SocketData>>,
        config: Arc<SocketIoConfig>,
    ) -> Self {
        let id = if esocket.protocol == ProtocolVersion::V3 {
            esocket.id
        } else {
            Sid::new()
        };
        Self {
            ns,
            message_handlers: RwLock::new(HashMap::new()),
            disconnect_handler: Mutex::new(None),
            ack_message: Mutex::new(HashMap::new()),
            ack_counter: AtomicI64::new(0),
            id,
            extensions: Extensions::new(),
            config,
            esocket,
        }
    }

    /// ### Register a message handler for the given event.
    ///
    /// The data parameter can be typed with anything that implement [serde::Deserialize](https://docs.rs/serde/latest/serde/)
    ///
    /// ### Acknowledgements
    /// The ack can be sent only once and take a `Serializable` value as parameter.
    ///
    /// For more info about ack see [socket.io documentation](https://socket.io/fr/docs/v4/emitting-events/#acknowledgements)
    ///
    /// If the client sent a normal message without expecting an ack, the ack callback will do nothing.
    ///
    /// #### Simple example with a closure:
    /// ```
    /// # use socketioxide::SocketIo;
    /// # use serde_json::Value;
    /// # use serde::{Serialize, Deserialize};
    ///
    /// #[derive(Debug, Serialize, Deserialize)]
    /// struct MyData {
    ///     name: String,
    ///     age: u8,
    /// }
    ///
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket, _: ()| async move {
    ///     socket.on("test", |socket, data: MyData, _, _| async move {
    ///         println!("Received a test message {:?}", data);
    ///         socket.emit("test-test", MyData { name: "Test".to_string(), age: 8 }).ok(); // Emit a message to the client
    ///     });
    /// });
    ///
    /// ```
    ///
    /// #### Example with a closure and an ackknowledgement + binary data:
    /// ```
    /// # use socketioxide::SocketIo;
    /// # use serde_json::Value;
    /// # use serde::{Serialize, Deserialize};
    ///
    /// #[derive(Debug, Serialize, Deserialize)]
    /// struct MyData {
    ///     name: String,
    ///     age: u8,
    /// }
    ///
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket, _: ()| async move {
    ///     socket.on("test", |socket, data: MyData, bin, ack| async move {
    ///         println!("Received a test message {:?}", data);
    ///         ack.bin(bin).send(data).ok(); // The data received is sent back to the client through the ack
    ///         socket.emit("test-test", MyData { name: "Test".to_string(), age: 8 }).ok(); // Emit a message to the client
    ///     });
    /// });
    /// ```
    pub fn on<C, F, V>(&self, event: impl Into<String>, callback: C)
    where
        C: Fn(Arc<Socket<A>>, V, Vec<Vec<u8>>, AckSender<A>) -> F + Send + Sync + 'static,
        F: Future<Output = ()> + Send + 'static,
        V: DeserializeOwned + Send + Sync + 'static,
    {
        let handler = Box::new(move |s, v, p, ack_fn| Box::pin(callback(s, v, p, ack_fn)) as _);
        self.message_handlers.write().unwrap().insert(
            event.into(),
            CallbackHandler::boxed_message_handler(handler),
        );
    }

    /// ## Register a disconnect handler.
    /// The callback will be called when the socket is disconnected from the server or the client or when the underlying connection crashes.
    /// A [`DisconnectReason`](crate::DisconnectReason) is passed to the callback to indicate the reason for the disconnection.
    /// ### Example
    /// ```
    /// # use socketioxide::SocketIo;
    /// # use serde_json::Value;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket, _: ()| async move {
    ///     socket.on("test", |socket, data: Value, bin, _| async move {
    ///         // Close the current socket
    ///         socket.disconnect().ok();
    ///     });
    ///     socket.on_disconnect(|socket, reason| async move {
    ///         println!("Socket {} on ns {} disconnected, reason: {:?}", socket.id, socket.ns(), reason);
    ///     });
    /// });
    pub fn on_disconnect<C, F>(&self, callback: C)
    where
        C: Fn(Arc<Socket<A>>, DisconnectReason) -> F + Send + Sync + 'static,
        F: Future<Output = ()> + Send + 'static,
    {
        let handler = Box::new(move |s, r| Box::pin(callback(s, r)) as _);
        *self.disconnect_handler.lock().unwrap() = Some(handler);
    }

    /// Emit a message to the client
    /// ##### Example
    /// ```
    /// # use socketioxide::SocketIo;
    /// # use serde_json::Value;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket, _: ()| async move {
    ///     socket.on("test", |socket, data: Value, bin, _| async move {
    ///         // Emit a test message to the client
    ///         socket.emit("test", data);
    ///     });
    /// });
    pub fn emit(
        &self,
        event: impl Into<String>,
        data: impl Serialize,
    ) -> Result<(), serde_json::Error> {
        let ns = self.ns.path.clone();
        let data = serde_json::to_value(data)?;
        if let Err(err) = self.send(Packet::event(ns, event.into(), data)) {
            debug!("sending error during emit message: {err:?}");
        }
        Ok(())
    }

    /// Emit a message to the client and wait for acknowledgement.
    ///
    /// The acknowledgement has a timeout specified in the config (5s by default) or with the `timeout()` operator.
    /// ##### Example
    /// ```
    /// # use socketioxide::SocketIo;
    /// # use serde_json::Value;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket, _: ()| async move {
    ///     socket.on("test", |socket, data: Value, bin, _| async move {
    ///         // Emit a test message and wait for an acknowledgement
    ///         match socket.emit_with_ack::<Value>("test", data).await {
    ///             Ok(ack) => println!("Ack received {:?}", ack),
    ///             Err(err) => println!("Ack error {:?}", err),
    ///         }
    ///    });
    /// });
    pub async fn emit_with_ack<V>(
        &self,
        event: impl Into<String>,
        data: impl Serialize,
    ) -> Result<AckResponse<V>, AckError>
    where
        V: DeserializeOwned + Send + Sync + 'static,
    {
        let ns = self.ns.path.clone();
        let data = serde_json::to_value(data)?;
        let packet = Packet::event(ns, event.into(), data);

        self.send_with_ack(packet, None).await
    }

    // Room actions

    /// Join the given rooms.
    pub fn join(&self, rooms: impl RoomParam) -> Result<(), A::Error> {
        self.ns.adapter.add_all(self.id, rooms)
    }

    /// Leave the given rooms.
    pub fn leave(&self, rooms: impl RoomParam) -> Result<(), A::Error> {
        self.ns.adapter.del(self.id, rooms)
    }

    /// Leave all rooms where the socket is connected.
    pub fn leave_all(&self) -> Result<(), A::Error> {
        self.ns.adapter.del_all(self.id)
    }

    /// Get all rooms where the socket is connected.
    pub fn rooms(&self) -> Result<Vec<Room>, A::Error> {
        self.ns.adapter.socket_rooms(self.id)
    }

    // Socket operators

    /// Select all clients in the given rooms except the current socket.
    ///
    /// If you want to include the current socket, use the `within()` operator.
    /// ##### Example
    /// ```
    /// # use socketioxide::SocketIo;
    /// # use serde_json::Value;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket, _: ()| async move {
    ///     socket.on("test", |socket, data: Value, _, _| async move {
    ///         let other_rooms = "room4".to_string();
    ///         // In room1, room2, room3 and room4 except the current
    ///         socket
    ///             .to("room1")
    ///             .to(["room2", "room3"])
    ///             .to(vec![other_rooms])
    ///             .emit("test", data);
    ///     });
    /// });
    pub fn to(&self, rooms: impl RoomParam) -> Operators<A> {
        Operators::new(self.ns.clone(), Some(self.id)).to(rooms)
    }

    /// Select all clients in the given rooms.
    ///
    /// It does include the current socket contrary to the `to()` operator.
    /// #### Example
    /// ```
    /// # use socketioxide::SocketIo;
    /// # use serde_json::Value;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket, _: ()| async move {
    ///     socket.on("test", |socket, data: Value, _, _| async move {
    ///         let other_rooms = "room4".to_string();
    ///         // In room1, room2, room3 and room4 including the current socket
    ///         socket
    ///             .within("room1")
    ///             .within(["room2", "room3"])
    ///             .within(vec![other_rooms])
    ///             .emit("test", data);
    ///     });
    /// });
    pub fn within(&self, rooms: impl RoomParam) -> Operators<A> {
        Operators::new(self.ns.clone(), Some(self.id)).within(rooms)
    }

    /// Filter out all clients selected with the previous operators which are in the given rooms.
    /// ##### Example
    /// ```
    /// # use socketioxide::SocketIo;
    /// # use serde_json::Value;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket, _: ()| async move {
    ///     socket.on("register1", |socket, data: Value, _, _| async move {
    ///         socket.join("room1");
    ///     });
    ///     socket.on("register2", |socket, data: Value, _, _| async move {
    ///         socket.join("room2");
    ///     });
    ///     socket.on("test", |socket, data: Value, _, _| async move {
    ///         // This message will be broadcast to all clients in the Namespace
    ///         // except for ones in room1 and the current socket
    ///         socket.broadcast().except("room1").emit("test", data);
    ///     });
    /// });
    pub fn except(&self, rooms: impl RoomParam) -> Operators<A> {
        Operators::new(self.ns.clone(), Some(self.id)).except(rooms)
    }

    /// Broadcast to all clients only connected on this node (when using multiple nodes).
    /// When using the default in-memory adapter, this operator is a no-op.
    /// ##### Example
    /// ```
    /// # use socketioxide::SocketIo;
    /// # use serde_json::Value;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket, _: ()| async move {
    ///     socket.on("test", |socket, data: Value, _, _| async move {
    ///         // This message will be broadcast to all clients in this namespace and connected on this node
    ///         socket.local().emit("test", data);
    ///     });
    /// });
    pub fn local(&self) -> Operators<A> {
        Operators::new(self.ns.clone(), Some(self.id)).local()
    }

    /// Set a custom timeout when sending a message with an acknowledgement.
    ///
    /// ##### Example
    /// ```
    /// # use socketioxide::SocketIo;
    /// # use serde_json::Value;
    /// # use futures::stream::StreamExt;
    /// # use std::time::Duration;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket, _: ()| async move {
    ///    socket.on("test", |socket, data: Value, bin, _| async move {
    ///       // Emit a test message in the room1 and room3 rooms, except for the room2 room with the binary payload received, wait for 5 seconds for an acknowledgement
    ///       socket.to("room1")
    ///             .to("room3")
    ///             .except("room2")
    ///             .bin(bin)
    ///             .timeout(Duration::from_secs(5))
    ///             .emit_with_ack::<Value>("message-back", data).unwrap().for_each(|ack| async move {
    ///                match ack {
    ///                    Ok(ack) => println!("Ack received {:?}", ack),
    ///                    Err(err) => println!("Ack error {:?}", err),
    ///                }
    ///             }).await;
    ///    });
    /// });
    ///
    pub fn timeout(&self, timeout: Duration) -> Operators<A> {
        Operators::new(self.ns.clone(), Some(self.id)).timeout(timeout)
    }

    /// Add a binary payload to the message.
    /// ##### Example
    /// ```
    /// # use socketioxide::SocketIo;
    /// # use serde_json::Value;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket, _: ()| async move {
    ///     socket.on("test", |socket, data: Value, bin, _| async move {
    ///         // This will send the binary payload received to all clients in this namespace with the test message
    ///         socket.bin(bin).emit("test", data);
    ///     });
    /// });
    pub fn bin(&self, binary: Vec<Vec<u8>>) -> Operators<A> {
        Operators::new(self.ns.clone(), Some(self.id)).bin(binary)
    }

    /// Broadcast to all clients without any filtering (except the current socket).
    /// ##### Example
    /// ```
    /// # use socketioxide::SocketIo;
    /// # use serde_json::Value;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket, _: ()| async move {
    ///     socket.on("test", |socket, data: Value, _, _| async move {
    ///         // This message will be broadcast to all clients in this namespace
    ///         socket.broadcast().emit("test", data);
    ///     });
    /// });
    pub fn broadcast(&self) -> Operators<A> {
        Operators::new(self.ns.clone(), Some(self.id)).broadcast()
    }

    /// Disconnect the socket from the current namespace,
    ///
    /// It will also call the disconnect handler if it is set.
    pub fn disconnect(self: Arc<Self>) -> Result<(), SendError> {
        self.send(Packet::disconnect(self.ns.path.clone()))?;
        self.close(DisconnectReason::ServerNSDisconnect)?;
        Ok(())
    }

    /// Close the engine.io connection if it is not already closed.
    /// Return a future that resolves when the underlying transport is closed.
    pub(crate) async fn close_underlying_transport(&self) {
        if !self.esocket.is_closed() {
            debug!("closing underlying transport for socket: {}", self.id);
            self.esocket.close(EIoDisconnectReason::ClosingServer);
        }
        self.esocket.closed().await;
    }

    /// Get the current namespace path.
    pub fn ns(&self) -> &String {
        &self.ns.path
    }

    pub(crate) fn send(&self, mut packet: Packet) -> Result<(), SendError> {
        let bin_payloads = match packet.inner {
            PacketData::BinaryEvent(_, ref mut bin, _) | PacketData::BinaryAck(ref mut bin, _) => {
                Some(std::mem::take(&mut bin.bin))
            }
            _ => None,
        };

        let msg = packet.try_into()?;
        self.esocket.emit(msg)?;
        if let Some(bin_payloads) = bin_payloads {
            for bin in bin_payloads {
                self.esocket.emit_binary(bin)?;
            }
        }

        Ok(())
    }

    pub(crate) async fn send_with_ack<V: DeserializeOwned>(
        &self,
        mut packet: Packet,
        timeout: Option<Duration>,
    ) -> Result<AckResponse<V>, AckError> {
        let (tx, rx) = oneshot::channel();
        let ack = self.ack_counter.fetch_add(1, Ordering::SeqCst) + 1;
        self.ack_message.lock().unwrap().insert(ack, tx);
        packet.inner.set_ack_id(ack);
        self.send(packet)?;
        let timeout = timeout.unwrap_or(self.config.ack_timeout);
        let v = tokio::time::timeout(timeout, rx).await??;
        Ok((serde_json::from_value(v.0)?, v.1))
    }

    /// Called when the socket is gracefully disconnected from the server or the client
    ///
    /// It maybe also close when the underlying transport is closed or failed.
    pub(crate) fn close(self: Arc<Self>, reason: DisconnectReason) -> Result<(), AdapterError> {
        if let Some(handler) = self.disconnect_handler.lock().unwrap().take() {
            tokio::spawn(handler(self.clone(), reason));
        }

        self.ns.remove_socket(self.id)?;
        Ok(())
    }

    // Receive data from client:
    pub(crate) fn recv(self: Arc<Self>, packet: PacketData) -> Result<(), Error> {
        match packet {
            PacketData::Event(e, data, ack) => self.recv_event(e, data, ack),
            PacketData::EventAck(data, ack_id) => self.recv_ack(data, ack_id),
            PacketData::BinaryEvent(e, packet, ack) => self.recv_bin_event(e, packet, ack),
            PacketData::BinaryAck(packet, ack) => self.recv_bin_ack(packet, ack),
            PacketData::Disconnect => self
                .close(DisconnectReason::ClientNSDisconnect)
                .map_err(Error::from),
            _ => unreachable!(),
        }
    }

    fn recv_event(self: Arc<Self>, e: String, data: Value, ack: Option<i64>) -> Result<(), Error> {
        if let Some(handler) = self.message_handlers.read().unwrap().get(&e) {
            handler.call(self.clone(), data, vec![], ack)?;
        }
        Ok(())
    }

    fn recv_bin_event(
        self: Arc<Self>,
        e: String,
        packet: BinaryPacket,
        ack: Option<i64>,
    ) -> Result<(), Error> {
        if let Some(handler) = self.message_handlers.read().unwrap().get(&e) {
            handler.call(self.clone(), packet.data, packet.bin, ack)?;
        }
        Ok(())
    }

    fn recv_ack(self: Arc<Self>, data: Value, ack: i64) -> Result<(), Error> {
        if let Some(tx) = self.ack_message.lock().unwrap().remove(&ack) {
            tx.send((data, vec![])).ok();
        }
        Ok(())
    }

    fn recv_bin_ack(self: Arc<Self>, packet: BinaryPacket, ack: i64) -> Result<(), Error> {
        if let Some(tx) = self.ack_message.lock().unwrap().remove(&ack) {
            tx.send((packet.data, packet.bin)).ok();
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

#[cfg(test)]
impl<A: Adapter> Socket<A> {
    pub fn new_dummy(sid: Sid, ns: Arc<Namespace<A>>) -> Socket<A> {
        let close_fn = Box::new(move |_, _| ());
        Socket {
            id: sid,
            ns,
            ack_counter: AtomicI64::new(0),
            ack_message: Mutex::new(HashMap::new()),
            message_handlers: RwLock::new(HashMap::new()),
            disconnect_handler: Mutex::new(None),
            config: Arc::new(SocketIoConfig::default()),
            extensions: Extensions::new(),
            esocket: engineioxide::Socket::new_dummy(close_fn).into(),
        }
    }
}
