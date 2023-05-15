use std::{
    collections::HashMap,
    pin::Pin,
    sync::{
        atomic::{AtomicI64, Ordering},
        Arc, RwLock,
    },
    time::Duration,
};

use futures::{Future, TryFutureExt};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};
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

type BoxAsyncFut<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;
pub type AckResponse<T> = (T, Option<Vec<Vec<u8>>>);

pub enum Ack<T>
where
    T: Serialize + Send + Sync + 'static,
{
    Bin(Vec<Vec<u8>>),
    Data(T),
    DataBin(T, Vec<Vec<u8>>),
    None,
}

impl From<()> for Ack<()> {
    fn from(_: ()) -> Self {
        Ack::None
    }
}

trait MessageCaller<A: Adapter>: Send + Sync + 'static {
    fn call(
        &self,
        s: Arc<Socket<A>>,
        v: Value,
        p: Option<Vec<Vec<u8>>>,
        ack_id: Option<i64>,
    ) -> Result<(), Error>;
}

struct MessageHandler<Param, ParamCb, F, A>
where
    Param: Send + Sync + 'static,
    ParamCb: Send + Sync + 'static,
{
    param: std::marker::PhantomData<Param>,
    param_cb: std::marker::PhantomData<ParamCb>,
    adapter: std::marker::PhantomData<A>,
    handler: F,
}

