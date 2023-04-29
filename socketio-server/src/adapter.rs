use std::{
    collections::{HashMap, HashSet},
    sync::{RwLock, Weak},
};

use engineio_server::async_trait;
use futures::Stream;

use crate::{ns::Namespace, packet::Packet};

pub struct Room(String);

#[repr(u8)]
enum BroadcastFlags {
    Volatile = 0b1,
    Compress = 0b1 << 1,
    Local = 0b1 << 2,
    Broadcast = 0b1 << 3,
    Binary = 0b1 << 4,
}
pub struct BroadcastOptions {
    flags: u8,
    rooms: Vec<String>,
    except: Vec<i64>,
}

#[async_trait]
pub trait Adapter: Send + Sync + 'static {
    fn new(ns: Weak<Namespace<Self>>) -> Self
    where
        Self: Sized;
    async fn init(&self);
    async fn close(&self);

    async fn server_count(&self) -> u16;

    async fn add_all(&self, sid: i64, rooms: Vec<String>);
    async fn del(&self, sid: i64, rooms: Vec<String>);
    async fn del_all(&self, sid: i64);

    fn broadcast(&self, packet: Packet, opts: BroadcastOptions);

    fn broadcast_with_ack(
        &self,
        packet: Packet,
        opts: BroadcastOptions,
    ) -> Box<dyn Stream<Item = Packet>>;

    async fn sockets(&self, rooms: Vec<Room>) -> Vec<i64>;
    async fn socket_rooms(&self, sid: i64) -> Vec<String>;

    async fn fetch_sockets(&self, opts: BroadcastOptions) -> Vec<i64>;
    fn add_sockets(&self, opts: BroadcastOptions, rooms: Vec<String>);
    fn del_sockets(&self, opts: BroadcastOptions, rooms: Vec<String>);
    fn disconnect_socket(&self, opts: BroadcastOptions, close: bool);
}

pub struct LocalAdapter {
    rooms: RwLock<HashMap<String, HashSet<i64>>>,
    ns: Weak<Namespace<Self>>,
}

#[async_trait]
impl Adapter for LocalAdapter {
    fn new(ns: Weak<Namespace<Self>>) -> Self {
        Self {
            rooms: HashMap::new().into(),
            ns,
        }
    }

    async fn init(&self) {}

    async fn close(&self) {}

    async fn server_count(&self) -> u16 {
        1
    }

    async fn add_all(&self, sid: i64, rooms: Vec<String>) {
        let mut rooms_map = self.rooms.write().unwrap();
        for room in rooms {
            rooms_map
                .entry(room)
                .or_insert_with(HashSet::new)
                .insert(sid);
        }
    }

    async fn del(&self, sid: i64, rooms: Vec<String>) {
        let mut rooms_map = self.rooms.write().unwrap();
        for room in rooms {
            if let Some(room) = rooms_map.get_mut(&room) {
                room.remove(&sid);
            }
        }
    }

    async fn del_all(&self, sid: i64) {
        let mut rooms_map = self.rooms.write().unwrap();
        for room in rooms_map.values_mut() {
            room.remove(&sid);
        }
    }

    fn broadcast(&self, packet: Packet, opts: BroadcastOptions) {
        // let rooms_map = self.rooms.read().unwrap();
        todo!()
    }

    fn broadcast_with_ack(
        &self,
        packet: Packet,
        opts: BroadcastOptions,
    ) -> Box<dyn Stream<Item = Packet>> {
        todo!()
    }

    async fn sockets(&self, rooms: Vec<Room>) -> Vec<i64> {
        todo!()
    }

    async fn socket_rooms(&self, sid: i64) -> Vec<String> {
        todo!()
    }

    async fn fetch_sockets(&self, opts: BroadcastOptions) -> Vec<i64> {
        todo!()
    }

    fn add_sockets(&self, opts: BroadcastOptions, rooms: Vec<String>) {
        todo!()
    }

    fn del_sockets(&self, opts: BroadcastOptions, rooms: Vec<String>) {
        todo!()
    }

    fn disconnect_socket(&self, opts: BroadcastOptions, close: bool) {
        todo!()
    }
}
