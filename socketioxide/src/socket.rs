use std::{
    collections::HashMap,
    fmt::Debug,
    sync::{
        atomic::{AtomicI64, Ordering},
        Arc, RwLock,
    },
    time::Duration,
};

use futures::Future;
use futures_core::future::BoxFuture;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use tokio::sync::oneshot;

use crate::{
    adapter::Adapter,
    client::Client,
    errors::{AckError, Error},
    handshake::Handshake,
    ns::Namespace,
    operators::{Operators, RoomParam},
    packet::{BinaryPacket, Packet, PacketData},
};
pub type AckResponse<T> = (T, Vec<Vec<u8>>);

trait MessageCaller<A: Adapter>: Send + Sync + 'static {
    fn call(
        &self,
        s: Arc<Socket<A>>,
        v: Value,
        p: Vec<Vec<u8>>,
        ack_id: Option<i64>,
    ) -> Result<(), Error>;
}

struct MessageHandler<Param, F, AckV, A>
where
    Param: Send + Sync + 'static,
    AckV: Send + Sync + 'static,
{
    param: std::marker::PhantomData<Param>,
    adapter: std::marker::PhantomData<A>,
    ack_val: std::marker::PhantomData<AckV>,
    handler: F,
}

type AckFn<AckV> = Box<dyn FnOnce(AckV) -> Result<(), Error> + Send + Sync + 'static>;

impl<Param, AckV, F, A> MessageCaller<A> for MessageHandler<Param, F, AckV, A>
where
    Param: DeserializeOwned + Send + Sync + 'static,
    AckV: Serialize + Send + Sync + 'static,
    F: Fn(Arc<Socket<A>>, Param, Vec<Vec<u8>>, AckFn<AckV>) -> BoxFuture<'static, ()>
        + Send
        + Sync
        + 'static,
    A: Adapter,
{
    fn call(
        &self,
        s: Arc<Socket<A>>,
        v: Value,
        p: Vec<Vec<u8>>,
        ack_id: Option<i64>,
    ) -> Result<(), Error> {
        // Unwrap array if it has only one element
        let v = match v {
            Value::Array(v) => {
                if v.len() == 1 {
                    v.into_iter().next().unwrap_or(Value::Null)
                } else {
                    Value::Array(v)
                }
            }
            v => v,
        };
        let v: Param = serde_json::from_value(v)?;
        let owned_socket = s.clone();
        let ack_fn: AckFn<AckV> = if let Some(ack_id) = ack_id {
            Box::new(move |data| {
                let ns = owned_socket.ns().clone();
                let data = serde_json::to_value(&data)?;
                owned_socket.send(Packet::ack(ns, data, ack_id), vec![])
            } as _)
        } else {
            Box::new(move |_: AckV| Ok(()) as _)
        };
        let fut = (self.handler)(s, v, p, ack_fn);
        tokio::spawn(fut);
        Ok(())
    }
}

pub struct Socket<A: Adapter> {
    client: Arc<Client<A>>,
    ns: Arc<Namespace<A>>,
    message_handlers: RwLock<HashMap<String, Box<dyn MessageCaller<A>>>>,
    ack_message: RwLock<HashMap<i64, oneshot::Sender<AckResponse<Value>>>>,
    ack_counter: AtomicI64,
    pub handshake: Handshake,
    pub sid: i64,
}

impl<A: Adapter> Socket<A> {
    pub(crate) fn new(
        client: Arc<Client<A>>,
        ns: Arc<Namespace<A>>,
        handshake: Handshake,
        sid: i64,
    ) -> Self {
        Self {
            client,
            ns,
            message_handlers: RwLock::new(HashMap::new()),
            ack_message: RwLock::new(HashMap::new()),
            ack_counter: AtomicI64::new(0),
            handshake,
            sid,
        }
    }

    /// ### Register a message handler for the given event.
    ///
    /// The data parameter can be type with anything that implement [serde::deserialize](https://docs.rs/serde/latest/serde/)
    /// #### Example with a closure :
    /// ```
    /// use socketioxide::Namespace;
    /// use serde_json::Value;
    /// use serde::{Serialize, Deserialize};
    ///
    /// #[derive(Debug, Serialize, Deserialize)]
    /// struct MyData {
    ///     name: String,
    ///     age: u8,
    /// }
    ///
    /// Namespace::builder().add("/", |socket| async move {
    ///     socket.on("test", |socket, data: MyData, bin| async move {
    ///         println!("Received a test message {:?}", data);
    ///     });
    /// });
    /// ```
    pub fn on<C, F, V>(&self, event: impl Into<String>, callback: C)
    where
        C: Fn(Arc<Socket<A>>, V, Vec<Vec<u8>>) -> F + Send + Sync + 'static,
        F: Future<Output = ()> + Send + 'static,
        V: DeserializeOwned + Send + Sync + 'static,
    {
        let handler = Box::new(move |s, v, p, _: AckFn<()>| Box::pin(callback(s, v, p)) as _);
        self.message_handlers.write().unwrap().insert(
            event.into(),
            Box::new(MessageHandler {
                param: std::marker::PhantomData,
                ack_val: std::marker::PhantomData,
                adapter: std::marker::PhantomData,
                handler,
            }),
        );
    }

