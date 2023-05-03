use std::{
    collections::{HashMap, HashSet},
    pin::Pin,
    sync::{Arc, RwLock, Weak},
};

use crate::{
    adapter::BroadcastFlags,
    errors::{AckError, Error},
    ns::Namespace,
    packet::Packet,
    socket::{AckResponse, Socket},
};
use futures::{stream, Stream, StreamExt, FutureExt};
use itertools::Itertools;
use serde::de::DeserializeOwned;

use super::{Adapter, BroadcastOptions, Room, RoomParam, adapter::AdapterFuture};

pub struct LocalAdapter {
    rooms: RwLock<HashMap<String, HashSet<i64>>>,
    ns: Weak<Namespace<Self>>,
}

#[cfg(not(feature = "remote_adapter"))]
impl Adapter for LocalAdapter {
    fn new(ns: Weak<Namespace<Self>>) -> Self {
        Self {
            rooms: HashMap::new().into(),
            ns,
        }
    }

    fn init(&self) {}

    fn close(&self) {}

    fn server_count(&self) -> u16 {
        1
    }

    fn add_all(&self, sid: i64, rooms: impl RoomParam) {
        let mut rooms_map = self.rooms.write().unwrap();
        for room in rooms.into_room_iter() {
            rooms_map
                .entry(room)
                .or_insert_with(HashSet::new)
                .insert(sid);
        }
    }

    fn del(&self, sid: i64, rooms: impl RoomParam) {
        let mut rooms_map = self.rooms.write().unwrap();
        for room in rooms.into_room_iter() {
            if let Some(room) = rooms_map.get_mut(&room) {
                room.remove(&sid);
            }
        }
    }

    fn del_all(&self, sid: i64) {
        let mut rooms_map = self.rooms.write().unwrap();
        for room in rooms_map.values_mut() {
            room.remove(&sid);
        }
    }

    fn broadcast(
        &self,
        packet: Packet,
        binary: Option<Vec<Vec<u8>>>,
        opts: BroadcastOptions,
    ) -> Result<(), Error> {
        let sockets = self.apply_opts(opts);

        tracing::debug!("broadcasting packet to {} sockets", sockets.len());
        sockets
            .into_iter()
            .map(|socket| socket.send(packet.clone(), binary.clone()))
            .collect::<Result<(), Error>>()
    }

    fn broadcast_with_ack<V: DeserializeOwned>(
        &self,
        packet: Packet,
        binary: Option<Vec<Vec<u8>>>,
        opts: BroadcastOptions,
    ) -> Pin<Box<dyn Stream<Item = Result<AckResponse<V>, AckError>>>> {
        let duration = opts.flags.iter().find_map(|flag| match flag {
            BroadcastFlags::Timeout(duration) => Some(*duration),
            _ => None,
        });
        let sockets = self.apply_opts(opts);
        tracing::debug!(
            "broadcasting packet to {} sockets: {:?}",
            sockets.len(),
            sockets.iter().map(|s| s.sid).collect::<Vec<_>>()
        );
        let count = sockets.len();
        let ack_futs = sockets.into_iter().map(move |socket| {
            let packet = packet.clone();
            let binary = binary.clone();
            async move { socket.clone().send_with_ack(packet, binary, duration).await }
        });
        stream::iter(ack_futs).buffer_unordered(count).boxed()
    }

    fn sockets(&self, rooms: impl RoomParam) -> Vec<i64> {
        // TODO: fix this depending on the utilisation of the function
        let mut opts = BroadcastOptions::new(0);
        opts.rooms.extend(rooms.into_room_iter());
        self.apply_opts(opts)
            .into_iter()
            .map(|socket| socket.sid)
            .collect()
    }

    //TODO: make this operation O(1)
    fn socket_rooms(&self, sid: i64) -> Vec<Room> {
        let rooms_map = self.rooms.read().unwrap();
        rooms_map
            .iter()
            .filter(|(_, sockets)| sockets.contains(&sid))
            .map(|(room, _)| room.clone())
            .collect()
    }

    fn fetch_sockets(&self, opts: BroadcastOptions) -> Vec<Arc<Socket<Self>>> {
        self.apply_opts(opts)
    }

    fn add_sockets(&self, opts: BroadcastOptions, rooms: impl RoomParam) {
        let rooms: Vec<Room> = rooms.into_room_iter().collect();
        for socket in self.apply_opts(opts) {
            self.add_all(socket.sid, rooms.clone());
        }
    }

    fn del_sockets(&self, opts: BroadcastOptions, rooms: impl RoomParam) {
        let rooms: Vec<Room> = rooms.into_room_iter().collect();
        for socket in self.apply_opts(opts) {
            self.del(socket.sid, rooms.clone());
        }
    }

    fn disconnect_socket(&self, opts: BroadcastOptions) -> Result<(), Error> {
        self.apply_opts(opts)
            .into_iter()
            .map(|socket| socket.disconnect())
            .collect::<Result<(), Error>>()
    }
}

#[cfg(feature = "remote_adapter")]
impl Adapter for LocalAdapter {
    fn new(ns: Weak<Namespace<Self>>) -> Self {
        Self {
            rooms: HashMap::new().into(),
            ns,
        }
    }

    fn init(&self) -> AdapterFuture { async move { Ok(()) }.boxed() }

    fn close(&self) -> AdapterFuture {}

    fn server_count(&self) -> u16 {
        1
    }