impl<Param, RetV, F, A> MessageCaller<A> for MessageHandler<Param, RetV, F, A>
where
    Param: DeserializeOwned + Send + Sync + 'static,
    RetV: Serialize + Send + Sync + 'static,
    F: Fn(Arc<Socket<A>>, Param, Option<Vec<Vec<u8>>>) -> BoxAsyncFut<Result<Ack<RetV>, Error>>
        + Send
        + Sync
        + 'static,
    A: Adapter,
{
    fn call(
        &self,
        s: Arc<Socket<A>>,
        v: Value,
        p: Option<Vec<Vec<u8>>>,
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
        let fut = (self.handler)(s, v, p);
        if let Some(ack_id) = ack_id {
            // Send ack for the message if it has an ack_id
            tokio::spawn(fut.map_ok(move |val| match val {
                Ack::Bin(b) => owned_socket.send_bin_ack(ack_id, json!({}), b),
                Ack::Data(d) => owned_socket.send_ack(ack_id, d),
                Ack::DataBin(d, b) => owned_socket.send_bin_ack(ack_id, d, b),
                Ack::None => Ok(()),
            }));
        } else {
            tokio::spawn(fut);
        }
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

    /// Register a message handler for the given event.
    /// ## Example with a closure :
    /// ```
    /// use socketio_server::{Namespace, Ack};
    /// use serde_json::Value;
    /// Namespace::builder().add("/", |socket| async move {
    ///     socket.on("test", |socket, data: Value, bin| async move {
    ///         println!("Received a test message {:?}", data);
    ///     });
    /// });
    /// ```
    pub fn on<C, F, V, RetV>(&self, event: impl Into<String>, callback: C)
    where
        C: Fn(Arc<Socket<A>>, V, Option<Vec<Vec<u8>>>) -> F + Send + Sync + 'static,
        F: Future<Output = Result<Ack<RetV>, Error>> + Send + 'static,
        V: DeserializeOwned + Send + Sync + 'static,
        RetV: Serialize + Send + Sync + 'static,
    {
        let handler = Box::new(move |s, v, p| Box::pin(callback(s, v, p)) as _);
        self.message_handlers.write().unwrap().insert(
            event.into(),
            Box::new(MessageHandler {
                param: std::marker::PhantomData,
                param_cb: std::marker::PhantomData,
                adapter: std::marker::PhantomData,
                handler,
            }),
        );
    }

    /// Emit a message to the client
    /// ## Example :
    /// ```
    /// use socketio_server::{Namespace, Ack};
    /// use serde_json::Value;
    /// Namespace::builder().add("/", |socket| async move {
    ///     socket.on("test", |socket, data: Value, bin| async move {
    ///         // Emit a test message to the client
    ///         socket.emit("test", data);
    ///         Ok(Ack::<()>::None)
    ///     });
    /// });
    pub fn emit(&self, event: impl Into<String>, data: impl Serialize) -> Result<(), Error> {
        let ns = self.ns.path.clone();
        let data = serde_json::to_value(data)?;
        self.client
            .emit(self.sid, Packet::event(ns, event.into(), data))
    }

    /// Emit a message to the client and wait for acknowledgement.
    /// 
    /// The acknowledgement has a timeout specified in the config (5s by default) or with the `timeout()` operator.
    /// ## Example :
    /// ```
    /// use socketio_server::{Namespace, Ack};
    /// use serde_json::Value;
    /// Namespace::builder().add("/", |socket| async move {
    ///     socket.on("test", |socket, data: Value, bin| async move {
    ///         // Emit a test message and wait for an acknowledgement
    ///         match socket.emit_with_ack::<Value>("test", data).await {
    ///             Ok(ack) => println!("Ack received {:?}", ack),
    ///             Err(err) => println!("Ack error {:?}", err),
    ///         }
    ///         Ok(Ack::<()>::None)
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

        self.send_with_ack(packet, None, None).await
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
    /// ## Example :
    /// ```
    /// use socketio_server::{Namespace, Ack};
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
    ///         Ok(Ack::<()>::None)
    ///     });
    /// });
    pub fn to(&self, rooms: impl RoomParam) -> Operators<A> {
        Operators::new(self.ns.clone(), self.sid).to(rooms)
    }

    /// Filter out all clients selected with the previous operators which are in the given rooms.
    /// ## Example :
    /// ```
    /// use socketio_server::{Namespace, Ack};
    /// use serde_json::Value;
    /// Namespace::builder().add("/", |socket| async move {
    ///     socket.on("register1", |socket, data: Value, _| async move {
    ///         socket.join("room1");
    ///         Ok(Ack::<()>::None)
    ///     });
    ///     socket.on("register2", |socket, data: Value, _| async move {
    ///         socket.join("room2");
    ///         Ok(Ack::<()>::None)
    ///     });
    ///     socket.on("test", |socket, data: Value, _| async move {
    ///         // This message will be broadcast to all clients in the Namespace
    ///         // except for ones in room1 and the current socket
    ///         socket.broadcast().except("room1").emit("test", data);
    ///         Ok(Ack::<()>::None)
    ///     });
    /// });
    pub fn except(&self, rooms: impl RoomParam) -> Operators<A> {
        Operators::new(self.ns.clone(), self.sid).except(rooms)
    }

    /// Broadcast to all clients only connected on this node (when using multiple nodes).
    /// When using the default in-memory adapter, this operator is a no-op.
    /// ## Example :
    /// ```
    /// use socketio_server::{Namespace, Ack};
    /// use serde_json::Value;
    /// Namespace::builder().add("/", |socket| async move {
    ///     socket.on("test", |socket, data: Value, _| async move {
    ///         // This message will be broadcast to all clients in this namespace and connected on this node
    ///         socket.local().emit("test", data);
    ///         Ok(Ack::<()>::None)
    ///     });
    /// });
    pub fn local(&self) -> Operators<A> {
        Operators::new(self.ns.clone(), self.sid).local()
    }

    /// Set a custom timeout when sending a message with an acknowledgement.
    ///
    /// ## Example :
    /// ```
    /// use socketio_server::{Namespace, Ack};
    /// use serde_json::Value;
    /// use futures::stream::StreamExt;
    /// use std::time::Duration;
    /// Namespace::builder().add("/", |socket| async move {
    ///    socket.on("test", |socket, data: Value, bin| async move {
    ///       // Emit a test message in the room1 and room3 rooms, except for the room2 room with the binary payload received, wait for 5 seconds for an acknowledgement
    ///       socket.to("room1")
    ///             .to("room3")
    ///             .except("room2")
    ///             .bin(bin.unwrap())
    ///             .timeout(Duration::from_secs(5))
    ///             .emit_with_ack::<Value>("message-back", data).unwrap().for_each(|ack| async move {
    ///                match ack {
    ///                    Ok(ack) => println!("Ack received {:?}", ack),
    ///                    Err(err) => println!("Ack error {:?}", err),
    ///                }
    ///             }).await;
    ///       Ok(Ack::<()>::None)
    ///    });
    /// });
    ///
    pub fn timeout(&self, timeout: Duration) -> Operators<A> {
        Operators::new(self.ns.clone(), self.sid).timeout(timeout)
    }

    /// Add a binary payload to the message.
    /// ## Example :
    /// ```
    /// use socketio_server::{Namespace, Ack};
    /// use serde_json::Value;
    /// Namespace::builder().add("/", |socket| async move {
    ///     socket.on("test", |socket, data: Value, bin| async move {
    ///         // This will send the binary payload received to all clients in this namespace with the test message
    ///         socket.bin(bin.unwrap()).emit("test", data);
    ///         Ok(Ack::<()>::None)
    ///     });
    /// });
    pub fn bin(&self, binary: Vec<Vec<u8>>) -> Operators<A> {
        Operators::new(self.ns.clone(), self.sid).bin(binary)
    }

    /// Broadcast to all clients without any filtering (except the current socket).
    /// ## Example :
    /// ```
    /// use socketio_server::{Namespace, Ack};
    /// use serde_json::Value;
    /// Namespace::builder().add("/", |socket| async move {
    ///     socket.on("test", |socket, data: Value, _| async move {
    ///         // This message will be broadcast to all clients in this namespace
    ///         socket.broadcast().emit("test", data);
    ///         Ok(Ack::<()>::None)
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

    pub(crate) fn send(&self, packet: Packet, payload: Option<Vec<Vec<u8>>>) -> Result<(), Error> {
        if let Some(payload) = payload {
            self.client.emit_bin(self.sid, packet, payload)
        } else {
            self.client.emit(self.sid, packet)
        }
    }

    pub(crate) async fn send_with_ack<V: DeserializeOwned>(
        &self,
        mut packet: Packet,
        payload: Option<Vec<Vec<u8>>>,
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

    pub(crate) fn send_ack(&self, ack_id: i64, data: impl Serialize) -> Result<(), Error> {
        let ns = self.ns.path.clone();
        let data = serde_json::to_value(&data)?;
        self.send(Packet::ack(ns, data, ack_id), None)
    }
    pub(crate) fn send_bin_ack(
        &self,
        ack_id: i64,
        data: impl Serialize,
        bin: Vec<Vec<u8>>,
    ) -> Result<(), Error> {
        let ns = self.ns.path.clone();
        let data = serde_json::to_value(&data)?;
        self.client
            .emit_bin(self.sid, Packet::bin_ack(ns, data, bin.len(), ack_id), bin)
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
            handler.call(self.clone(), data, None, ack)?;
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
            handler.call(self.clone(), packet.data, Some(packet.bin), ack)?;
        }
        Ok(())
    }

    fn recv_ack(self: Arc<Self>, data: Value, ack: i64) -> Result<(), Error> {
        if let Some(tx) = self.ack_message.write().unwrap().remove(&ack) {
            tx.send((data, None)).ok();
        }
        Ok(())
    }

    fn recv_bin_ack(self: Arc<Self>, packet: BinaryPacket, ack: i64) -> Result<(), Error> {
        if let Some(tx) = self.ack_message.write().unwrap().remove(&ack) {
            tx.send((packet.data, Some(packet.bin))).ok();
        }
        Ok(())
    }
}
