use std::{
    borrow::Cow,
    collections::HashMap,
    marker::PhantomData,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{self, AtomicI64},
    },
    time::Duration,
};

use pin_project_lite::pin_project;
use serde::{Serialize, de};
use socketioxide_core::{
    Sid, Str, Value,
    packet::{Packet, PacketData},
    parser::Parse,
};
use tokio::{
    sync::{mpsc, oneshot},
    time,
};

use crate::{
    errors::{AckError, SendError},
    handler::{
        BoxedDisconnectHandler, BoxedMessageHandler, MakeErasedHandler,
        disconnect::DisconnectHandler, message::MessageHandler,
    },
    parser::Parser,
};

/// All the possible reasons for a [`Socket`] to be disconnected from a namespace.
///
/// It can be used as an extractor in the [`on_disconnect`](crate::handler::disconnect) handler.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum DisconnectReason {
    IoServerDisconnect,
    IoClientDisconnect,
    PingTimeout,
    TransportClose,
    TransportError,
}

type AckResult<T> = Result<T, AckError>;

pin_project! {
    /// A [`Future`] of ack data received from the server.
    ///
    /// It will yield the ack response received from the client.
    ///
    /// If the client didn't respond before the timeout, the [`AckStream`] will yield
    /// an [`AckError::Timeout`]. If the data sent by the client is not deserializable as `T`,
    /// an [`AckError::Decode`] will be yielded.
    ///
    /// An [`AckStream`] can be created from:
    /// * The [`SocketRef::emit_with_ack`] method, in this case there will be only one ack response.
    /// * The [`Operator::emit_with_ack`] method, in this case there will be as many ack response
    /// as there are selected sockets.
    ///
    /// [`SocketRef::emit_with_ack`]: crate::extract::SocketRef#method.emit_with_ack
    /// [`Operator::emit_with_ack`]: crate::operators::Operators#method.emit_with_ack
    ///
    /// # Example
    /// ```rust
    /// # use socketioxide::extract::SocketRef;
    /// # use socketioxide::ack::AckStream;
    /// # use futures_util::StreamExt;
    /// # use socketioxide::SocketIo;
    /// let (svc, io) = SocketIo::new_svc();
    /// io.ns("/test", async |socket: SocketRef| {
    ///     // We wait for the acknowledgement of the first emit (only one in this case)
    ///     let ack = socket.emit_with_ack::<_, String>("test", "test").unwrap().await;
    ///     println!("Ack: {:?}", ack);
    ///
    ///     // We apply the `for_each` StreamExt fn to the AckStream
    ///     let res = socket.emit_with_ack::<_, String>("test", "test").await.unwrap().await;
    /// });
    /// ```
    #[must_use = "futures and streams do nothing unless you `.await` or poll them"]
    pub struct AckFuture<T> {
        #[pin]
        inner: time::Timeout<oneshot::Receiver<AckResult<Value>>>,
        parser: Parser,
        _phantom: PhantomData<fn() -> T>,
    }
}

impl<T> AckFuture<T> {
    fn new(parser: Parser, timeout: Duration) -> (Self, oneshot::Sender<AckResult<Value>>) {
        let (tx, rx) = oneshot::channel();

        let ack_fut = Self {
            inner: time::timeout(timeout, rx),
            parser,
            _phantom: PhantomData,
        };

        (ack_fut, tx)
    }
}

impl<T: de::DeserializeOwned> Future for AckFuture<T> {
    type Output = Result<T, AckError>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let mut v = std::task::ready!(self.as_mut().project().inner.poll(cx))???;
        let v = self.parser.decode_value(&mut v, false)?;
        std::task::Poll::Ready(Ok(v))
    }
}

pub struct Socket {
    tx: mpsc::Sender<Packet>,

    message_handlers: RwLock<HashMap<Cow<'static, str>, BoxedMessageHandler>>,
    fallback_message_handler: RwLock<Option<BoxedMessageHandler>>,
    // shared so it can be called outside of the lock: unlike the other ones it
    // is called again after every reconnection.
    disconnect_handler: Mutex<Option<BoxedDisconnectHandler>>,

    ack_message: Mutex<HashMap<i64, oneshot::Sender<AckResult<Value>>>>,
    ack_counter: AtomicI64,

    pub(crate) parser: Parser,
    pub ns: Str,
    /// The id the server gave to this socket on its namespace, `None` until it
    /// is connected. A new one is assigned on every reconnection.
    id: RwLock<Option<Sid>>,
}

