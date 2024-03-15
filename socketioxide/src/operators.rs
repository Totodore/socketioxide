//! Operators are used to select sockets to send a packet to,
//! or to configure the packet that will be emitted.
//!
//! They use the builder pattern to chain operators.
//!
//! There is two types of operators:
//! * [`ConfOperators`]: Chainable operators to configure the message to be sent.
//! * [`BroadcastOperators`]: Chainable operators to select sockets to send a message to and to configure the message to be sent.
use std::borrow::Cow;
use std::marker::PhantomData;
use std::{sync::Arc, time::Duration};

use bytes::Bytes;
use engineioxide::sid::Sid;
use serde_json::Value;

use crate::ack::{AckInnerStream, AckStream};
use crate::adapter::LocalAdapter;
use crate::errors::{BroadcastError, DisconnectError};
use crate::extract::SocketRef;
use crate::socket::Socket;
use crate::SendError;
use crate::{
    adapter::{Adapter, BroadcastFlags, BroadcastOptions, Room},
    ns::Namespace,
    packet::Packet,
};

use self::holding::{BinaryHolding, WithBinary, WithoutBinary};

pub(crate) mod holding {
    pub trait BinaryHolding {}

    pub struct WithoutBinary;
    impl BinaryHolding for WithoutBinary {}

    pub struct WithBinary;
    impl BinaryHolding for WithBinary {}
}

/// A trait for types that can be used as a room parameter.
///
/// [`String`], [`Vec<String>`], [`Vec<&str>`], [`&'static str`](str) and const arrays are implemented by default.
pub trait RoomParam: 'static {
    /// The type of the iterator returned by `into_room_iter`.
    type IntoIter: Iterator<Item = Room>;

    /// Convert `self` into an iterator of rooms.
    fn into_room_iter(self) -> Self::IntoIter;
}

impl RoomParam for Room {
    type IntoIter = std::iter::Once<Room>;
    #[inline(always)]
    fn into_room_iter(self) -> Self::IntoIter {
        std::iter::once(self)
    }
}
impl RoomParam for String {
    type IntoIter = std::iter::Once<Room>;
    #[inline(always)]
    fn into_room_iter(self) -> Self::IntoIter {
        std::iter::once(Cow::Owned(self))
    }
}
impl RoomParam for Vec<String> {
    type IntoIter = std::iter::Map<std::vec::IntoIter<String>, fn(String) -> Room>;
    #[inline(always)]
    fn into_room_iter(self) -> Self::IntoIter {
        self.into_iter().map(Cow::Owned)
    }
}
impl RoomParam for Vec<&'static str> {
    type IntoIter = std::iter::Map<std::vec::IntoIter<&'static str>, fn(&'static str) -> Room>;
    #[inline(always)]
    fn into_room_iter(self) -> Self::IntoIter {
        self.into_iter().map(Cow::Borrowed)
    }
}

impl RoomParam for Vec<Room> {
    type IntoIter = std::vec::IntoIter<Room>;
    #[inline(always)]
    fn into_room_iter(self) -> Self::IntoIter {
        self.into_iter()
    }
}
impl RoomParam for &'static str {
    type IntoIter = std::iter::Once<Room>;
    #[inline(always)]
    fn into_room_iter(self) -> Self::IntoIter {
        std::iter::once(Cow::Borrowed(self))
    }
}
impl<const COUNT: usize> RoomParam for [&'static str; COUNT] {
    type IntoIter =
        std::iter::Map<std::array::IntoIter<&'static str, COUNT>, fn(&'static str) -> Room>;

    #[inline(always)]
    fn into_room_iter(self) -> Self::IntoIter {
        self.into_iter().map(Cow::Borrowed)
    }
}
impl<const COUNT: usize> RoomParam for [String; COUNT] {
    type IntoIter = std::iter::Map<std::array::IntoIter<String, COUNT>, fn(String) -> Room>;
    #[inline(always)]
    fn into_room_iter(self) -> Self::IntoIter {
        self.into_iter().map(Cow::Owned)
    }
}
impl RoomParam for Sid {
    type IntoIter = std::iter::Once<Room>;
    #[inline(always)]
    fn into_room_iter(self) -> Self::IntoIter {
        std::iter::once(Cow::Owned(self.to_string()))
    }
}

/// Chainable operators to configure the message to be sent.
pub struct ConfOperators<'a, BH, A: Adapter = LocalAdapter> {
    binary: Vec<Bytes>,
    timeout: Option<Duration>,
    socket: &'a Socket<A>,
    _phantom: PhantomData<BH>,
}
/// Chainable operators to select sockets to send a message to and to configure the message to be sent.
pub struct BroadcastOperators<BH, A: Adapter = LocalAdapter> {
    binary: Vec<Bytes>,
    timeout: Option<Duration>,
    ns: Arc<Namespace<A>>,
    opts: BroadcastOptions,
    _phantom: PhantomData<BH>,
}

impl<BH: BinaryHolding, A: Adapter> From<ConfOperators<'_, BH, A>> for BroadcastOperators<BH, A> {
    fn from(conf: ConfOperators<'_, BH, A>) -> Self {
        let opts = BroadcastOptions {
            sid: Some(conf.socket.id),
            ..Default::default()
        };
        Self {
            binary: conf.binary,
            timeout: conf.timeout,
            ns: conf.socket.ns.clone(),
            opts,
            _phantom: PhantomData::<BH>,
        }
    }
}

impl<'a, A: Adapter> ConfOperators<'a, WithoutBinary, A> {
    pub(crate) fn new(sender: &'a Socket<A>) -> Self {
        Self {
            binary: vec![],
            timeout: None,
            socket: sender,
            _phantom: PhantomData::<WithoutBinary>,
        }
    }
}

