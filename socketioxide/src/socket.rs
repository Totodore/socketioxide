use std::{
    collections::HashMap,
    collections::VecDeque,
    fmt::Debug,
    sync::Mutex,
    sync::{
        atomic::{AtomicI64, Ordering},
        Arc, RwLock,
    },
    time::Duration,
};

use engineioxide::{
    sid_generator::Sid, socket::DisconnectReason as EIoDisconnectReason, SendPacket as EnginePacket,
};
use futures::{future::BoxFuture, Future};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::oneshot;
use tracing::debug;

use crate::errors::{AdapterError, SendError, TransportError};
use crate::{
    adapter::{Adapter, Room},
    errors::{AckError, Error},
    extensions::Extensions,
    handler::{AckResponse, AckSender, BoxedHandler, MessageHandler},
    handshake::Handshake,
    ns::Namespace,
    operators::{Operators, RoomParam},
    packet::{BinaryPacket, Packet, PacketData},
    SocketIoConfig,
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
        }
    }
}

/// A Socket represents a client connected to a namespace.
/// It is used to send and receive messages from the client, join and leave rooms, etc.
pub struct Socket<A: Adapter> {
    config: Arc<SocketIoConfig>,
    ns: Arc<Namespace<A>>,
    message_handlers: RwLock<HashMap<String, BoxedHandler<A>>>,
    disconnect_handler: Mutex<Option<DisconnectCallback<A>>>,
    ack_message: Mutex<HashMap<i64, oneshot::Sender<AckResponse<Value>>>>,
    ack_counter: AtomicI64,
    pub handshake: Handshake,
    pub sid: Sid,
    pub extensions: Extensions,
    sender: Mutex<PacketSender>,
}
/// PacketSender is internal struct, it is used to send/resend messages from the client.
struct PacketSender {
    tx: tokio::sync::mpsc::Sender<EnginePacket>, // Asynchronous sender for EnginePacket
    bin_payload_buffer: Option<VecDeque<EnginePacket>>, // Optional buffer for binary payloads
}

impl PacketSender {
    /// Creates a new PacketSender instance.
    ///
    /// # Arguments
    ///
    /// * `tx` - Asynchronous sender for EnginePacket.
    ///
    /// # Returns
    ///
    /// A new PacketSender instance.
    fn new(tx: tokio::sync::mpsc::Sender<EnginePacket>) -> Self {
        Self {
            tx,
            bin_payload_buffer: None,
        }
    }

    /// Sends a raw RetryablePacket, it's an internal function, should be used from RetryablePacket.retry() method.
    ///
    /// # Arguments
    ///
    /// * `packet` - The RetryablePacket to be sent.
    ///
    /// # Returns
    ///
    /// An Ok(()) if the send operation was successful, otherwise returns a TransportError.
    fn send_raw(&mut self, mut packet: RetryablePacket) -> Result<(), TransportError> {
        if let Err(err) = self.send_buffered_binaries() {
            match err {
                TransportError::SendFailedBinPayloads(None) => {
                    Err(TransportError::SendMainPacket(packet))
                }
                TransportError::SocketClosed => Err(TransportError::SocketClosed),
                _ => unreachable!(),
            }
        } else {
            let main_packet = packet.main_packet;
            match self.tx.try_send(main_packet) {
                Err(TrySendError::Full(main_packet)) => {
                    packet.main_packet = main_packet;
                    Err(TransportError::SendMainPacket(packet))
                }
                Err(TrySendError::Closed(_)) => Err(TransportError::SocketClosed),
                Ok(_) => {
                    self.bin_payload_buffer = Some(packet.attachments);
                    self.send_buffered_binaries()?;
                    Ok(())
                }
            }
        }
    }
    /// Sends a Packet.
    ///
    /// # Arguments
    ///
    /// * `packet` - The Packet to be sent.
    ///
    /// # Returns
    ///
    /// An Ok(()) if the send operation was successful, otherwise returns a SendError.
    fn send(&mut self, mut packet: Packet) -> Result<(), SendError> {
        if let Err(err) = self.send_buffered_binaries() {
            Err(err.add_main_packet(packet).into())
        } else {
            let bin_payloads = match packet.inner {
                PacketData::BinaryEvent(_, ref mut bin, _)
                | PacketData::BinaryAck(ref mut bin, _) => Some(
                    std::mem::take(&mut bin.bin)
                        .into_iter()
                        .map(EnginePacket::Binary)
                        .collect(),
                ),
                _ => None,
            };
            match self.tx.try_send(packet.try_into()?) {
                Err(TrySendError::Full(packet)) => {
                    let bin_payloads = bin_payloads.unwrap_or(VecDeque::new());
                    return Err(TransportError::SendMainPacket(RetryablePacket {
                        main_packet: packet,
                        attachments: bin_payloads,
                    })
                    .into());
                }
                Err(TrySendError::Closed(_)) => {
                    return Err(TransportError::SocketClosed.into());
                }
                _ => {}
            };
            self.bin_payload_buffer = bin_payloads;
            self.send_buffered_binaries()?;
            Ok(())
        }
    }