impl Socket {
    /// Create a new [`Socket`] instance from an already connected transport.
    ///
    /// The transport should already have received a `CONNECT` packet.
    pub(crate) fn new(tx: mpsc::Sender<Packet>, ns: Str) -> Self {
        Self {
            tx,
            message_handlers: RwLock::new(HashMap::new()),
            fallback_message_handler: RwLock::new(None),
            disconnect_handler: Mutex::new(None),

            ack_message: Mutex::new(HashMap::new()),
            ack_counter: AtomicI64::default(),

            id: RwLock::new(None),
            ns,
            parser: Parser::default(),
        }
    }

    /// The id the server gave to this socket on its namespace.
    ///
    /// It is `None` while the socket is not connected, and it changes on every
    /// reconnection: it is not the id of the underlying engine.io connection.
    pub fn id(&self) -> Option<Sid> {
        *self.id.read().unwrap()
    }

    /// # Registers a [`MessageHandler`] for the given event.
    ///
    /// * See the [`message`](crate::handler::message) module doc for more details on message handler.
    /// * See the [`extract`](crate::extract) module doc for more details on available extractors.
    ///
    /// _It is recommended for code clarity to define your handler as top level function rather than closures._
    ///
    /// # Simple example with an async closure and an async fn:
    /// ```
    /// # use socketioxide::{SocketIo, extract::*};
    /// # use serde::{Serialize, Deserialize};
    /// #[derive(Debug, Serialize, Deserialize)]
    /// struct MyData {
    ///     name: String,
    ///     age: u8,
    /// }
    /// async fn handler(socket: SocketRef, Data(data): Data::<MyData>) {
    ///     println!("Received a test message {:?}", data);
    ///     socket.emit("test-test", &MyData { name: "Test".to_string(), age: 8 }).ok(); // Emit a message to the client
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
    ///     // Do the same thing but with an async function
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
    ///     // Do the same thing but with an async function
    ///     socket.on("test_2", handler);
    /// });
    /// ```
    pub fn on<H, T>(&self, event: impl Into<Cow<'static, str>>, handler: H)
    where
        H: MessageHandler<T>,
        T: Send + Sync + 'static,
    {
        self.message_handlers
            .write()
            .unwrap()
            .insert(event.into(), MakeErasedHandler::new_message_boxed(handler));
    }

    /// # Registers a fallback [`MessageHandler`] when no other handler is found. You can see this as a 404 handler.
    /// You can register only one fallback handler per socket. If you register multiple handlers, only the last one will be used.
    ///
    /// * See the [`message`](crate::handler::message) module doc for more details on message handler.
    /// * See the [`extract`](crate::extract) module doc for more details on available extractors.
    ///
    /// _It is recommended for code clarity to define your handler as top level function rather than closures._
    ///
    /// # Example:
    /// ```
    /// # use socketioxide::{SocketIo, extract::*};
    /// # use serde::{Serialize, Deserialize};
    /// # use serde_json::Value;
    /// async fn fallback_handler(socket: SocketRef, Event(event): Event, Data(data): Data::<Value>) {
    ///     println!("Received an {event} event with message {:?}", data);
    /// }
    ///
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", async |socket: SocketRef| {
    ///     // Register a fallback handler.
    ///     // In our example it will be always called as there is no other handler.
    ///     socket.on_fallback(fallback_handler);
    /// });
    ///
    /// ```
    pub fn on_fallback<H, T>(&self, handler: H)
    where
        H: MessageHandler<T>,
        T: Send + Sync + 'static,
    {
        self.fallback_message_handler
            .write()
            .unwrap()
            .replace(MakeErasedHandler::new_message_boxed(handler));
    }

    /// # Register a disconnect handler.
    /// You can register only one disconnect handler per socket. If you register multiple handlers, only the last one will be used.
    ///
    /// This implementation is slightly different to the socket.io spec.
    /// The difference being that [`rooms`](Self::rooms) are still available in this handler
    /// and only cleaned up AFTER the execution of this handler.
    /// Therefore you must not indefinitely stall/hang this handler, for example by entering an endless loop.
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
    /// io.ns("/", async |socket: SocketRef| {
    ///     socket.on("test", async |socket: SocketRef| {
    ///         // Close the current socket
    ///         socket.disconnect().ok();
    ///     });
    ///     socket.on_disconnect(async |socket: SocketRef, reason: DisconnectReason| {
    ///         println!("Socket {} on ns {} disconnected, reason: {:?}", socket.id(), socket.ns(), reason);
    ///     });
    /// });
    pub fn on_disconnect<C, T>(&self, callback: C)
    where
        C: DisconnectHandler<T> + Send + Sync + 'static,
        T: Send + Sync + 'static,
    {
        let handler = MakeErasedHandler::new_disconnect_boxed(callback);
        self.disconnect_handler.lock().unwrap().replace(handler);
    }

    pub async fn emit<T: ?Sized + Serialize>(
        &self,
        event: impl AsRef<str>,
        data: &T,
    ) -> Result<(), SendError> {
        let permit = self
            .tx
            .reserve()
            .await
            .map_err(|_| SendError::SocketClosed)?;

        let data = self.parser.encode_value(data, Some(event.as_ref()))?;
        permit.send(Packet::event(self.ns.clone(), data));

        Ok(())
    }

    pub async fn emit_with_ack<T: ?Sized + Serialize, V>(
        &self,
        event: impl AsRef<str>,
        data: &T,
    ) -> Result<AckFuture<V>, SendError> {
        let permit = self
            .tx
            .reserve()
            .await
            .map_err(|_| SendError::SocketClosed)?;

        let data = self.parser.encode_value(data, Some(event.as_ref()))?;
        let ack_id = self.ack_counter.fetch_add(1, atomic::Ordering::Relaxed);
        //TODO: dyn duration
        let (ack_fut, tx) = AckFuture::new(self.parser, Duration::from_secs(30));

        let packet = Packet::event_with_ack(self.ns.clone(), data, ack_id);
        self.ack_message.lock().unwrap().insert(ack_id, tx);

        permit.send(packet);
        Ok(ack_fut)
    }

    /// Disconnect this socket from its namespace.
    ///
    /// The socket cannot be used anymore afterwards, and the underlying
    /// engine.io connection is closed if it was the last one using it.
    pub async fn disconnect(&self) -> Result<(), SendError> {
        let permit = self
            .tx
            .reserve()
            .await
            .map_err(|_| SendError::SocketClosed)?;

        permit.send(Packet::disconnect(self.ns.clone()));

        Ok(())
    }
}

impl Socket {
    /// Dispatch an incoming packet of this namespace to the relevant handler.
    pub(crate) fn recv(self: &Arc<Self>, packet: PacketData) {
        match packet {
            PacketData::Event(data, ack) | PacketData::BinaryEvent(data, ack) => {
                self.recv_event(data, ack)
            }
            PacketData::Disconnect => self.close(DisconnectReason::IoServerDisconnect),
            PacketData::EventAck(v, ack_id) | PacketData::BinaryAck(v, ack_id) => {
                self.recv_ack(v, ack_id)
            }
            //TODO: namespace connection handshake.
            PacketData::Connect(_) | PacketData::ConnectError(_) => (),
        }
    }