// ==== impl ConfOperators operations for any BinaryHolding state ====
impl<'a, BH: BinaryHolding, A: Adapter> ConfOperators<'a, BH, A> {
    /// Selects all sockets in the given rooms except the current socket.
    /// If it is called from the `Namespace` level there will be no difference with the `within()` operator
    ///
    /// If you want to include the current socket, use the `within()` operator.
    /// #### Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::*};
    /// # use serde_json::Value;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///     socket.on("test", |socket: SocketRef, Data::<Value>(data)| async move {
    ///         let other_rooms = "room4".to_string();
    ///         // In room1, room2, room3 and room4 except the current
    ///         socket
    ///             .to("room1")
    ///             .to(["room2", "room3"])
    ///             .to(vec![other_rooms])
    ///             .emit("test", data);
    ///     });
    /// });
    pub fn to(self, rooms: impl RoomParam) -> BroadcastOperators<BH, A> {
        BroadcastOperators::from(self).to(rooms)
    }

    /// Selects all sockets in the given rooms.
    ///
    /// It does include the current socket contrary to the `to()` operator.
    /// If it is called from the `Namespace` level there will be no difference with the `to()` operator
    /// #### Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::*};
    /// # use serde_json::Value;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///     socket.on("test", |socket: SocketRef, Data::<Value>(data)| async move {
    ///         let other_rooms = "room4".to_string();
    ///         // In room1, room2, room3 and room4 including the current socket
    ///         socket
    ///             .within("room1")
    ///             .within(["room2", "room3"])
    ///             .within(vec![other_rooms])
    ///             .emit("test", data);
    ///     });
    /// });
    pub fn within(self, rooms: impl RoomParam) -> BroadcastOperators<BH, A> {
        BroadcastOperators::from(self).within(rooms)
    }

    /// Filters out all sockets selected with the previous operators which are in the given rooms.
    /// #### Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::*};
    /// # use serde_json::Value;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///     socket.on("register1", |socket: SocketRef, Data::<Value>(data)| async move {
    ///         socket.join("room1");
    ///     });
    ///     socket.on("register2", |socket: SocketRef, Data::<Value>(data)| async move {
    ///         socket.join("room2");
    ///     });
    ///     socket.on("test", |socket: SocketRef, Data::<Value>(data)| async move {
    ///         // This message will be broadcast to all sockets in the Namespace
    ///         // except for ones in room1 and the current socket
    ///         socket.broadcast().except("room1").emit("test", data);
    ///     });
    /// });
    pub fn except(self, rooms: impl RoomParam) -> BroadcastOperators<BH, A> {
        BroadcastOperators::from(self).except(rooms)
    }

    /// Broadcasts to all sockets only connected on this node (when using multiple nodes).
    /// When using the default in-memory adapter, this operator is a no-op.
    /// #### Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::*};
    /// # use serde_json::Value;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///     socket.on("test", |socket: SocketRef, Data::<Value>(data)| async move {
    ///         // This message will be broadcast to all sockets in this namespace and connected on this node
    ///         socket.local().emit("test", data);
    ///     });
    /// });
    pub fn local(self) -> BroadcastOperators<BH, A> {
        BroadcastOperators::from(self).local()
    }

    /// Broadcasts to all sockets without any filtering (except the current socket).
    /// #### Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::*};
    /// # use serde_json::Value;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///     socket.on("test", |socket: SocketRef, Data::<Value>(data)| async move {
    ///         // This message will be broadcast to all sockets in this namespace
    ///         socket.broadcast().emit("test", data);
    ///     });
    /// });
    pub fn broadcast(self) -> BroadcastOperators<BH, A> {
        BroadcastOperators::from(self).broadcast()
    }

    /// Sets a custom timeout when sending a message with an acknowledgement.
    ///
    /// See [`SocketIoBuilder::ack_timeout`](crate::SocketIoBuilder) for the default timeout.
    ///
    /// See [`emit_with_ack()`] for more details on acknowledgements.
    ///
    /// [`emit_with_ack()`]: #method.emit_with_ack
    ///
    /// # Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::*};
    /// # use serde_json::Value;
    /// # use futures::stream::StreamExt;
    /// # use std::time::Duration;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///    socket.on("test", |socket: SocketRef, Data::<Value>(data), Bin(bin)| async move {
    ///       // Emit a test message in the room1 and room3 rooms, except for the room2
    ///       // room with the binary payload received, wait for 5 seconds for an acknowledgement
    ///       socket.to("room1")
    ///             .to("room3")
    ///             .except("room2")
    ///             .bin(bin)
    ///             .timeout(Duration::from_secs(5))
    ///             .emit_with_ack::<Value>("message-back", data)
    ///             .unwrap()
    ///             .for_each(|(id, ack)| async move {
    ///                match ack {
    ///                    Ok(ack) => println!("Ack received, socket {} {:?}", id, ack),
    ///                    Err(err) => println!("Ack error, socket {} {:?}", id, err),
    ///                }
    ///             }).await;
    ///    });
    /// });
    ///
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }
}

// ==== impl ConfOperators consume fns for any BinaryHolding state ====
impl<BH: BinaryHolding, A: Adapter> ConfOperators<'_, BH, A> {
    /// Makes all sockets selected with the previous operators join the given room(s).
    ///
    /// ### Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::*};
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///   socket.on("test", |socket: SocketRef| async move {
    ///     // Add all sockets that are in the room1 and room3 to the room4 and room5
    ///     socket.within("room1").within("room3").join(["room4", "room5"]).unwrap();
    ///   });
    /// });
    pub fn join(self, rooms: impl RoomParam) -> Result<(), A::Error> {
        self.socket.join(rooms)
    }

    /// Makes all sockets selected with the previous operators leave the given room(s).
    ///
    /// ### Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::*};
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    /// socket.on("test", |socket: SocketRef| async move {
    ///     // Remove all sockets that are in the room1 and room3 from the room4 and room5
    ///     socket.within("room1").within("room3").leave(["room4", "room5"]).unwrap();
    ///   });
    /// });
    pub fn leave(self, rooms: impl RoomParam) -> Result<(), A::Error> {
        self.socket.leave(rooms)
    }

    /// Gets all room names for a given namespace
    pub fn rooms(self) -> Result<Vec<Room>, A::Error> {
        self.socket.rooms()
    }
}