    /// Sends the binary payloads from the failed buffer appeared on the previous attempts of sending.
    ///
    /// # Returns
    ///
    /// An Ok(()) if the send operation was successful, otherwise returns a TransportError.
    fn send_buffered_binaries(&mut self) -> Result<(), TransportError> {
        while let Some(p) = self.bin_payload_buffer.as_mut().and_then(|p| p.pop_front()) {
            match self.tx.try_send(p) {
                Err(TrySendError::Full(p @ EnginePacket::Binary(_))) => {
                    self.bin_payload_buffer.as_mut().unwrap().push_front(p);
                    return Err(TransportError::SendFailedBinPayloads(None));
                }
                Err(TrySendError::Full(EnginePacket::Message(_) | EnginePacket::Close(_))) => {
                    unreachable!()
                }
                Err(TrySendError::Closed(_)) => return Err(TransportError::SocketClosed),
                Ok(()) => {}
            }
        }
        Ok(())
    }
}

/// The RetryablePacket struct represents a packet that can be retried for sending in case of failure,
/// It cannot be created from user space. There is only one way to get it:
/// it can only be returned from the socket send method.
#[derive(Debug)]
pub struct RetryablePacket {
    main_packet: EnginePacket,
    attachments: VecDeque<EnginePacket>,
}

impl RetryablePacket {
    /// This method attempts to send the packet represented by `self`
    /// If the sending operation fails, an error of type `TransportError` is returned.
    /// If the sending operation succeeds, `Ok(())` is returned.
    pub fn retry<A: Adapter>(self, socket: &Socket<A>) -> Result<(), TransportError> {
        socket.sender.lock().unwrap().send_raw(self)
    }
}