    fn add_all(&self, sid: i64, rooms: impl RoomParam) {
        let mut rooms_map = self.rooms.write().unwrap();
        for room in rooms.into_room_iter() {
            rooms_map
                .entry(room)
                .or_insert_with(HashSet::new)
                .insert(sid);
        }
    }

    fn del(&self, sid: i64, rooms: impl RoomParam) {
        let mut rooms_map = self.rooms.write().unwrap();
        for room in rooms.into_room_iter() {
            if let Some(room) = rooms_map.get_mut(&room) {
                room.remove(&sid);
            }
        }
    }

    fn del_all(&self, sid: i64) {
        let mut rooms_map = self.rooms.write().unwrap();
        for room in rooms_map.values_mut() {
            room.remove(&sid);
        }
    }

    fn broadcast(
        &self,
        packet: Packet,
        binary: Option<Vec<Vec<u8>>>,
        opts: BroadcastOptions,
    ) -> Result<(), Error> {
        let sockets = self.apply_opts(opts);

        tracing::debug!("broadcasting packet to {} sockets", sockets.len());
        sockets
            .into_iter()
            .map(|socket| socket.send(packet.clone(), binary.clone()))
            .collect::<Result<(), Error>>()
    }

    fn broadcast_with_ack<V: DeserializeOwned>(
        &self,
        packet: Packet,
        binary: Option<Vec<Vec<u8>>>,
        opts: BroadcastOptions,
    ) -> Pin<Box<dyn Stream<Item = Result<AckResponse<V>, AckError>>>> {
        let duration = opts.flags.iter().find_map(|flag| match flag {
            BroadcastFlags::Timeout(duration) => Some(*duration),
            _ => None,
        });
        let sockets = self.apply_opts(opts);
        tracing::debug!(
            "broadcasting packet to {} sockets: {:?}",
            sockets.len(),
            sockets.iter().map(|s| s.sid).collect::<Vec<_>>()
        );
        let count = sockets.len();
        let ack_futs = sockets.into_iter().map(move |socket| {
            let packet = packet.clone();
            let binary = binary.clone();
            async move { socket.clone().send_with_ack(packet, binary, duration).await }
        });
        stream::iter(ack_futs).buffer_unordered(count).boxed()
    }

    fn sockets(&self, rooms: impl RoomParam) -> Vec<i64> {
        // TODO: fix this depending on the utilisation of the function
        let mut opts = BroadcastOptions::new(0);
        opts.rooms.extend(rooms.into_room_iter());
        self.apply_opts(opts)
            .into_iter()
            .map(|socket| socket.sid)
            .collect()
    }

    //TODO: make this operation O(1)
    fn socket_rooms(&self, sid: i64) -> Vec<Room> {
        let rooms_map = self.rooms.read().unwrap();
        rooms_map
            .iter()
            .filter(|(_, sockets)| sockets.contains(&sid))
            .map(|(room, _)| room.clone())
            .collect()
    }

    fn fetch_sockets(&self, opts: BroadcastOptions) -> Vec<Arc<Socket<Self>>> {
        self.apply_opts(opts)
    }

    fn add_sockets(&self, opts: BroadcastOptions, rooms: impl RoomParam) {
        let rooms: Vec<Room> = rooms.into_room_iter().collect();
        for socket in self.apply_opts(opts) {
            self.add_all(socket.sid, rooms.clone());
        }
    }

    fn del_sockets(&self, opts: BroadcastOptions, rooms: impl RoomParam) {
        let rooms: Vec<Room> = rooms.into_room_iter().collect();
        for socket in self.apply_opts(opts) {
            self.del(socket.sid, rooms.clone());
        }
    }

    fn disconnect_socket(&self, opts: BroadcastOptions) -> Result<(), Error> {
        self.apply_opts(opts)
            .into_iter()
            .map(|socket| socket.disconnect())
            .collect::<Result<(), Error>>()
    }
}

impl LocalAdapter {
    /// Apply the given `opts` and return the sockets that match.
    fn apply_opts(&self, opts: BroadcastOptions) -> Vec<Arc<Socket<Self>>> {
        let rooms = opts.rooms;

        let except = self.get_except_sids(&opts.except);
        let ns = self.ns.upgrade().unwrap();
        if rooms.len() > 0 {
            let rooms_map = self.rooms.read().unwrap();
            rooms_map
                .iter()
                .filter(|(room, _)| rooms.contains(room))
                .flat_map(|(_, sockets)| sockets)
                .filter(|sid| {
                    !except.contains(*sid)
                        && (opts.flags.contains(&BroadcastFlags::Broadcast) && **sid != opts.sid)
                })
                .unique()
                .map(|sid| ns.get_socket(*sid))
                .filter(Option::is_some)
                .map(Option::unwrap)
                .collect()
        } else if opts.flags.contains(&BroadcastFlags::Broadcast) {
            let sockets = ns.get_sockets();
            sockets
                .into_iter()
                .filter(|socket| !except.contains(&socket.sid))
                .collect()
        } else if let Some(sock) = ns.get_socket(opts.sid) {
            vec![sock]
        } else {
            vec![]
        }
    }

    fn get_except_sids(&self, except: &Vec<Room>) -> HashSet<i64> {
        let mut except_sids = HashSet::new();
        let rooms_map = self.rooms.read().unwrap();
        for room in except {
            if let Some(sockets) = rooms_map.get(room) {
                except_sids.extend(sockets);
            }
        }
        except_sids
    }
}
