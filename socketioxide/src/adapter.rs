use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, RwLock, Weak},
    time::Duration,
};

use engineioxide::sid_generator::Sid;
use futures::{
    stream::{self, BoxStream},
    StreamExt,
};
use itertools::Itertools;
use serde::de::DeserializeOwned;

use crate::{
    errors::{AckError, Error},
    handler::AckResponse,
    ns::Namespace,
    operators::RoomParam,
    packet::Packet,
    socket::Socket,
};

pub type Room = String;

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum BroadcastFlags {
    Local,
    Broadcast,
    Timeout(Duration),
}
#[derive(Clone, Debug)]
pub struct BroadcastOptions {
    pub flags: HashSet<BroadcastFlags>,
    pub rooms: Vec<Room>,
    pub except: Vec<Room>,
    pub sid: Sid,
}
impl BroadcastOptions {
    pub fn new(sid: Sid) -> Self {
        Self {
            flags: HashSet::new(),
            rooms: Vec::new(),
            except: Vec::new(),
            sid,
        }
    }
}

//TODO: Make an AsyncAdapter trait
pub trait Adapter: std::fmt::Debug + Send + Sync + 'static {
    fn new(ns: Weak<Namespace<Self>>) -> Self
    where
        Self: Sized;
    fn init(&self);
    fn close(&self);

    fn server_count(&self) -> u16;

    fn add_all(&self, sid: Sid, rooms: impl RoomParam);
    fn del(&self, sid: Sid, rooms: impl RoomParam);
    fn del_all(&self, sid: Sid);

    fn broadcast(&self, packet: Packet, opts: BroadcastOptions) -> Result<(), Error>;

    fn broadcast_with_ack<V: DeserializeOwned>(
        &self,
        packet: Packet,
        opts: BroadcastOptions,
    ) -> BoxStream<'static, Result<AckResponse<V>, AckError>>;

    fn sockets(&self, rooms: impl RoomParam) -> Vec<Sid>;
    fn socket_rooms(&self, sid: Sid) -> Vec<Room>;

    fn fetch_sockets(&self, opts: BroadcastOptions) -> Vec<Arc<Socket<Self>>>
    where
        Self: Sized;
    fn add_sockets(&self, opts: BroadcastOptions, rooms: impl RoomParam);
    fn del_sockets(&self, opts: BroadcastOptions, rooms: impl RoomParam);
    fn disconnect_socket(&self, opts: BroadcastOptions) -> Result<(), Error>;

    //TODO: implement
    // fn server_side_emit(&self, packet: Packet, opts: BroadcastOptions) -> Result<u64, Error>;
    // fn persist_session(&self, sid: i64);
    // fn restore_session(&self, sid: i64) -> Session;
}

#[derive(Debug)]
pub struct LocalAdapter {
    rooms: RwLock<HashMap<Room, HashSet<Sid>>>,
    ns: Weak<Namespace<Self>>,
}

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

    fn add_all(&self, sid: Sid, rooms: impl RoomParam) {
        let mut rooms_map = self.rooms.write().unwrap();
        for room in rooms.into_room_iter() {
            rooms_map
                .entry(room)
                .or_insert_with(HashSet::new)
                .insert(sid);
        }
    }

    fn del(&self, sid: Sid, rooms: impl RoomParam) {
        let mut rooms_map = self.rooms.write().unwrap();
        for room in rooms.into_room_iter() {
            if let Some(room) = rooms_map.get_mut(&room) {
                room.remove(&sid);
            }
        }
    }

    fn del_all(&self, sid: Sid) {
        let mut rooms_map = self.rooms.write().unwrap();
        for room in rooms_map.values_mut() {
            room.remove(&sid);
        }
    }

    fn broadcast(&self, packet: Packet, opts: BroadcastOptions) -> Result<(), Error> {
        let sockets = self.apply_opts(opts);

        tracing::debug!("broadcasting packet to {} sockets", sockets.len());
        sockets
            .into_iter()
            .try_for_each(|socket| socket.send(packet.clone()))
    }

    fn broadcast_with_ack<V: DeserializeOwned>(
        &self,
        packet: Packet,
        opts: BroadcastOptions,
    ) -> BoxStream<'static, Result<AckResponse<V>, AckError>> {
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
            async move { socket.clone().send_with_ack(packet, duration).await }
        });
        stream::iter(ack_futs).buffer_unordered(count).boxed()
    }

    fn sockets(&self, rooms: impl RoomParam) -> Vec<Sid> {
        let mut opts = BroadcastOptions::new(0i64.into());
        opts.rooms.extend(rooms.into_room_iter());
        self.apply_opts(opts)
            .into_iter()
            .map(|socket| socket.sid)
            .collect()
    }

    //TODO: make this operation O(1)
    fn socket_rooms(&self, sid: Sid) -> Vec<Room> {
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
            .try_for_each(|socket| socket.disconnect())
    }
}

