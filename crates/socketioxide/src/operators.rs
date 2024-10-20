//! Operators are used to select sockets to send a packet to,
//! or to configure the packet that will be emitted.
//!
//! They use the builder pattern to chain operators.
//!
//! There is two types of operators:
//! * [`ConfOperators`]: Chainable operators to configure the message to be sent.
//! * [`BroadcastOperators`]: Chainable operators to select sockets to send a message to and to configure the message to be sent.
use std::borrow::Cow;
use std::{sync::Arc, time::Duration};

use engineioxide::sid::Sid;
use serde::Serialize;
use socketioxide_core::parser::Parse;

use crate::ack::{AckInnerStream, AckStream};
use crate::adapter::LocalAdapter;
use crate::errors::{BroadcastError, DisconnectError};
use crate::extract::SocketRef;
use crate::parser::{self, Parser};
use crate::socket::Socket;
use crate::SendError;
use crate::{
    adapter::{Adapter, BroadcastFlags, BroadcastOptions, Room},
    ns::Namespace,
    packet::Packet,
};

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
pub struct ConfOperators<'a, A: Adapter = LocalAdapter> {
    timeout: Option<Duration>,
    socket: &'a Socket<A>,
}
/// Chainable operators to select sockets to send a message to and to configure the message to be sent.
pub struct BroadcastOperators<A: Adapter = LocalAdapter> {
    timeout: Option<Duration>,
    ns: Arc<Namespace<A>>,
    parser: Parser,
    opts: BroadcastOptions,
}

impl<A: Adapter> From<ConfOperators<'_, A>> for BroadcastOperators<A> {
    fn from(conf: ConfOperators<'_, A>) -> Self {
        let opts = BroadcastOptions {
            sid: Some(conf.socket.id),
            ..Default::default()
        };
        Self {
            timeout: conf.timeout,
            ns: conf.socket.ns.clone(),
            parser: conf.socket.parser(),
            opts,
        }
    }
}