// ==== impl ConfOperators consume fns for WithoutBinary ====
impl<'a, A: Adapter> ConfOperators<'a, WithoutBinary, A> {
    /// Adds a binary payload to the message.
    /// #### Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::*};
    /// # use serde_json::Value;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///     socket.on("test", |socket: SocketRef, Data::<Value>(data), Bin(bin)| async move {
    ///         // This will send the binary payload received to all sockets in this namespace with the test message
    ///         socket.bin(bin).emit("test", data);
    ///     });
    /// });
    pub fn bin(
        self,
        binary: impl IntoIterator<Item = impl Into<Bytes>>,
    ) -> ConfOperators<'a, WithBinary, A> {
        ConfOperators {
            binary: binary.into_iter().map(Into::into).collect(),
            timeout: self.timeout,
            socket: self.socket,
            _phantom: PhantomData::<WithBinary>,
        }
    }
}

// ==== impl ConfOperators consume fns for WithoutBinary ====
impl<A: Adapter> ConfOperators<'_, WithoutBinary, A> {
    /// Emits a message to the client and apply the previous operators on the message.
    ///
    /// If you provide array-like data (tuple, vec, arrays), it will be considered as multiple arguments.
    /// Therefore if you want to send an array as the _first_ argument of the payload,
    /// you need to wrap it in an array or a tuple.
    ///
    /// If the provided data contains binary data (via members of type [`bytes::Bytes`], or of some
    /// other type that will serialize to binary data), it will be extracted and added to the
    /// outgoing event as if [`bin()`] was called first.
    ///
    /// ## Errors
    /// * When encoding the data into JSON a [`SendError::Serialize`] may be returned.
    /// * If the underlying engine.io connection is closed a [`SendError::Socket(SocketError::Closed)`]
    /// will be returned and the provided data to be send will be given back in the error.
    /// * If the packet buffer is full, a [`SendError::Socket(SocketError::InternalChannelFull)`]
    /// will be returned and the provided data to be send will be given back in the error.
    /// See [`SocketIoBuilder::max_buffer_size`] option for more infos on internal buffer config
    ///
    /// [`SocketIoBuilder::max_buffer_size`]: crate::SocketIoBuilder#method.max_buffer_size
    /// [`SendError::Socket(SocketError::Closed)`]: crate::SocketError::Closed
    /// [`SendError::Socket(SocketError::InternalChannelFull)`]: crate::SocketError::InternalChannelFull
    ///
    /// #### Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::*};
    /// # use serde_json::Value;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///     socket.on("test", |socket: SocketRef, Data::<Value>(data), Bin(bin)| async move {
    ///          // Emit a test message to the client
    ///         socket.bin(bin.clone()).emit("test", data).ok();
    ///
    ///         // Emit a test message with multiple arguments to the client
    ///         let msg = serde_json::to_value(&("world", "hello", 1)).unwrap();
    ///         socket.bin(bin.clone()).emit("test", msg).ok();
    ///
    ///         // Emit a test message with an array as the first argument
    ///         let arr = serde_json::to_value(&[1, 2, 3, 4]).unwrap();
    ///         socket.bin(bin).emit("test", arr).ok();
    ///     });
    /// });
    pub fn emit<T: serde::Serialize>(
        self,
        event: impl Into<Cow<'static, str>>,
        data: T,
    ) -> Result<(), SendError<Value>> {
        let (data, bins) = crate::to_value(data)?;
        self.bin(bins).emit(event, data)
    }

    /// Emits a message to the client and wait for acknowledgement.
    ///
    /// The acknowledgement has a timeout specified in the config (5s by default)
    /// (see [`SocketIoBuilder::ack_timeout`]) or with the [`timeout()`] operator.
    ///
    /// To get acknowledgements, an [`AckStream`] is returned.
    /// It can be used in two ways:
    /// * As a [`Stream`]: It will yield all the [`AckResponse`] with their corresponding socket id
    /// received from the client. It can useful when broadcasting to multiple sockets and therefore expecting
    /// more than one acknowledgement. If you want to get the socket from this id, use [`io::get_socket()`].
    /// * As a [`Future`]: It will yield the first [`AckResponse`] received from the client.
    /// Useful when expecting only one acknowledgement.
    ///
    /// If the provided data contains binary data (via members of type [`bytes::Bytes`], or of some
    /// other type that will serialize to binary data), it will be extracted and added to the
    /// outgoing event as if [`bin()`] was called first.
    ///
    /// If the packet encoding failed a [`serde_json::Error`] is **immediately** returned.
    ///
    /// If the socket is full or if it has been closed before receiving the acknowledgement,
    /// an [`SendError::Socket`] will be **immediately returned** and the value to send will be given back.
    ///
    /// If the client didn't respond before the timeout, the [`AckStream`] will yield
    /// an [`AckError::Timeout`]. If the data sent by the client is not deserializable as `V`,
    /// an [`AckError::Serde`] will be yielded.
    ///
    /// [`timeout()`]: crate::operators::ConfOperators#method.timeout
    /// [`SocketIoBuilder::ack_timeout`]: crate::SocketIoBuilder#method.ack_timeout
    /// [`Stream`]: futures::stream::Stream
    /// [`Future`]: futures::future::Future
    /// [`AckResponse`]: crate::ack::AckResponse
    /// [`AckError`]: crate::AckError
    /// [`AckError::Serde`]: crate::AckError::Serde
    /// [`AckError::Timeout`]: crate::AckError::Timeout
    /// [`AckError::Socket`]: crate::AckError::Socket
    /// [`AckError::Socket(SocketError::Closed)`]: crate::SocketError::Closed
    /// [`io::get_socket()`]: crate::SocketIo#method.get_socket
    ///
    /// # Basic example
    /// ```
    /// # use socketioxide::{SocketIo, extract::*};
    /// # use serde_json::Value;
    /// # use std::sync::Arc;
    /// # use tokio::time::Duration;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///     socket.on("test", |socket: SocketRef, Data::<Value>(data), Bin(bin)| async move {
    ///         // Emit a test message and wait for an acknowledgement with the timeout specified in the config
    ///         match socket.bin(bin).timeout(Duration::from_millis(2)).emit_with_ack::<Value>("test", data).unwrap().await {
    ///             Ok(ack) => println!("Ack received {:?}", ack),
    ///             Err(err) => println!("Ack error {:?}", err),
    ///         }
    ///    });
    /// });
    /// ```
    pub fn emit_with_ack<T: serde::Serialize, V>(
        self,
        event: impl Into<Cow<'static, str>>,
        data: T,
    ) -> Result<AckStream<V>, SendError<Value>> {
        let (data, bins) = crate::to_value(data)?;
        self.bin(bins).emit_with_ack(event, data)
    }
}