    fn recv_event(self: &Arc<Self>, data: Value, ack: Option<i64>) {
        let event = match self.parser.read_event(&data) {
            Ok(event) => event,
            Err(_e) => {
                #[cfg(feature = "tracing")]
                tracing::debug!(?_e, "failed to read event name");
                return;
            }
        };

        if let Some(handler) = self.message_handlers.read().unwrap().get(event) {
            handler.call(self.clone(), data, ack);
        } else if let Some(fallback) = self.fallback_message_handler.read().unwrap().as_ref() {
            fallback.call(self.clone(), data, ack);
        }
    }

    fn recv_ack(&self, data: Value, ack: i64) {
        if let Some(tx) = self.ack_message.lock().unwrap().remove(&ack) {
            tx.send(Ok(data)).ok();
        } else {
            #[cfg(feature = "tracing")]
            tracing::debug!(sid = ?self.id(), "ack not found: {ack}");
        }
    }

    /// The server accepted this socket on its namespace and gave it this id.
    pub(crate) fn set_connected(&self, id: Sid) {
        *self.id.write().unwrap() = Some(id);
    }

    /// Notify the socket that it is disconnected, calling its disconnect handler.
    ///
    /// The socket may be connected again afterwards, on a new connection: the
    /// handler is cloned out rather than taken, so that it is called every time.
    pub(crate) fn close(self: &Arc<Self>, reason: DisconnectReason) {
        *self.id.write().unwrap() = None;
        // the acks of a connection that is over will never be answered, and
        // their ids mean nothing on the next one.
        self.ack_message.lock().unwrap().clear();

        // cloned so the lock is not held while the extractors run.
        let handler = self.disconnect_handler.lock().unwrap().clone();
        if let Some(handler) = handler {
            handler.call(self.clone(), reason);
        }
    }
}