impl LocalAdapter {
    /// Apply the given `opts` and return the sockets that match.
    fn apply_opts(&self, opts: BroadcastOptions) -> Vec<Arc<Socket<Self>>> {
        let rooms = opts.rooms;

        let except = self.get_except_sids(&opts.except);
        let ns = self.ns.upgrade().unwrap();
        if !rooms.is_empty() {
            let rooms_map = self.rooms.read().unwrap();
            rooms
                .iter()
                .filter_map(|room| rooms_map.get(room))
                .flatten()
                .unique()
                .filter(|sid| {
                    !except.contains(*sid)
                        && (!opts.flags.contains(&BroadcastFlags::Broadcast) || **sid != opts.sid)
                })
                .filter_map(|sid| ns.get_socket(*sid).ok())
                .collect()
        } else if opts.flags.contains(&BroadcastFlags::Broadcast) {
            let sockets = ns.get_sockets();
            sockets
                .into_iter()
                .filter(|socket| !except.contains(&socket.sid))
                .collect()
        } else if let Ok(sock) = ns.get_socket(opts.sid) {
            vec![sock]
        } else {
            vec![]
        }
    }

    fn get_except_sids(&self, except: &Vec<Room>) -> HashSet<Sid> {
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

#[cfg(test)]
mod test {

    use super::*;

    #[tokio::test]
    async fn test_server_count() {
        let ns = Namespace::new_dummy([]);
        let adapter = LocalAdapter::new(Arc::downgrade(&ns));
        assert_eq!(adapter.server_count(), 1);
    }

    #[tokio::test]
    async fn test_add_all() {
        let socket: Sid = 1i64.into();
        let ns = Namespace::new_dummy([socket]);
        let adapter = LocalAdapter::new(Arc::downgrade(&ns));
        adapter.add_all(socket, ["room1", "room2"]);
        let rooms_map = adapter.rooms.read().unwrap();
        assert_eq!(rooms_map.len(), 2);
        assert_eq!(rooms_map.get("room1").unwrap().len(), 1);
        assert_eq!(rooms_map.get("room2").unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_del() {
        let socket: Sid = 1i64.into();
        let ns = Namespace::new_dummy([socket]);
        let adapter = LocalAdapter::new(Arc::downgrade(&ns));
        adapter.add_all(socket, ["room1", "room2"]);
        adapter.del(socket, "room1");
        let rooms_map = adapter.rooms.read().unwrap();
        assert_eq!(rooms_map.len(), 2);
        assert_eq!(rooms_map.get("room1").unwrap().len(), 0);
        assert_eq!(rooms_map.get("room2").unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_del_all() {
        let socket: Sid = 1i64.into();
        let ns = Namespace::new_dummy([socket]);
        let adapter = LocalAdapter::new(Arc::downgrade(&ns));
        adapter.add_all(socket, ["room1", "room2"]);
        adapter.del_all(socket);
        let rooms_map = adapter.rooms.read().unwrap();
        assert_eq!(rooms_map.len(), 2);
        assert_eq!(rooms_map.get("room1").unwrap().len(), 0);
        assert_eq!(rooms_map.get("room2").unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_socket_room() {
        let ns = Namespace::new_dummy([1i64, 2, 3].map(Into::into));
        let adapter = LocalAdapter::new(Arc::downgrade(&ns));
        adapter.add_all(1i64.into(), ["room1", "room2"]);
        adapter.add_all(2i64.into(), ["room1"]);
        adapter.add_all(3i64.into(), ["room2"]);
        assert!(adapter.socket_rooms(1i64.into()).contains(&"room1".into()));
        assert!(adapter.socket_rooms(1i64.into()).contains(&"room2".into()));
        assert_eq!(adapter.socket_rooms(2i64.into()), ["room1"]);
        assert_eq!(adapter.socket_rooms(3i64.into()), ["room2"]);
    }

    #[tokio::test]
    async fn test_add_socket() {
        let socket: Sid = 0i64.into();
        let ns = Namespace::new_dummy([socket]);
        let adapter = LocalAdapter::new(Arc::downgrade(&ns));
        adapter.add_all(socket, ["room1"]);

        let mut opts = BroadcastOptions::new(socket);
        opts.rooms = vec!["room1".to_string()];
        adapter.add_sockets(opts, "room2");
        let rooms_map = adapter.rooms.read().unwrap();

        assert_eq!(rooms_map.len(), 2);
        assert!(rooms_map.get("room1").unwrap().contains(&socket));
        assert!(rooms_map.get("room2").unwrap().contains(&socket));
    }

    #[tokio::test]
    async fn test_del_socket() {
        let socket: Sid = 0i64.into();
        let ns = Namespace::new_dummy([socket]);
        let adapter = LocalAdapter::new(Arc::downgrade(&ns));
        adapter.add_all(socket, ["room1"]);

        let mut opts = BroadcastOptions::new(socket);
        opts.rooms = vec!["room1".to_string()];
        adapter.add_sockets(opts, "room2");

        {
            let rooms_map = adapter.rooms.read().unwrap();

            assert_eq!(rooms_map.len(), 2);
            assert!(rooms_map.get("room1").unwrap().contains(&socket));
            assert!(rooms_map.get("room2").unwrap().contains(&socket));
        }

        let mut opts = BroadcastOptions::new(socket);
        opts.rooms = vec!["room1".to_string()];
        adapter.del_sockets(opts, "room2");

        {
            let rooms_map = adapter.rooms.read().unwrap();

            assert_eq!(rooms_map.len(), 2);
            assert!(rooms_map.get("room1").unwrap().contains(&socket));
            assert!(rooms_map.get("room2").unwrap().is_empty());
        }
    }

    #[tokio::test]
    async fn test_sockets() {
        let socket0: Sid = 0i64.into();
        let socket1: Sid = 1i64.into();
        let socket2: Sid = 2i64.into();
        let ns = Namespace::new_dummy([socket0, socket1, socket2]);
        let adapter = LocalAdapter::new(Arc::downgrade(&ns));
        adapter.add_all(socket0, ["room1", "room2"]);
        adapter.add_all(socket1, ["room1", "room3"]);
        adapter.add_all(socket2, ["room2", "room3"]);

        let sockets = adapter.sockets("room1");
        assert_eq!(sockets.len(), 2);
        assert!(sockets.contains(&socket0));
        assert!(sockets.contains(&socket1));

        let sockets = adapter.sockets("room2");
        assert_eq!(sockets.len(), 2);
        assert!(sockets.contains(&socket0));
        assert!(sockets.contains(&socket2));

        let sockets = adapter.sockets("room3");
        assert_eq!(sockets.len(), 2);
        assert!(sockets.contains(&socket1));
        assert!(sockets.contains(&socket2));
    }

    #[tokio::test]
    async fn test_disconnect_socket() {
        let socket0: Sid = 0i64.into();
        let socket1: Sid = 1i64.into();
        let socket2: Sid = 2i64.into();
        let ns = Namespace::new_dummy([socket0, socket1, socket2]);
        let adapter = LocalAdapter::new(Arc::downgrade(&ns));
        adapter.add_all(socket0, ["room1", "room2", "room4"]);
        adapter.add_all(socket1, ["room1", "room3", "room5"]);
        adapter.add_all(socket2, ["room2", "room3", "room6"]);

        let mut opts = BroadcastOptions::new(socket0);
        opts.rooms = vec!["room5".to_string()];
        match adapter.disconnect_socket(opts) {
            Err(Error::EngineGone) | Ok(_) => {}
            e => panic!(
                "should return an EngineGone error as it is a stub namespace: {:?}",
                e
            ),
        }

        let sockets = adapter.sockets("room2");
        assert_eq!(sockets.len(), 2);
        assert!(sockets.contains(&socket2));
        assert!(sockets.contains(&socket0));
    }
    #[tokio::test]
    async fn test_apply_opts() {
        let socket0: Sid = 0i64.into();
        let socket1: Sid = 1i64.into();
        let socket2: Sid = 2i64.into();
        let ns = Namespace::new_dummy([socket0, socket1, socket2]);
        let adapter = LocalAdapter::new(Arc::downgrade(&ns));
        // Add socket 0 to room1 and room2
        adapter.add_all(socket0, ["room1", "room2"]);
        // Add socket 1 to room1 and room3
        adapter.add_all(socket1, ["room1", "room3"]);
        // Add socket 2 to room2 and room3
        adapter.add_all(socket2, ["room1", "room2", "room3"]);

        // socket 2 is the sender
        let mut opts = BroadcastOptions::new(socket2);
        opts.rooms = vec!["room1".to_string()];
        opts.except = vec!["room2".to_string()];
        let sockets = adapter.fetch_sockets(opts);
        assert_eq!(sockets.len(), 1);
        assert_eq!(sockets[0].sid, socket1);

        let mut opts = BroadcastOptions::new(socket2);
        opts.flags.insert(BroadcastFlags::Broadcast);
        opts.except = vec!["room2".to_string()];
        let sockets = adapter.fetch_sockets(opts);
        assert_eq!(sockets.len(), 1);

        let opts = BroadcastOptions::new(socket2);
        let sockets = adapter.fetch_sockets(opts);
        assert_eq!(sockets.len(), 1);
        assert_eq!(sockets[0].sid, socket2);

        let opts = BroadcastOptions::new(10000i64.into());
        let sockets = adapter.fetch_sockets(opts);
        assert_eq!(sockets.len(), 0);
    }
}