// ==== impl ConfOperators consume fns for WithBinary ====
impl<A: Adapter> ConfOperators<'_, WithBinary, A> {
    /// Emits a message to the client and apply the previous operators on the message.
    ///
    /// If you provide array-like data (tuple, vec, arrays), it will be considered as multiple arguments.
    /// Therefore if you want to send an array as the _first_ argument of the payload,
    /// you need to wrap it in an array or a tuple.
    ///
    /// ## Errors
    /// * If the underlying engine.io connection is closed a [`SendError::Socket(SocketError::Closed)`]
    /// will be returned and the provided data to be send will be given back in the error.
    /// * If the packet buffer is full, a [`SendError::Socket(SocketError::InternalChannelFull)`]
    /// will be returned and the provided data to be send will be given back in the error.
    /// See [`SocketIoBuilder::max_buffer_size`] option for more infos on internal buffer config
    ///
    /// [`SocketIoBuilder::max_buffer_size`]: crate::SocketIoBuilder#method.max_buffer_size
    /// [`SendError::Socket(SocketError::Closed)`]: crate::SocketError::Closed
    /// [`SendError::Socket(SocketError::InternalChannelFull)`]: crate::SocketError::InternalChannelFull
    ///
    /// #### Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::*};
    /// # use serde_json::Value;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///     socket.on("test", |socket: SocketRef, Data::<Value>(data), Bin(bin)| async move {
    ///          // Emit a test message to the client
    ///         socket.bin(bin.clone()).emit("test", data).ok();
    ///
    ///         // Emit a test message with multiple arguments to the client
    ///         let msg = serde_json::to_value(&("world", "hello", 1)).unwrap();
    ///         socket.bin(bin.clone()).emit("test", msg).ok();
    ///
    ///         // Emit a test message with an array as the first argument
    ///         let arr = serde_json::to_value(&[1, 2, 3, 4]).unwrap();
    ///         socket.bin(bin).emit("test", arr).ok();
    ///     });
    /// });
    pub fn emit(
        mut self,
        event: impl Into<Cow<'static, str>>,
        data: Value,
    ) -> Result<(), SendError<Value>> {
        use crate::errors::SocketError;
        use crate::socket::PermitExt;
        if !self.socket.connected() {
            return Err(SendError::Socket(SocketError::Closed(data)));
        }
        let permit = match self.socket.reserve() {
            Ok(permit) => permit,
            Err(e) => {
                #[cfg(feature = "tracing")]
                tracing::debug!("sending error during emit message: {e:?}");
                return Err(e.with_value(data).into());
            }
        };
        let packet = self.get_packet(event, data)?;
        permit.send(packet);

        Ok(())
    }

    /// Emits a message to the client and wait for acknowledgement.
    ///
    /// The acknowledgement has a timeout specified in the config (5s by default)
    /// (see [`SocketIoBuilder::ack_timeout`]) or with the [`timeout()`] operator.
    ///
    /// To get acknowledgements, an [`AckStream`] is returned.
    /// It can be used in two ways:
    /// * As a [`Stream`]: It will yield all the [`AckResponse`] with their corresponding socket id
    /// received from the client. It can useful when broadcasting to multiple sockets and therefore expecting
    /// more than one acknowledgement. If you want to get the socket from this id, use [`io::get_socket()`].
    /// * As a [`Future`]: It will yield the first [`AckResponse`] received from the client.
    /// Useful when expecting only one acknowledgement.
    ///
    /// If the socket is full or if it has been closed before receiving the acknowledgement,
    /// an [`SendError::Socket`] will be **immediately returned** and the value to send will be given back.
    ///
    /// If the client didn't respond before the timeout, the [`AckStream`] will yield
    /// an [`AckError::Timeout`]. If the data sent by the client is not deserializable as `V`,
    /// an [`AckError::Serde`] will be yielded.
    ///
    /// [`timeout()`]: crate::operators::ConfOperators#method.timeout
    /// [`SocketIoBuilder::ack_timeout`]: crate::SocketIoBuilder#method.ack_timeout
    /// [`Stream`]: futures::stream::Stream
    /// [`Future`]: futures::future::Future
    /// [`AckResponse`]: crate::ack::AckResponse
    /// [`AckError`]: crate::AckError
    /// [`AckError::Serde`]: crate::AckError::Serde
    /// [`AckError::Timeout`]: crate::AckError::Timeout
    /// [`AckError::Socket`]: crate::AckError::Socket
    /// [`AckError::Socket(SocketError::Closed)`]: crate::SocketError::Closed
    /// [`io::get_socket()`]: crate::SocketIo#method.get_socket
    ///
    /// # Basic example
    /// ```
    /// # use socketioxide::{SocketIo, extract::*};
    /// # use serde_json::Value;
    /// # use std::sync::Arc;
    /// # use tokio::time::Duration;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///     socket.on("test", |socket: SocketRef, Data::<Value>(data), Bin(bin)| async move {
    ///         // Emit a test message and wait for an acknowledgement with the timeout specified in the config
    ///         match socket.bin(bin).timeout(Duration::from_millis(2)).emit_with_ack::<Value>("test", data).unwrap().await {
    ///             Ok(ack) => println!("Ack received {:?}", ack),
    ///             Err(err) => println!("Ack error {:?}", err),
    ///         }
    ///    });
    /// });
    /// ```
    pub fn emit_with_ack<V>(
        mut self,
        event: impl Into<Cow<'static, str>>,
        data: Value,
    ) -> Result<AckStream<V>, SendError<Value>> {
        use crate::errors::SocketError;
        if !self.socket.connected() {
            return Err(SendError::Socket(SocketError::Closed(data)));
        }
        let permit = match self.socket.reserve() {
            Ok(permit) => permit,
            Err(e) => {
                #[cfg(feature = "tracing")]
                tracing::debug!("sending error during emit message: {e:?}");
                return Err(e.with_value(data).into());
            }
        };
        let timeout = self.timeout.unwrap_or(self.socket.config.ack_timeout);
        let packet = self.get_packet(event, data)?;
        let rx = self.socket.send_with_ack_permit(packet, permit);
        let stream = AckInnerStream::send(rx, timeout, self.socket.id);
        Ok(AckStream::<V>::from(stream))
    }

    /// Creates a packet with the given event and data.
    fn get_packet(
        &mut self,
        event: impl Into<Cow<'static, str>>,
        data: Value,
    ) -> Result<Packet<'static>, serde_json::Error> {
        let ns = self.socket.ns.path.clone();
        let packet = if self.binary.is_empty() {
            Packet::event(ns, event.into(), data)
        } else {
            let binary = std::mem::take(&mut self.binary);
            Packet::bin_event(ns, event.into(), data, binary)
        };
        Ok(packet)
    }
}