// ==== impl ConfOperators operations ====
impl<'a, A: Adapter> ConfOperators<'a, A> {
    pub(crate) fn new(sender: &'a Socket<A>) -> Self {
        Self {
            timeout: None,
            socket: sender,
        }
    }

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
    ///             .emit("test", &data);
    ///     });
    /// });
    pub fn to(self, rooms: impl RoomParam) -> BroadcastOperators<A> {
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
    ///             .emit("test", &data);
    ///     });
    /// });
    pub fn within(self, rooms: impl RoomParam) -> BroadcastOperators<A> {
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
    ///         socket.broadcast().except("room1").emit("test", &data);
    ///     });
    /// });
    pub fn except(self, rooms: impl RoomParam) -> BroadcastOperators<A> {
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
    ///         socket.local().emit("test", &data);
    ///     });
    /// });
    pub fn local(self) -> BroadcastOperators<A> {
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
    ///         socket.broadcast().emit("test", &data);
    ///     });
    /// });
    pub fn broadcast(self) -> BroadcastOperators<A> {
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
    /// # use futures_util::stream::StreamExt;
    /// # use std::time::Duration;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///    socket.on("test", |socket: SocketRef, Data::<Value>(data)| async move {
    ///       // Emit a test message in the room1 and room3 rooms, except for the room2
    ///       // room with the binary payload received, wait for 5 seconds for an acknowledgement
    ///       socket.to("room1")
    ///             .to("room3")
    ///             .except("room2")
    ///             .timeout(Duration::from_secs(5))
    ///             .emit_with_ack::<_, Value>("message-back", &data)
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

// ==== impl ConfOperators consume fns ====
impl<A: Adapter> ConfOperators<'_, A> {
    /// Emits a message to the client and apply the previous operators on the message.
    ///
    /// If you provide tuple-like data (tuple, arrays), it will be considered as multiple arguments.
    /// Therefore if you want to send an array as the _first_ argument of the payload,
    /// you need to wrap it in an array or a tuple. [`Vec`] will be always considered as a single argument though.
    ///
    /// ## Emitting binary data
    /// To emit binary data, you must use a data type that implements [`Serialize`] as binary data.
    /// Currently if you use `Vec<u8>` it will be considered as a number sequence and not binary data.
    /// To counter that you must either use a special type like [`Bytes`] or use the [`serde_bytes`] crate.
    /// If you want to emit generic data that may contains binary, use [`rmpv::Value`] rather
    /// than [`serde_json::Value`] otherwise the binary data will also be serialized as a number sequence.
    ///
    /// ## Errors
    /// * When encoding the data a [`SendError::Serialize`] may be returned.
    /// * If the underlying engine.io connection is closed a [`SendError::Socket(SocketError::Closed)`]
    ///   will be returned and the provided data to be send will be given back in the error.
    /// * If the packet buffer is full, a [`SendError::Socket(SocketError::InternalChannelFull)`]
    ///   will be returned and the provided data to be send will be given back in the error.
    ///   See [`SocketIoBuilder::max_buffer_size`] option for more infos on internal buffer config
    ///
    /// [`SocketIoBuilder::max_buffer_size`]: crate::SocketIoBuilder#method.max_buffer_size
    /// [`SendError::Socket(SocketError::Closed)`]: crate::SocketError::Closed
    /// [`SendError::Socket(SocketError::InternalChannelFull)`]: crate::SocketError::InternalChannelFull
    /// [`Bytes`]: bytes::Bytes
    /// [`serde_bytes`]: https://docs.rs/serde_bytes
    /// [`rmpv::Value`]: https://docs.rs/rmpv
    /// [`serde_json::Value`]: https://docs.rs/serde_json/latest/serde_json/value
    /// #### Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::*};
    /// # use serde_json::Value;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///     socket.on("test", |socket: SocketRef, Data::<Value>(data)| async move {
    ///          // Emit a test message to the client
    ///         socket.emit("test", &data).ok();
    ///
    ///         // Emit a test message with multiple arguments to the client
    ///         socket.emit("test", &("world", "hello", 1)).ok();
    ///
    ///         // Emit a test message with an array as the first argument
    ///         let arr = [1, 2, 3, 4];
    ///         socket.emit("test", &[arr]).ok();
    ///     });
    /// });
    /// ```
    ///
    /// ## Binary Example with the `bytes` crate
    /// ```
    /// # use socketioxide::{SocketIo, extract::*};
    /// # use serde_json::Value;
    /// # use std::sync::Arc;
    /// # use bytes::Bytes;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///     socket.on("test", |socket: SocketRef, Data::<(String, Bytes, Bytes)>(data)| async move {
    ///         // Emit a test message to the client
    ///         socket.emit("test", &data).ok();
    ///
    ///         // Emit a test message with multiple arguments to the client
    ///         socket.emit("test", &("world", "hello", Bytes::from_static(&[1, 2, 3, 4]))).ok();
    ///
    ///         // Emit a test message with an array as the first argument
    ///         let arr = [1, 2, 3, 4];
    ///         socket.emit("test", &[arr]).ok();
    ///     });
    /// });
    /// ```
    pub fn emit<T: ?Sized + Serialize>(
        mut self,
        event: impl AsRef<str>,
        data: &T,
    ) -> Result<(), SendError> {
        use crate::errors::SocketError;
        use crate::socket::PermitExt;
        if !self.socket.connected() {
            return Err(SendError::Socket(SocketError::Closed));
        }
        let permit = match self.socket.reserve() {
            Ok(permit) => permit,
            Err(e) => {
                #[cfg(feature = "tracing")]
                tracing::debug!("sending error during emit message: {e:?}");
                return Err(SendError::Socket(e));
            }
        };
        let packet = self.get_packet(event, data)?;
        permit.send(packet, self.socket.parser());

        Ok(())
    }

    /// Emits a message to the client and wait for acknowledgement.
    ///
    /// See [`emit()`](#method.emit) for more details on emitting messages.
    ///
    /// The acknowledgement has a timeout specified in the config (5s by default)
    /// (see [`SocketIoBuilder::ack_timeout`]) or with the [`timeout()`] operator.
    ///
    /// To get acknowledgements, an [`AckStream`] is returned.
    /// It can be used in two ways:
    /// * As a [`Stream`]: It will yield all the ack responses with their corresponding socket id
    ///   received from the client. It can useful when broadcasting to multiple sockets and therefore expecting
    ///   more than one acknowledgement. If you want to get the socket from this id, use [`io::get_socket()`].
    /// * As a [`Future`]: It will yield the first ack response received from the client.
    ///   Useful when expecting only one acknowledgement.
    ///
    /// If the packet encoding failed an [`EncodeError`] is **immediately** returned.
    ///
    /// If the socket is full or if it has been closed before receiving the acknowledgement,
    /// an [`SendError::Socket`] will be **immediately returned** and the value to send will be given back.
    ///
    /// If the client didn't respond before the timeout, the [`AckStream`] will yield
    /// an [`AckError::Timeout`]. If the data sent by the client is not deserializable as `V`,
    /// an [`AckError::Decode`] will be yielded.
    ///
    /// [`timeout()`]: crate::operators::ConfOperators#method.timeout
    /// [`SocketIoBuilder::ack_timeout`]: crate::SocketIoBuilder#method.ack_timeout
    /// [`Stream`]: futures_core::stream::Stream
    /// [`Future`]: futures_core::future::Future
    /// [`AckError`]: crate::AckError
    /// [`AckError::Decode`]: crate::AckError::Decode
    /// [`AckError::Timeout`]: crate::AckError::Timeout
    /// [`AckError::Socket`]: crate::AckError::Socket
    /// [`AckError::Socket(SocketError::Closed)`]: crate::SocketError::Closed
    /// [`EncodeError`]: crate::EncodeError
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
    ///     socket.on("test", |socket: SocketRef, Data::<Value>(data)| async move {
    ///         // Emit a test message and wait for an acknowledgement with the timeout specified in the config
    ///         match socket.timeout(Duration::from_millis(2)).emit_with_ack::<_, Value>("test", &data).unwrap().await {
    ///             Ok(ack) => println!("Ack received {:?}", ack),
    ///             Err(err) => println!("Ack error {:?}", err),
    ///         }
    ///    });
    /// });
    /// ```
    pub fn emit_with_ack<T: ?Sized + Serialize, V>(
        mut self,
        event: impl AsRef<str>,
        data: &T,
    ) -> Result<AckStream<V>, SendError> {
        use crate::errors::SocketError;
        if !self.socket.connected() {
            return Err(SendError::Socket(SocketError::Closed));
        }
        let permit = match self.socket.reserve() {
            Ok(permit) => permit,
            Err(e) => {
                #[cfg(feature = "tracing")]
                tracing::debug!("sending error during emit message: {e:?}");
                return Err(SendError::Socket(e));
            }
        };
        let timeout = self
            .timeout
            .unwrap_or_else(|| self.socket.get_io().config().ack_timeout);
        let packet = self.get_packet(event, data)?;
        let rx = self.socket.send_with_ack_permit(packet, permit);
        let stream = AckInnerStream::send(rx, timeout, self.socket.id);
        Ok(AckStream::<V>::new(stream, self.socket.parser()))
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

    /// Creates a packet with the given event and data.
    fn get_packet<T: ?Sized + Serialize>(
        &mut self,
        event: impl AsRef<str>,
        data: &T,
    ) -> Result<Packet, parser::EncodeError> {
        let ns = self.socket.ns.path.clone();
        let event = event.as_ref();
        let data = self.socket.parser().encode_value(&data, Some(event))?;
        Ok(Packet::event(ns, data))
    }
}

impl<A: Adapter> BroadcastOperators<A> {
    pub(crate) fn new(ns: Arc<Namespace<A>>, parser: Parser) -> Self {
        Self {
            timeout: None,
            ns,
            parser,
            opts: BroadcastOptions::default(),
        }
    }
    pub(crate) fn from_sock(ns: Arc<Namespace<A>>, sid: Sid, parser: Parser) -> Self {
        Self {
            timeout: None,
            ns,
            parser,
            opts: BroadcastOptions {
                sid: Some(sid),
                ..Default::default()
            },
        }
    }

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
    ///             .emit("test", &data);
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
    ///             .emit("test", &data);
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
    ///         socket.broadcast().except("room1").emit("test", &data);
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
    ///         socket.local().emit("test", &data);
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
    ///         socket.broadcast().emit("test", &data);
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
    /// # use futures_util::stream::StreamExt;
    /// # use std::time::Duration;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///    socket.on("test", |socket: SocketRef, Data::<Value>(data)| async move {
    ///       // Emit a test message in the room1 and room3 rooms, except for the room2
    ///       // room with the binary payload received, wait for 5 seconds for an acknowledgement
    ///       socket.to("room1")
    ///             .to("room3")
    ///             .except("room2")
    ///             .timeout(Duration::from_secs(5))
    ///             .emit_with_ack::<_, Value>("message-back", &data)
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

// ==== impl BroadcastOperators consume fns ====
impl<A: Adapter> BroadcastOperators<A> {
    /// Emits a message to all sockets selected with the previous operators.
    ///
    /// If you provide tuple-like data (tuple, arrays), it will be considered as multiple arguments.
    /// Therefore if you want to send an array as the _first_ argument of the payload,
    /// you need to wrap it in an array or a tuple. [`Vec`] will be always considered as a single argument though.
    ///
    /// ## Emitting binary data
    /// To emit binary data, you must use a data type that implements [`Serialize`] as binary data.
    /// Currently if you use `Vec<u8>` it will be considered as a number sequence and not binary data.
    /// To counter that you must either use a special type like [`Bytes`] or use the [`serde_bytes`] crate.
    /// If you want to emit generic data that may contains binary, use [`rmpv::Value`] rather
    /// than [`serde_json::Value`] otherwise the binary data will also be serialized as a number sequence.
    ///
    /// ## Errors
    /// * When encoding the data a [`BroadcastError::Serialize`] may be returned.
    /// * If the underlying engine.io connection is closed for a given socket a [`BroadcastError::Socket(SocketError::Closed)`]
    ///   will be returned.
    /// * If the packet buffer is full for a given socket, a [`BroadcastError::Socket(SocketError::InternalChannelFull)`]
    ///   will be retured. See [`SocketIoBuilder::max_buffer_size`] option for more infos on internal buffer config
    ///
    /// > **Note**: If a error is returned because of a specific socket, the message will still be sent to all other sockets.
    ///
    /// [`SocketIoBuilder::max_buffer_size`]: crate::SocketIoBuilder#method.max_buffer_size
    /// [`BroadcastError::Socket(SocketError::Closed)`]: crate::SocketError::Closed
    /// [`BroadcastError::Socket(SocketError::InternalChannelFull)`]: crate::SocketError::InternalChannelFull
    /// [`Bytes`]: bytes::Bytes
    /// [`serde_bytes`]: https://docs.rs/serde_bytes
    /// [`rmpv::Value`]: https://docs.rs/rmpv
    /// [`serde_json::Value`]: https://docs.rs/serde_json/latest/serde_json/value
    ///
    /// #### Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::*};
    /// # use serde_json::Value;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///     socket.on("test", |socket: SocketRef, Data::<Value>(data)| async move {
    ///         // Emit a test message in the room1 and room3 rooms, except for the room2 room with the binary payload received
    ///         socket.to("room1").to("room3").except("room2").emit("test", &data);
    ///
    ///         // Emit a test message with multiple arguments to the client
    ///         socket.to("room1").emit("test", &("world", "hello", 1)).ok();
    ///
    ///         // Emit a test message with an array as the first argument
    ///         let arr = [1, 2, 3, 4];
    ///         socket.to("room2").emit("test", &[arr]).ok();
    ///     });
    /// });
    /// ```
    ///
    /// ## Binary Example with the `bytes` crate
    /// ```
    /// # use socketioxide::{SocketIo, extract::*};
    /// # use serde_json::Value;
    /// # use std::sync::Arc;
    /// # use bytes::Bytes;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///     socket.on("test", |socket: SocketRef, Data::<(String, Bytes, Bytes)>(data)| async move {
    ///         // Emit a test message to the client
    ///         socket.emit("test", &data).ok();
    ///
    ///         // Emit a test message with multiple arguments to the client
    ///         socket.emit("test", &("world", "hello", Bytes::from_static(&[1, 2, 3, 4]))).ok();
    ///
    ///         // Emit a test message with an array as the first argument
    ///         let arr = [1, 2, 3, 4];
    ///         socket.emit("test", &[arr]).ok();
    ///     });
    /// });
    /// ```
    pub fn emit<T: ?Sized + Serialize>(
        mut self,
        event: impl AsRef<str>,
        data: &T,
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
    /// See [`emit()`](#method.emit) for more details on emitting messages.
    ///
    /// The acknowledgement has a timeout specified in the config (5s by default)
    /// (see [`SocketIoBuilder::ack_timeout`](crate::SocketIoBuilder)) or with the [`timeout()`] operator.
    ///
    /// To get acknowledgements, an [`AckStream`] is returned.
    /// It can be used in two ways:
    /// * As a [`Stream`]: It will yield all the ack responses with their corresponding socket id
    ///   received from the client. It can useful when broadcasting to multiple sockets and therefore expecting
    ///   more than one acknowledgement. If you want to get the socket from this id, use [`io::get_socket()`].
    /// * As a [`Future`]: It will yield the first ack response received from the client.
    ///   Useful when expecting only one acknowledgement.
    ///
    /// If the packet encoding failed an [`EncodeError`] is **immediately** returned.
    ///
    /// If the socket is full or if it has been closed before receiving the acknowledgement,
    /// an [`AckError::Socket`] will be yielded.
    ///
    /// If the client didn't respond before the timeout, the [`AckStream`] will yield
    /// an [`AckError::Timeout`]. If the data sent by the client is not deserializable as `V`,
    /// an [`AckError::Decode`] will be yielded.
    ///
    /// [`timeout()`]: #method.timeout
    /// [`Stream`]: futures_core::stream::Stream
    /// [`Future`]: futures_core::future::Future
    /// [`AckResponse`]: crate::ack::AckResponse
    /// [`AckError::Decode`]: crate::AckError::Decode
    /// [`AckError::Timeout`]: crate::AckError::Timeout
    /// [`AckError::Socket`]: crate::AckError::Socket
    /// [`AckError::Socket(SocketError::Closed)`]: crate::SocketError::Closed
    /// [`EncodeError`]: crate::EncodeError
    /// [`io::get_socket()`]: crate::SocketIo#method.get_socket
    ///
    /// # Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::*};
    /// # use serde_json::Value;
    /// # use futures_util::stream::StreamExt;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///     socket.on("test", |socket: SocketRef, Data::<Value>(data)| async move {
    ///         // Emit a test message in the room1 and room3 rooms,
    ///         // except for the room2 room with the binary payload received
    ///         let ack_stream = socket.to("room1")
    ///             .to("room3")
    ///             .except("room2")
    ///             .emit_with_ack::<_, String>("message-back", &data)
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
    pub fn emit_with_ack<T: ?Sized + Serialize, V>(
        mut self,
        event: impl AsRef<str>,
        data: &T,
    ) -> Result<AckStream<V>, parser::EncodeError> {
        let packet = self.get_packet(event, data)?;
        let stream = self
            .ns
            .adapter
            .broadcast_with_ack(packet, self.opts, self.timeout);
        Ok(AckStream::new(stream, self.parser))
    }

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

    /// Creates a packet with the given event and data.
    fn get_packet<T: ?Sized + Serialize>(
        &mut self,
        event: impl AsRef<str>,
        data: &T,
    ) -> Result<Packet, parser::EncodeError> {
        let ns = self.ns.path.clone();
        let data = self.parser.encode_value(data, Some(event.as_ref()))?;
        Ok(Packet::event(ns, data))
    }
}