impl<A: Adapter> Socket<A> {
    pub(crate) fn new(
        sid: Sid,
        ns: Arc<Namespace<A>>,
        handshake: Handshake,
        tx: tokio::sync::mpsc::Sender<EnginePacket>,
        config: Arc<SocketIoConfig>,
    ) -> Self {
        Self {
            ns,
            message_handlers: RwLock::new(HashMap::new()),
            disconnect_handler: Mutex::new(None),
            ack_message: Mutex::new(HashMap::new()),
            ack_counter: AtomicI64::new(0),
            handshake,
            sid,
            extensions: Extensions::new(),
            config,
            sender: Mutex::new(PacketSender::new(tx)),
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
    /// # use socketioxide::Namespace;
    /// # use serde_json::Value;
    /// # use serde::{Serialize, Deserialize};
    ///
    /// #[derive(Debug, Serialize, Deserialize)]
    /// struct MyData {
    ///     name: String,
    ///     age: u8,
    /// }
    ///
    /// Namespace::builder().add("/", |socket| async move {
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
    /// # use socketioxide::Namespace;
    /// # use serde_json::Value;
    /// # use serde::{Serialize, Deserialize};
    ///
    /// #[derive(Debug, Serialize, Deserialize)]
    /// struct MyData {
    ///     name: String,
    ///     age: u8,
    /// }
    ///
    /// Namespace::builder().add("/", |socket| async move {
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
        self.message_handlers
            .write()
            .unwrap()
            .insert(event.into(), MessageHandler::boxed(handler));
    }

    /// ## Register a disconnect handler.
    /// The callback will be called when the socket is disconnected from the server or the client or when the underlying connection crashes.
    /// A [`DisconnectReason`](crate::DisconnectReason) is passed to the callback to indicate the reason for the disconnection.
    /// ### Example
    /// ```
    /// # use socketioxide::Namespace;
    /// # use serde_json::Value;
    /// Namespace::builder().add("/", |socket| async move {
    ///     socket.on("test", |socket, data: Value, bin, _| async move {
    ///         // Close the current socket
    ///         socket.disconnect().ok();
    ///     });
    ///     socket.on_disconnect(|socket, reason| async move {
    ///         println!("Socket {} on ns {} disconnected, reason: {:?}", socket.sid, socket.ns(), reason);
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
    /// # use socketioxide::Namespace;
    /// # use serde_json::Value;
    /// Namespace::builder().add("/", |socket| async move {
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

    /// Retries sending any failed binary payloads that are currently buffered.
    ///
    /// This method attempts to resend any binary payloads that have failed to be sent in previous attempts. It acquires
    /// a lock on the `PacketSender` associated with the `Socket` and calls the `send_buffered_binaries` method to
    /// retry sending the failed payloads. If the sending operation fails again, an error of type `TransportError` is
    /// returned. If the sending operation succeeds, `Ok(())` is returned.
    ///
    /// This method is useful in scenarios where binary payloads were not successfully sent due to temporary errors, such
    /// as a full buffer or a closed socket. By invoking this method, you can retry sending the failed payloads and ensure
    /// the data is transmitted successfully.
    pub fn retry_failed(&self) -> Result<(), TransportError> {
        self.sender.lock().unwrap().send_buffered_binaries()
    }

    /// Emit a message to the client and wait for acknowledgement.
    ///
    /// The acknowledgement has a timeout specified in the config (5s by default) or with the `timeout()` operator.
    /// ##### Example
    /// ```
    /// # use socketioxide::Namespace;
    /// # use serde_json::Value;
    /// Namespace::builder().add("/", |socket| async move {
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
        self.ns.adapter.add_all(self.sid, rooms)
    }

    /// Leave the given rooms.
    pub fn leave(&self, rooms: impl RoomParam) -> Result<(), A::Error> {
        self.ns.adapter.del(self.sid, rooms)
    }

    /// Leave all rooms where the socket is connected.
    pub fn leave_all(&self) -> Result<(), A::Error> {
        self.ns.adapter.del_all(self.sid)
    }

    /// Get all rooms where the socket is connected.
    pub fn rooms(&self) -> Result<Vec<Room>, A::Error> {
        self.ns.adapter.socket_rooms(self.sid)
    }

    // Socket operators