impl<A: Adapter> BroadcastOperators<WithoutBinary, A> {
    pub(crate) fn new(ns: Arc<Namespace<A>>) -> Self {
        Self {
            binary: vec![],
            timeout: None,
            ns,
            opts: BroadcastOptions::default(),
            _phantom: PhantomData::<WithoutBinary>,
        }
    }
    pub(crate) fn from_sock(ns: Arc<Namespace<A>>, sid: Sid) -> Self {
        Self {
            binary: vec![],
            timeout: None,
            ns,
            opts: BroadcastOptions {
                sid: Some(sid),
                ..Default::default()
            },
            _phantom: PhantomData::<WithoutBinary>,
        }
    }
}

impl<BH: BinaryHolding, A: Adapter> BroadcastOperators<BH, A> {
    /// Selects all sockets in the given rooms except the current socket.
    /// If it is called from the `Namespace` level there will be no difference with the `within()` operator
    ///
    /// If you want to include the current socket, use the `within()` operator.
    /// #### Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::*};
    /// # use serde_json::Value;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///     socket.on("test", |socket: SocketRef, Data::<Value>(data)| async move {
    ///         let other_rooms = "room4".to_string();
    ///         // In room1, room2, room3 and room4 except the current
    ///         socket
    ///             .to("room1")
    ///             .to(["room2", "room3"])
    ///             .to(vec![other_rooms])
    ///             .emit("test", data);
    ///     });
    /// });
    pub fn to(mut self, rooms: impl RoomParam) -> Self {
        self.opts.rooms.extend(rooms.into_room_iter());
        self.broadcast()
    }

    /// Selects all sockets in the given rooms.
    ///
    /// It does include the current socket contrary to the `to()` operator.
    /// If it is called from the `Namespace` level there will be no difference with the `to()` operator
    /// #### Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::*};
    /// # use serde_json::Value;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///     socket.on("test", |socket: SocketRef, Data::<Value>(data)| async move {
    ///         let other_rooms = "room4".to_string();
    ///         // In room1, room2, room3 and room4 including the current socket
    ///         socket
    ///             .within("room1")
    ///             .within(["room2", "room3"])
    ///             .within(vec![other_rooms])
    ///             .emit("test", data);
    ///     });
    /// });
    pub fn within(mut self, rooms: impl RoomParam) -> Self {
        self.opts.rooms.extend(rooms.into_room_iter());
        self
    }

    /// Filters out all sockets selected with the previous operators which are in the given rooms.
    /// #### Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::*};
    /// # use serde_json::Value;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///     socket.on("register1", |socket: SocketRef, Data::<Value>(data)| async move {
    ///         socket.join("room1");
    ///     });
    ///     socket.on("register2", |socket: SocketRef, Data::<Value>(data)| async move {
    ///         socket.join("room2");
    ///     });
    ///     socket.on("test", |socket: SocketRef, Data::<Value>(data)| async move {
    ///         // This message will be broadcast to all sockets in the Namespace
    ///         // except for ones in room1 and the current socket
    ///         socket.broadcast().except("room1").emit("test", data);
    ///     });
    /// });
    pub fn except(mut self, rooms: impl RoomParam) -> Self {
        self.opts.except.extend(rooms.into_room_iter());
        self.broadcast()
    }

    /// Broadcasts to all sockets only connected on this node (when using multiple nodes).
    /// When using the default in-memory adapter, this operator is a no-op.
    /// #### Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::*};
    /// # use serde_json::Value;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///     socket.on("test", |socket: SocketRef, Data::<Value>(data)| async move {
    ///         // This message will be broadcast to all sockets in this namespace and connected on this node
    ///         socket.local().emit("test", data);
    ///     });
    /// });
    pub fn local(mut self) -> Self {
        self.opts.flags.insert(BroadcastFlags::Local);
        self
    }

