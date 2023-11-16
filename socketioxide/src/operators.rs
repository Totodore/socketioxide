//! [`Operators`] are used to select sockets to send a packet to, or to configure the packet that will be emitted.
//! It uses the builder pattern to chain operators.
use std::borrow::Cow;
use std::{sync::Arc, time::Duration};

use engineioxide::sid::Sid;
use futures::stream::BoxStream;
use serde::de::DeserializeOwned;

use crate::adapter::LocalAdapter;
use crate::errors::BroadcastError;
use crate::extract::SocketRef;
use crate::socket::AckResponse;
use crate::{
    adapter::{Adapter, BroadcastFlags, BroadcastOptions, Room},
    errors::AckError,
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

/// Operators are used to select sockets to send a packet to, or to configure the packet that will be emitted.
#[derive(Debug)]
pub struct Operators<A: Adapter = LocalAdapter> {
    opts: BroadcastOptions,
    ns: Arc<Namespace<A>>,
    binary: Vec<Vec<u8>>,
}

impl<A: Adapter> Operators<A> {
    pub(crate) fn new(ns: Arc<Namespace<A>>, sid: Option<Sid>) -> Self {
        Self {
            opts: BroadcastOptions::new(sid),
            ns,
            binary: vec![],
        }
    }

    /// Select all sockets in the given rooms except the current socket.
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
        self.opts.flags.insert(BroadcastFlags::Broadcast);
        self
    }

    /// Select all sockets in the given rooms.
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

    /// Filter out all sockets selected with the previous operators which are in the given rooms.
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
        self.opts.flags.insert(BroadcastFlags::Broadcast);
        self
    }

    /// Broadcast to all sockets only connected on this node (when using multiple nodes).
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

    /// Broadcast to all sockets without any filtering (except the current socket).
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

    /// Set a custom timeout when sending a message with an acknowledgement.
    ///
    /// #### Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::*};
    /// # use serde_json::Value;
    /// # use futures::stream::StreamExt;
    /// # use std::time::Duration;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///    socket.on("test", |socket: SocketRef, Data::<Value>(data), Bin(bin)| async move {
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
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.opts.flags.insert(BroadcastFlags::Timeout(timeout));
        self
    }

    /// Add a binary payload to the message.
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
    pub fn bin(mut self, binary: Vec<Vec<u8>>) -> Self {
        self.binary = binary;
        self
    }

    /// Emit a message to all sockets selected with the previous operators.
    /// #### Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::*};
    /// # use serde_json::Value;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///     socket.on("test", |socket: SocketRef, Data::<Value>(data), Bin(bin)| async move {
    ///         // Emit a test message in the room1 and room3 rooms, except for the room2 room with the binary payload received
    ///         socket.to("room1").to("room3").except("room2").bin(bin).emit("test", data);
    ///     });
    /// });
    pub fn emit(
        mut self,
        event: impl Into<Cow<'static, str>>,
        data: impl serde::Serialize,
    ) -> Result<(), BroadcastError> {
        let packet = self.get_packet(event, data)?;
        if let Err(e) = self.ns.adapter.broadcast(packet, self.opts) {
            #[cfg(feature = "tracing")]
            tracing::debug!("broadcast error: {e:?}");
            return Err(e);
        }
        Ok(())
    }

    /// Emit a message to all sockets selected with the previous operators and return a stream of acknowledgements.
    ///
    /// Each acknowledgement has a timeout specified in the config (5s by default) or with the `timeout()` operator.
    /// #### Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::*};
    /// # use serde_json::Value;
    /// # use futures::stream::StreamExt;
    /// let (_, io) = SocketIo::new_svc();
    /// io.ns("/", |socket: SocketRef| {
    ///    socket.on("test", |socket: SocketRef, Data::<Value>(data), Bin(bin)| async move {
    ///       // Emit a test message in the room1 and room3 rooms, except for the room2 room with the binary payload received
    ///       socket.to("room1")
    ///             .to("room3")
    ///             .except("room2")
    ///             .bin(bin)
    ///             .emit_with_ack::<Value>("message-back", data).unwrap().for_each(|ack| async move {
    ///                match ack {
    ///                    Ok(ack) => println!("Ack received {:?}", ack),
    ///                    Err(err) => println!("Ack error {:?}", err),
    ///                }
    ///             }).await;
    ///    });
    /// });
    pub fn emit_with_ack<V: DeserializeOwned + Send>(
        mut self,
        event: impl Into<Cow<'static, str>>,
        data: impl serde::Serialize,
    ) -> Result<BoxStream<'static, Result<AckResponse<V>, AckError>>, BroadcastError> {
        let packet = self.get_packet(event, data)?;
        self.ns.adapter.broadcast_with_ack(packet, self.opts)
    }

    /// Get all sockets selected with the previous operators.
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

    /// Disconnect all sockets selected with the previous operators.
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
    pub fn disconnect(self) -> Result<(), BroadcastError> {
        self.ns.adapter.disconnect_socket(self.opts)
    }

    /// Make all sockets selected with the previous operators join the given room(s).
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

    /// Make all sockets selected with the previous operators leave the given room(s).
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

    /// Create a packet with the given event and data.
    fn get_packet(
        &mut self,
        event: impl Into<Cow<'static, str>>,
        data: impl serde::Serialize,
    ) -> Result<Packet<'static>, serde_json::Error> {
        let ns = self.ns.path.clone();
        let data = serde_json::to_value(data)?;
        let packet = if self.binary.is_empty() {
            Packet::event(ns, event.into(), data)
        } else {
            let binary = std::mem::take(&mut self.binary);
            Packet::bin_event(ns, event.into(), data, binary)
        };
        Ok(packet)
    }
}

#[cfg(feature = "test-utils")]
impl<A: Adapter> Operators<A> {
    #[allow(dead_code)]
    pub(crate) fn is_broadcast(&self) -> bool {
        self.opts.flags.contains(&BroadcastFlags::Broadcast)
    }
}