    /// Select all clients in the given rooms except the current socket.
    ///
    /// If you want to include the current socket, use the `within()` operator.
    /// ##### Example
    /// ```
    /// # use socketioxide::Namespace;
    /// # use serde_json::Value;
    /// Namespace::builder().add("/", |socket| async move {
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
        Operators::new(self.ns.clone(), self.sid).to(rooms)
    }

    /// Select all clients in the given rooms.
    ///
    /// It does include the current socket contrary to the `to()` operator.
    /// #### Example
    /// ```
    /// # use socketioxide::Namespace;
    /// # use serde_json::Value;
    /// Namespace::builder().add("/", |socket| async move {
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
        Operators::new(self.ns.clone(), self.sid).within(rooms)
    }

    /// Filter out all clients selected with the previous operators which are in the given rooms.
    /// ##### Example
    /// ```
    /// # use socketioxide::Namespace;
    /// # use serde_json::Value;
    /// Namespace::builder().add("/", |socket| async move {
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
        Operators::new(self.ns.clone(), self.sid).except(rooms)
    }

    /// Broadcast to all clients only connected on this node (when using multiple nodes).
    /// When using the default in-memory adapter, this operator is a no-op.
    /// ##### Example
    /// ```
    /// # use socketioxide::Namespace;
    /// # use serde_json::Value;
    /// Namespace::builder().add("/", |socket| async move {
    ///     socket.on("test", |socket, data: Value, _, _| async move {
    ///         // This message will be broadcast to all clients in this namespace and connected on this node
    ///         socket.local().emit("test", data);
    ///     });
    /// });
    pub fn local(&self) -> Operators<A> {
        Operators::new(self.ns.clone(), self.sid).local()
    }

    /// Set a custom timeout when sending a message with an acknowledgement.
    ///
    /// ##### Example
    /// ```
    /// # use socketioxide::Namespace;
    /// # use serde_json::Value;
    /// # use futures::stream::StreamExt;
    /// # use std::time::Duration;
    /// Namespace::builder().add("/", |socket| async move {
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
        Operators::new(self.ns.clone(), self.sid).timeout(timeout)
    }

    /// Add a binary payload to the message.
    /// ##### Example
    /// ```
    /// # use socketioxide::Namespace;
    /// # use serde_json::Value;
    /// Namespace::builder().add("/", |socket| async move {
    ///     socket.on("test", |socket, data: Value, bin, _| async move {
    ///         // This will send the binary payload received to all clients in this namespace with the test message
    ///         socket.bin(bin).emit("test", data);
    ///     });
    /// });
    pub fn bin(&self, binary: Vec<Vec<u8>>) -> Operators<A> {
        Operators::new(self.ns.clone(), self.sid).bin(binary)
    }

    /// Broadcast to all clients without any filtering (except the current socket).
    /// ##### Example
    /// ```
    /// # use socketioxide::Namespace;
    /// # use serde_json::Value;
    /// Namespace::builder().add("/", |socket| async move {
    ///     socket.on("test", |socket, data: Value, _, _| async move {
    ///         // This message will be broadcast to all clients in this namespace
    ///         socket.broadcast().emit("test", data);
    ///     });
    /// });
    pub fn broadcast(&self) -> Operators<A> {
        Operators::new(self.ns.clone(), self.sid).broadcast()
    }

    /// Disconnect the socket from the current namespace,
    ///
    /// It will also call the disconnect handler if it is set.
    pub fn disconnect(self: Arc<Self>) -> Result<(), SendError> {
        self.send(Packet::disconnect(self.ns.path.clone()))?;
        self.close(DisconnectReason::ServerNSDisconnect)?;
        Ok(())
    }

    /// Get the current namespace path.
    pub fn ns(&self) -> &String {
        &self.ns.path
    }

    pub(crate) fn send(&self, packet: Packet) -> Result<(), SendError> {
        self.sender.lock().unwrap().send(packet)
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
    /// It maybe also closed when the underlying transport is closed or failed.
    pub(crate) fn close(self: Arc<Self>, reason: DisconnectReason) -> Result<(), AdapterError> {
        if let Some(handler) = self.disconnect_handler.lock().unwrap().take() {
            tokio::spawn(handler(self.clone(), reason));
        }
        self.ns.remove_socket(self.sid)
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
            .field("handshake", &self.handshake)
            .field("sid", &self.sid)
            .finish()
    }
}

#[cfg(test)]
impl<A: Adapter> Socket<A> {
    pub fn new_dummy(sid: Sid, ns: Arc<Namespace<A>>) -> Socket<A> {
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        tokio::spawn(async move {
            while let Some(packet) = rx.recv().await {
                println!("Dummy socket received packet {:?}", packet);
            }
        });
        Socket::new(
            sid,
            ns,
            Handshake::new_dummy(),
            tx,
            Arc::new(SocketIoConfig::default()),
        )
    }
}