    /// Broadcasts to all sockets without any filtering (except the current socket).
    /// #### Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::*};
    /// # use serde_json::Value;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///     socket.on("test", |socket: SocketRef, Data::<Value>(data)| async move {
    ///         // This message will be broadcast to all sockets in this namespace
    ///         socket.broadcast().emit("test", data);
    ///     });
    /// });
    pub fn broadcast(mut self) -> Self {
        self.opts.flags.insert(BroadcastFlags::Broadcast);
        self
    }

    /// Sets a custom timeout when sending a message with an acknowledgement.
    ///
    /// See [`SocketIoBuilder::ack_timeout`](crate::SocketIoBuilder) for the default timeout.
    ///
    /// See [`emit_with_ack()`] for more details on acknowledgements.
    ///
    /// [`emit_with_ack()`]: #method.emit_with_ack
    ///
    /// # Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::*};
    /// # use serde_json::Value;
    /// # use futures::stream::StreamExt;
    /// # use std::time::Duration;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///    socket.on("test", |socket: SocketRef, Data::<Value>(data), Bin(bin)| async move {
    ///       // Emit a test message in the room1 and room3 rooms, except for the room2
    ///       // room with the binary payload received, wait for 5 seconds for an acknowledgement
    ///       socket.to("room1")
    ///             .to("room3")
    ///             .except("room2")
    ///             .bin(bin)
    ///             .timeout(Duration::from_secs(5))
    ///             .emit_with_ack::<Value>("message-back", data)
    ///             .unwrap()
    ///             .for_each(|(id, ack)| async move {
    ///                match ack {
    ///                    Ok(ack) => println!("Ack received, socket {} {:?}", id, ack),
    ///                    Err(err) => println!("Ack error, socket {} {:?}", id, err),
    ///                }
    ///             }).await;
    ///    });
    /// });
    ///
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }
}

// ==== impl BroadcastOperators consume fns for WithoutBinary ====
impl<A: Adapter> BroadcastOperators<WithoutBinary, A> {
    /// Adds a binary payload to the message.
    /// #### Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::*};
    /// # use serde_json::Value;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///     socket.on("test", |socket: SocketRef, Data::<Value>(data), Bin(bin)| async move {
    ///         // This will send the binary payload received to all sockets in this namespace with the test message
    ///         socket.bin(bin).emit("test", data);
    ///     });
    /// });
    pub fn bin(
        self,
        binary: impl IntoIterator<Item = impl Into<Bytes>>,
    ) -> BroadcastOperators<WithBinary, A> {
        BroadcastOperators {
            binary: binary.into_iter().map(Into::into).collect(),
            timeout: self.timeout,
            ns: self.ns,
            opts: self.opts,
            _phantom: PhantomData::<WithBinary>,
        }
    }

    /// Emits a message to all sockets selected with the previous operators.
    ///
    /// If you provide array-like data (tuple, vec, arrays), it will be considered as multiple arguments.
    /// Therefore if you want to send an array as the _first_ argument of the payload,
    /// you need to wrap it in an array or a tuple.
    ///
    /// If the provided data contains binary data (via members of type [`bytes::Bytes`], or of some
    /// other type that will serialize to binary data), it will be extracted and added to the
    /// outgoing event as if [`bin()`] was called first.
    ///
    /// ## Errors
    /// * When encoding the data into JSON a [`BroadcastError::Serialize`] may be returned.
    /// * If the underlying engine.io connection is closed for a given socket a [`BroadcastError::Socket(SocketError::Closed)`]
    /// will be returned.
    /// * If the packet buffer is full for a given socket, a [`BroadcastError::Socket(SocketError::InternalChannelFull)`]
    /// will be retured.
    /// See [`SocketIoBuilder::max_buffer_size`] option for more infos on internal buffer config
    ///
    /// > **Note**: If a error is returned because of a specific socket, the message will still be sent to all other sockets.
    ///
    /// [`SocketIoBuilder::max_buffer_size`]: crate::SocketIoBuilder#method.max_buffer_size
    /// [`BroadcastError::Socket(SocketError::Closed)`]: crate::SocketError::Closed
    /// [`BroadcastError::Socket(SocketError::InternalChannelFull)`]: crate::SocketError::InternalChannelFull
    ///
    /// #### Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::*};
    /// # use serde_json::Value;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///     socket.on("test", |socket: SocketRef, Data::<Value>(data), Bin(bin)| async move {
    ///         // Emit a test message in the room1 and room3 rooms, except for the room2 room with the binary payload received
    ///         socket.to("room1").to("room3").except("room2").bin(bin).emit("test", data);
    ///
    ///         // Emit a test message with multiple arguments to the client
    ///         socket.to("room1").emit("test", ("world", "hello", 1)).ok();
    ///
    ///         // Emit a test message with an array as the first argument
    ///         let arr = [1, 2, 3, 4];
    ///         socket.to("room2").emit("test", [arr]).ok();
    ///     });
    /// });
    pub fn emit<T: serde::Serialize>(
        self,
        event: impl Into<Cow<'static, str>>,
        data: T,
    ) -> Result<(), BroadcastError> {
        let (data, bins) = crate::to_value(data)?;
        self.bin(bins).emit(event, data)
    }

