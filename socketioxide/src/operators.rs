use std::{sync::Arc, time::Duration};

use futures_core::stream::BoxStream;
use itertools::Itertools;
use serde::{de::DeserializeOwned, Serialize};

use crate::{
    adapter::{Adapter, BroadcastFlags, BroadcastOptions, Room},
    errors::{AckError, Error},
    ns::Namespace,
    packet::Packet,
    socket::AckResponse,
};

/// A trait for types that can be used as a room parameter.
pub trait RoomParam: 'static {
    type IntoIter: Iterator<Item = Room>;
    fn into_room_iter(self) -> Self::IntoIter;
}

impl RoomParam for Room {
    type IntoIter = std::iter::Once<Room>;
    fn into_room_iter(self) -> Self::IntoIter {
        std::iter::once(self)
    }
}
impl RoomParam for Vec<Room> {
    type IntoIter = std::vec::IntoIter<Room>;
    fn into_room_iter(self) -> Self::IntoIter {
        self.into_iter()
    }
}
impl RoomParam for &'static str {
    type IntoIter = std::iter::Once<Room>;
    fn into_room_iter(self) -> Self::IntoIter {
        std::iter::once(self.to_string())
    }
}
impl<const COUNT: usize> RoomParam for [&'static str; COUNT] {
    type IntoIter =
        std::iter::Map<std::array::IntoIter<&'static str, COUNT>, fn(&'static str) -> Room>;

    fn into_room_iter(self) -> Self::IntoIter {
        self.into_iter().map(|s| s.to_string().to_string())
    }
}

/// Operators are used to select clients to send a packet to, or to configure the packet that will be emitted.
pub struct Operators<A: Adapter> {
    opts: BroadcastOptions,
    ns: Arc<Namespace<A>>,
    binary: Option<Vec<Vec<u8>>>,
}

impl<A: Adapter> Operators<A> {
    pub(crate) fn new(ns: Arc<Namespace<A>>, sid: i64) -> Self {
        Self {
            opts: BroadcastOptions::new(sid),
            ns,
            binary: None,
        }
    }

    /// Select all clients in the given rooms except the current socket.
    /// ## Example :
    /// ```
    /// use socketioxide::{Namespace, Ack};
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
    pub fn to(mut self, rooms: impl RoomParam) -> Self {
        self.opts.rooms.extend(rooms.into_room_iter().unique());
        self.opts.flags.insert(BroadcastFlags::Broadcast);
        self
    }

    /// Filter out all clients selected with the previous operators which are in the given rooms.
    /// ## Example :
    /// ```
    /// use socketioxide::{Namespace, Ack};
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
    pub fn except(mut self, rooms: impl RoomParam) -> Self {
        self.opts.except.extend(rooms.into_room_iter().unique());
        self.opts.flags.insert(BroadcastFlags::Broadcast);
        self
    }

    /// Broadcast to all clients only connected on this node (when using multiple nodes).
    /// When using the default in-memory adapter, this operator is a no-op.
    /// ## Example :
    /// ```
    /// use socketioxide::{Namespace, Ack};
    /// use serde_json::Value;
    /// Namespace::builder().add("/", |socket| async move {
    ///     socket.on("test", |socket, data: Value, _| async move {
    ///         // This message will be broadcast to all clients in this namespace and connected on this node
    ///         socket.local().emit("test", data);
    ///         Ok(Ack::<()>::None)
    ///     });
    /// });
    pub fn local(mut self) -> Self {
        self.opts.flags.insert(BroadcastFlags::Local);
        self
    }

    /// Broadcast to all clients without any filtering (except the current socket).
    /// ## Example :
    /// ```
    /// use socketioxide::{Namespace, Ack};
    /// use serde_json::Value;
    /// Namespace::builder().add("/", |socket| async move {
    ///     socket.on("test", |socket, data: Value, _| async move {
    ///         // This message will be broadcast to all clients in this namespace
    ///         socket.broadcast().emit("test", data);
    ///         Ok(Ack::<()>::None)
    ///     });
    /// });
    pub fn broadcast(mut self) -> Self {
        self.opts.flags.insert(BroadcastFlags::Broadcast);
        self
    }

    /// Set a custom timeout when sending a message with an acknowledgement.
    ///
    /// ## Example :
    /// ```
    /// use socketioxide::{Namespace, Ack};
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
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.opts.flags.insert(BroadcastFlags::Timeout(timeout));
        self
    }

    /// Add a binary payload to the message.
    /// ## Example :
    /// ```
    /// use socketioxide::{Namespace, Ack};
    /// use serde_json::Value;
    /// Namespace::builder().add("/", |socket| async move {
    ///     socket.on("test", |socket, data: Value, bin| async move {
    ///         // This will send the binary payload received to all clients in this namespace with the test message
    ///         socket.bin(bin.unwrap()).emit("test", data);
    ///         Ok(Ack::<()>::None)
    ///     });
    /// });
    pub fn bin(mut self, binary: Vec<Vec<u8>>) -> Self {
        self.binary = Some(binary);
        self
    }

    /// Emit a message to all clients selected with the previous operators.
    /// ## Example :
    /// ```
    /// use socketioxide::{Namespace, Ack};
    /// use serde_json::Value;
    /// Namespace::builder().add("/", |socket| async move {
    ///     socket.on("test", |socket, data: Value, bin| async move {
    ///         // Emit a test message in the room1 and room3 rooms, except for the room2 room with the binary payload received
    ///         socket.to("room1").to("room3").except("room2").bin(bin.unwrap()).emit("test", data);
    ///         Ok(Ack::<()>::None)
    ///     });
    /// });
    pub fn emit(self, event: impl Into<String>, data: impl serde::Serialize) -> Result<(), Error> {
        let packet = self.get_packet(event, data)?;
        self.ns.adapter.broadcast(packet, self.binary, self.opts)
    }

    /// Emit a message to all clients selected with the previous operators and return a stream of acknowledgements.
    /// 
    /// Each acknowledgement has a timeout specified in the config (5s by default) or with the `timeout()` operator.
    /// ## Example :
    /// ```
    /// use socketioxide::{Namespace, Ack};
    /// use serde_json::Value;
    /// use futures::stream::StreamExt;
    /// Namespace::builder().add("/", |socket| async move {
    ///    socket.on("test", |socket, data: Value, bin| async move {
    ///       // Emit a test message in the room1 and room3 rooms, except for the room2 room with the binary payload received
    ///       socket.to("room1")
    ///             .to("room3")
    ///             .except("room2")
    ///             .bin(bin.unwrap())
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
    pub fn emit_with_ack<V: DeserializeOwned + Send>(
        self,
        event: impl Into<String>,
        data: impl serde::Serialize,
    ) -> Result<BoxStream<'static, Result<AckResponse<V>, AckError>>, Error> {
        let packet = self.get_packet(event, data)?;
        Ok(self
            .ns
            .adapter
            .broadcast_with_ack(packet, self.binary, self.opts))
    }

    /// Create a packet with the given event and data.
    fn get_packet(&self, event: impl Into<String>, data: impl Serialize) -> Result<Packet, Error> {
        let ns = self.ns.clone();
        let data = serde_json::to_value(data)?;
        let packet = if let Some(ref bin) = self.binary {
            Packet::bin_event(ns.path.clone(), event.into(), data, bin.len())
        } else {
            Packet::event(ns.path.clone(), event.into(), data)
        };
        Ok(packet)
    }
}