    /// ### Register a message handler for the given event with an ack.
    ///
    /// The data parameter can be typed with anything that implement [serde::Deserialize](https://docs.rs/serde/latest/serde/)
    ///
    /// The ack can be sent only once and take a `Serializable` value as parameter.
    ///
    /// For more info about ack see [socket.io documentation](https://socket.io/fr/docs/v4/emitting-events/#acknowledgements)
    ///
    /// If the client sent a normal message without expecting an ack, the ack callback will do nothing.
    ///
    /// #### Example with a closure :
    /// ```
    /// use socketioxide::Namespace;
    /// use serde_json::Value;
    /// use serde::{Serialize, Deserialize};
    ///
    /// #[derive(Debug, Serialize, Deserialize)]
    /// struct MyData {
    ///     name: String,
    ///     age: u8,
    /// }
    ///
    /// Namespace::builder().add("/", |socket| async move {
    ///     socket.on_ack("test", |socket, data: MyData, bin, ack| async move {
    ///         println!("Received a test message {:?}", data);
    ///         ack(data); // The data received is sent back to the client through the ack
    ///     });
    /// });
    /// ```
    pub fn on_ack<C, F, V, AckV>(&self, event: impl Into<String>, callback: C)
    where
        C: Fn(Arc<Socket<A>>, V, Vec<Vec<u8>>, AckFn<AckV>) -> F + Send + Sync + 'static,
        F: Future<Output = ()> + Send + 'static,
        V: DeserializeOwned + Send + Sync + 'static,
        AckV: Serialize + Send + Sync + 'static,
    {
        let handler = Box::new(move |s, v, p, ack_fn| Box::pin(callback(s, v, p, ack_fn)) as _);
        self.message_handlers.write().unwrap().insert(
            event.into(),
            Box::new(MessageHandler {
                param: std::marker::PhantomData,
                ack_val: std::marker::PhantomData,
                adapter: std::marker::PhantomData,
                handler,
            }),
        );
    }

    /// Emit a message to the client
    /// ### Example :
    /// ```
    /// use socketioxide::Namespace;
    /// use serde_json::Value;
    /// Namespace::builder().add("/", |socket| async move {
    ///     socket.on("test", |socket, data: Value, bin| async move {
    ///         // Emit a test message to the client
    ///         socket.emit("test", data);
    ///     });
    /// });
    pub fn emit(&self, event: impl Into<String>, data: impl Serialize) -> Result<(), Error> {
        let ns = self.ns.path.clone();
        let data = serde_json::to_value(data)?;
        self.send(Packet::event(ns, event.into(), data), vec![])
    }

    /// Emit a message to the client and wait for acknowledgement.
    ///
    /// The acknowledgement has a timeout specified in the config (5s by default) or with the `timeout()` operator.
    /// ### Example :
    /// ```
    /// use socketioxide::Namespace;
    /// use serde_json::Value;
    /// Namespace::builder().add("/", |socket| async move {
    ///     socket.on("test", |socket, data: Value, bin| async move {
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