    /// Emits a message to all sockets selected with the previous operators and
    /// waits for the acknowledgement(s).
    ///
    /// The acknowledgement has a timeout specified in the config (5s by default)
    /// (see [`SocketIoBuilder::ack_timeout`](crate::SocketIoBuilder)) or with the [`timeout()`] operator.
    ///
    /// To get acknowledgements, an [`AckStream`] is returned.
    /// It can be used in two ways:
    /// * As a [`Stream`]: It will yield all the [`AckResponse`] with their corresponding socket id
    /// received from the client. It can useful when broadcasting to multiple sockets and therefore expecting
    /// more than one acknowledgement. If you want to get the socket from this id, use [`io::get_socket()`].
    /// * As a [`Future`]: It will yield the first [`AckResponse`] received from the client.
    /// Useful when expecting only one acknowledgement.
    ///
    /// If the provided data contains binary data (via members of type [`bytes::Bytes`], or of some
    /// other type that will serialize to binary data), it will be extracted and added to the
    /// outgoing event as if [`bin()`] was called first.
    ///
    /// If the packet encoding failed a [`serde_json::Error`] is **immediately** returned.
    ///
    /// If the socket is full or if it has been closed before receiving the acknowledgement,
    /// an [`AckError::Socket`] will be yielded.
    ///
    /// If the client didn't respond before the timeout, the [`AckStream`] will yield
    /// an [`AckError::Timeout`]. If the data sent by the client is not deserializable as `V`,
    /// an [`AckError::Serde`] will be yielded.
    ///
    /// [`timeout()`]: #method.timeout
    /// [`Stream`]: futures::stream::Stream
    /// [`Future`]: futures::future::Future
    /// [`AckResponse`]: crate::ack::AckResponse
    /// [`AckError::Serde`]: crate::AckError::Serde
    /// [`AckError::Timeout`]: crate::AckError::Timeout
    /// [`AckError::Socket`]: crate::AckError::Socket
    /// [`AckError::Socket(SocketError::Closed)`]: crate::SocketError::Closed
    /// [`io::get_socket()`]: crate::SocketIo#method.get_socket
    ///
    /// # Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::*};
    /// # use serde_json::Value;
    /// # use futures::stream::StreamExt;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///     socket.on("test", |socket: SocketRef, Data::<Value>(data), Bin(bin)| async move {
    ///         // Emit a test message in the room1 and room3 rooms,
    ///         // except for the room2 room with the binary payload received
    ///         let ack_stream = socket.to("room1")
    ///             .to("room3")
    ///             .except("room2")
    ///             .bin(bin)
    ///             .emit_with_ack::<String>("message-back", data)
    ///             .unwrap();
    ///
    ///         ack_stream.for_each(|(id, ack)| async move {
    ///             match ack {
    ///                 Ok(ack) => println!("Ack received, socket {} {:?}", id, ack),
    ///                 Err(err) => println!("Ack error, socket {} {:?}", id, err),
    ///             }
    ///         }).await;
    ///     });
    /// });
    pub fn emit_with_ack<V>(
        self,
        event: impl Into<Cow<'static, str>>,
        data: impl serde::Serialize,
    ) -> Result<AckStream<V>, serde_json::Error> {
        let (data, bins) = crate::to_value(data)?;
        self.bin(bins).emit_with_ack(event, data)
    }
}

// ==== impl BroadcastOperators consume fns for WithBinary ====
impl<A: Adapter> BroadcastOperators<WithBinary, A> {
    /// Emits a message to all sockets selected with the previous operators.
    ///
    /// If you provide array-like data (tuple, vec, arrays), it will be considered as multiple arguments.
    /// Therefore if you want to send an array as the _first_ argument of the payload,
    /// you need to wrap it in an array or a tuple.
    ///
    /// ## Errors
    /// * If the underlying engine.io connection is closed for a given socket a [`BroadcastError::Socket(SocketError::Closed)`]
    /// will be returned.
    /// * If the packet buffer is full for a given socket, a [`BroadcastError::Socket(SocketError::InternalChannelFull)`]
    /// will be retured.
    /// See [`SocketIoBuilder::max_buffer_size`] option for more infos on internal buffer config
    ///
    /// > **Note**: If a error is returned because of a specific socket, the message will still be sent to all other sockets.
    ///
    /// [`SocketIoBuilder::max_buffer_size`]: crate::SocketIoBuilder#method.max_buffer_size
    /// [`BroadcastError::Socket(SocketError::Closed)`]: crate::SocketError::Closed
    /// [`BroadcastError::Socket(SocketError::InternalChannelFull)`]: crate::SocketError::InternalChannelFull
    ///
    /// #### Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::*};
    /// # use serde_json::Value;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///     socket.on("test", |socket: SocketRef, Data::<Value>(data), Bin(bin)| async move {
    ///         // Emit a test message in the room1 and room3 rooms, except for the room2 room with the binary payload received
    ///         socket.to("room1").to("room3").except("room2").bin(bin).emit("test", data);
    ///
    ///         // Emit a test message with multiple arguments to the client
    ///         socket.to("room1").emit("test", ("world", "hello", 1)).ok();
    ///
    ///         // Emit a test message with an array as the first argument
    ///         let arr = [1, 2, 3, 4];
    ///         socket.to("room2").emit("test", [arr]).ok();
    ///     });
    /// });
    pub fn emit(
        mut self,
        event: impl Into<Cow<'static, str>>,
        data: Value,
    ) -> Result<(), BroadcastError> {
        let packet = self.get_packet(event, data)?;
        if let Err(e) = self.ns.adapter.broadcast(packet, self.opts) {
            #[cfg(feature = "tracing")]
            tracing::debug!("broadcast error: {e:?}");
            return Err(e);
        }
        Ok(())
    }

