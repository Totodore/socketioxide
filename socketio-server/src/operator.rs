use std::{pin::Pin, sync::Arc, time::Duration};

use futures::Stream;
use itertools::Itertools;
use serde::{de::DeserializeOwned, Serialize};

use crate::{
    adapter::{Adapter, BroadcastFlags, BroadcastOptions, Room},
    errors::{AckError, Error},
    ns::Namespace,
    packet::Packet,
    socket::AckResponse,
};

pub struct BroadcastOperator<A: Adapter> {
    opts: BroadcastOptions,
    ns: Arc<Namespace<A>>,
    binary: Option<Vec<Vec<u8>>>,
}
/// A trait for types that can be used as a room parameter.
/// ```
/// use socketio_server::operator::RoomParam;
/// use std::collections::HashSet;
///
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
impl<A: Adapter> BroadcastOperator<A> {
    pub fn new(ns: Arc<Namespace<A>>, sid: i64) -> Self {
        Self {
            opts: BroadcastOptions {
                sid,
                ..Default::default()
            },
            ns,
            binary: None,
        }
    }

    pub fn to(self, rooms: impl RoomParam) -> Self {
        let mut curr_rooms = self.opts.rooms;
        curr_rooms.extend(rooms.into_room_iter().unique());
        let mut flags = self.opts.flags;
        flags.insert(BroadcastFlags::Broadcast);
        Self {
            opts: BroadcastOptions {
                rooms: curr_rooms,
                flags,
                ..self.opts
            },
            ..self
        }
    }

    pub fn except(self, rooms: impl RoomParam) -> Self {
        let mut curr_rooms = self.opts.except;
        curr_rooms.extend(rooms.into_room_iter().unique());
        let mut flags = self.opts.flags;
        flags.insert(BroadcastFlags::Broadcast);
        Self {
            opts: BroadcastOptions {
                except: curr_rooms,
                flags,
                ..self.opts
            },
            ..self
        }
    }

    pub fn local(self) -> Self {
        let mut flags = self.opts.flags;
        flags.insert(BroadcastFlags::Local);
        Self {
            opts: BroadcastOptions { flags, ..self.opts },
            ..self
        }
    }

    pub fn broadcast(self) -> Self {
        let mut flags = self.opts.flags;
        flags.insert(BroadcastFlags::Broadcast);
        Self {
            opts: BroadcastOptions { flags, ..self.opts },
            ..self
        }
    }

    pub fn timeout(self, timeout: Duration) -> Self {
        let mut flags = self.opts.flags;
        flags.insert(BroadcastFlags::Timeout(timeout));
        Self {
            opts: BroadcastOptions { flags, ..self.opts },
            ..self
        }
    }
    pub fn bin(self, binary: Vec<Vec<u8>>) -> Self {
        Self {
            binary: Some(binary),
            ..self
        }
    }

    pub fn emit(self, event: impl Into<String>, data: impl serde::Serialize) -> Result<(), Error> {
        let packet = self.get_packet(event, data)?;
        self.ns.adapter.broadcast(packet, self.binary, self.opts)
    }

    pub fn emit_with_ack<V: DeserializeOwned>(
        self,
        event: impl Into<String>,
        data: impl serde::Serialize,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<AckResponse<V>, AckError>>>>, Error> {
        let packet = self.get_packet(event, data)?;
        Ok(self.ns
            .adapter
            .broadcast_with_ack(packet, self.binary, self.opts))

    }

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