        self.send_with_ack(packet, vec![], None).await
    }

    // Room actions

    /// Join the given rooms.
    pub fn join(&self, rooms: impl RoomParam) {
        self.ns.adapter.add_all(self.sid, rooms);
    }

    /// Leave the given rooms.
    pub fn leave(&self, rooms: impl RoomParam) {
        self.ns.adapter.del(self.sid, rooms);
    }

    /// Leave all rooms where the socket is connected.
    pub fn leave_all(&self) {
        self.ns.adapter.del_all(self.sid);
    }

    // Socket operators

    /// Select all clients in the given rooms except the current socket.
    /// ### Example :
    /// ```
    /// use socketioxide::Namespace;
    /// use serde_json::Value;
    /// Namespace::builder().add("/", |socket| async move {
    ///     socket.on("test", |socket, data: Value, _| async move {
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

    /// Filter out all clients selected with the previous operators which are in the given rooms.
    /// ### Example :
    /// ```
    /// use socketioxide::Namespace;
    /// use serde_json::Value;
    /// Namespace::builder().add("/", |socket| async move {
    ///     socket.on("register1", |socket, data: Value, _| async move {
    ///         socket.join("room1");
    ///     });
    ///     socket.on("register2", |socket, data: Value, _| async move {
    ///         socket.join("room2");
    ///     });
    ///     socket.on("test", |socket, data: Value, _| async move {
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
    /// ### Example :
    /// ```
    /// use socketioxide::Namespace;
    /// use serde_json::Value;
    /// Namespace::builder().add("/", |socket| async move {
    ///     socket.on("test", |socket, data: Value, _| async move {
    ///         // This message will be broadcast to all clients in this namespace and connected on this node
    ///         socket.local().emit("test", data);
    ///     });
    /// });
    pub fn local(&self) -> Operators<A> {
        Operators::new(self.ns.clone(), self.sid).local()
    }

    /// Set a custom timeout when sending a message with an acknowledgement.
    ///
    /// ### Example :
    /// ```
    /// use socketioxide::Namespace;
    /// use serde_json::Value;
    /// use futures::stream::StreamExt;
    /// use std::time::Duration;
    /// Namespace::builder().add("/", |socket| async move {
    ///    socket.on("test", |socket, data: Value, bin| async move {
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
    /// ### Example :
    /// ```
    /// use socketioxide::Namespace;
    /// use serde_json::Value;
    /// Namespace::builder().add("/", |socket| async move {
    ///     socket.on("test", |socket, data: Value, bin| async move {
    ///         // This will send the binary payload received to all clients in this namespace with the test message
    ///         socket.bin(bin).emit("test", data);
    ///     });
    /// });
    pub fn bin(&self, binary: Vec<Vec<u8>>) -> Operators<A> {
        Operators::new(self.ns.clone(), self.sid).bin(binary)
    }

    /// Broadcast to all clients without any filtering (except the current socket).
    /// ### Example :
    /// ```
    /// use socketioxide::Namespace;
    /// use serde_json::Value;
    /// Namespace::builder().add("/", |socket| async move {
    ///     socket.on("test", |socket, data: Value, _| async move {
    ///         // This message will be broadcast to all clients in this namespace
    ///         socket.broadcast().emit("test", data);
    ///     });
    /// });
    pub fn broadcast(&self) -> Operators<A> {
        Operators::new(self.ns.clone(), self.sid).broadcast()
    }

    /// Disconnect the socket from the current namespace.
    pub fn disconnect(&self) -> Result<(), Error> {
        self.ns.disconnect(self.sid)
    }

    /// Get the current namespace path.
    pub fn ns(&self) -> &String {
        &self.ns.path
    }

    pub(crate) fn send(&self, packet: Packet, payload: Vec<Vec<u8>>) -> Result<(), Error> {
        self.client.emit(self.sid, packet, payload)
    }

    pub(crate) async fn send_with_ack<V: DeserializeOwned>(
        &self,
        mut packet: Packet,
        payload: Vec<Vec<u8>>,
        timeout: Option<Duration>,
    ) -> Result<AckResponse<V>, AckError> {
        let (tx, rx) = oneshot::channel();
        let ack = self.ack_counter.fetch_add(1, Ordering::SeqCst) + 1;
        self.ack_message.write().unwrap().insert(ack, tx);
        packet.inner.set_ack_id(ack);
        self.send(packet, payload)?;
        let timeout = timeout.unwrap_or(self.client.config.ack_timeout);
        let v = tokio::time::timeout(timeout, rx).await??;
        Ok((serde_json::from_value(v.0)?, v.1))
    }

    // Receive data from client:

    pub(crate) fn recv(self: Arc<Self>, packet: PacketData) -> Result<(), Error> {
        match packet {
            PacketData::Event(e, data, ack) => self.recv_event(e, data, ack),
            PacketData::EventAck(data, ack_id) => self.recv_ack(data, ack_id),
            PacketData::BinaryEvent(e, packet, ack) => self.recv_bin_event(e, packet, ack),
            PacketData::BinaryAck(packet, ack) => self.recv_bin_ack(packet, ack),
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
        if let Some(tx) = self.ack_message.write().unwrap().remove(&ack) {
            tx.send((data, vec![])).ok();
        }
        Ok(())
    }

    fn recv_bin_ack(self: Arc<Self>, packet: BinaryPacket, ack: i64) -> Result<(), Error> {
        if let Some(tx) = self.ack_message.write().unwrap().remove(&ack) {
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
    pub fn new_dummy(sid: i64, ns: Arc<Namespace<A>>) -> Socket<A> {
        use crate::SocketIoConfig;
        use std::sync::Weak;
        let client = Arc::new(Client::new(
            SocketIoConfig::default(),
            Weak::new(),
            HashMap::new(),
        ));
        Socket::new(client, ns, Handshake::new_dummy(), sid)
    }
}