    /// Emits a message to all sockets selected with the previous operators and
    /// waits for the acknowledgement(s).
    ///
    /// The acknowledgement has a timeout specified in the config (5s by default)
    /// (see [`SocketIoBuilder::ack_timeout`](crate::SocketIoBuilder)) or with the [`timeout()`] operator.
    ///
    /// To get acknowledgements, an [`AckStream`] is returned.
    /// It can be used in two ways:
    /// * As a [`Stream`]: It will yield all the [`AckResponse`] with their corresponding socket id
    /// received from the client. It can useful when broadcasting to multiple sockets and therefore expecting
    /// more than one acknowledgement. If you want to get the socket from this id, use [`io::get_socket()`].
    /// * As a [`Future`]: It will yield the first [`AckResponse`] received from the client.
    /// Useful when expecting only one acknowledgement.
    ///
    /// If the socket is full or if it has been closed before receiving the acknowledgement,
    /// an [`AckError::Socket`] will be yielded.
    ///
    /// If the client didn't respond before the timeout, the [`AckStream`] will yield
    /// an [`AckError::Timeout`]. If the data sent by the client is not deserializable as `V`,
    /// an [`AckError::Serde`] will be yielded.
    ///
    /// [`timeout()`]: #method.timeout
    /// [`Stream`]: futures::stream::Stream
    /// [`Future`]: futures::future::Future
    /// [`AckResponse`]: crate::ack::AckResponse
    /// [`AckError::Serde`]: crate::AckError::Serde
    /// [`AckError::Timeout`]: crate::AckError::Timeout
    /// [`AckError::Socket`]: crate::AckError::Socket
    /// [`AckError::Socket(SocketError::Closed)`]: crate::SocketError::Closed
    /// [`io::get_socket()`]: crate::SocketIo#method.get_socket
    ///
    /// # Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::*};
    /// # use serde_json::Value;
    /// # use futures::stream::StreamExt;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///     socket.on("test", |socket: SocketRef, Data::<Value>(data), Bin(bin)| async move {
    ///         // Emit a test message in the room1 and room3 rooms,
    ///         // except for the room2 room with the binary payload received
    ///         let ack_stream = socket.to("room1")
    ///             .to("room3")
    ///             .except("room2")
    ///             .bin(bin)
    ///             .emit_with_ack::<String>("message-back", data)
    ///             .unwrap();
    ///
    ///         ack_stream.for_each(|(id, ack)| async move {
    ///             match ack {
    ///                 Ok(ack) => println!("Ack received, socket {} {:?}", id, ack),
    ///                 Err(err) => println!("Ack error, socket {} {:?}", id, err),
    ///             }
    ///         }).await;
    ///     });
    /// });
    pub fn emit_with_ack<V>(
        mut self,
        event: impl Into<Cow<'static, str>>,
        data: Value,
    ) -> Result<AckStream<V>, serde_json::Error> {
        let packet = self.get_packet(event, data)?;
        let stream = self
            .ns
            .adapter
            .broadcast_with_ack(packet, self.opts, self.timeout)
            .into();
        Ok(stream)
    }

    /// Creates a packet with the given event and data.
    fn get_packet(
        &mut self,
        event: impl Into<Cow<'static, str>>,
        data: Value,
    ) -> Result<Packet<'static>, serde_json::Error> {
        let ns = self.ns.path.clone();
        let packet = if self.binary.is_empty() {
            Packet::event(ns, event.into(), data)
        } else {
            let binary = std::mem::take(&mut self.binary);
            Packet::bin_event(ns, event.into(), data, binary)
        };
        Ok(packet)
    }
}

impl<BH: BinaryHolding, A: Adapter> BroadcastOperators<BH, A> {
    /// Gets all sockets selected with the previous operators.
    ///
    /// It can be used to retrieve any extension data (with the `extensions` feature enabled) from the sockets or to make some sockets join other rooms.
    ///
    /// ### Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::*};
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///   socket.on("test", |socket: SocketRef| async move {
    ///     // Find an extension data in each sockets in the room1 and room3 rooms, except for the room2
    ///     let sockets = socket.within("room1").within("room3").except("room2").sockets().unwrap();
    ///     for socket in sockets {
    ///         println!("Socket custom string: {:?}", socket.extensions.get::<String>());
    ///     }
    ///   });
    /// });
    pub fn sockets(self) -> Result<Vec<SocketRef<A>>, A::Error> {
        self.ns.adapter.fetch_sockets(self.opts)
    }

    /// Disconnects all sockets selected with the previous operators.
    ///
    /// ### Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::*};
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///   socket.on("test", |socket: SocketRef| async move {
    ///     // Disconnect all sockets in the room1 and room3 rooms, except for the room2
    ///     socket.within("room1").within("room3").except("room2").disconnect().unwrap();
    ///   });
    /// });
    pub fn disconnect(self) -> Result<(), Vec<DisconnectError>> {
        self.ns.adapter.disconnect_socket(self.opts)
    }

    /// Makes all sockets selected with the previous operators join the given room(s).
    ///
    /// ### Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::*};
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///   socket.on("test", |socket: SocketRef| async move {
    ///     // Add all sockets that are in the room1 and room3 to the room4 and room5
    ///     socket.within("room1").within("room3").join(["room4", "room5"]).unwrap();
    ///   });
    /// });
    pub fn join(self, rooms: impl RoomParam) -> Result<(), A::Error> {
        self.ns.adapter.add_sockets(self.opts, rooms)
    }

    /// Makes all sockets selected with the previous operators leave the given room(s).
    ///
    /// ### Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::*};
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    /// socket.on("test", |socket: SocketRef| async move {
    ///     // Remove all sockets that are in the room1 and room3 from the room4 and room5
    ///     socket.within("room1").within("room3").leave(["room4", "room5"]).unwrap();
    ///   });
    /// });
    pub fn leave(self, rooms: impl RoomParam) -> Result<(), A::Error> {
        self.ns.adapter.del_sockets(self.opts, rooms)
    }

    /// Gets all room names for a given namespace
    pub fn rooms(self) -> Result<Vec<Room>, A::Error> {
        self.ns.adapter.rooms()
    }

    /// Gets a [`SocketRef`] by the specified [`Sid`].
    pub fn get_socket(&self, sid: Sid) -> Option<SocketRef<A>> {
        self.ns.get_socket(sid).map(SocketRef::from).ok()
    }
}
