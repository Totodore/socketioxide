use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    convert::Infallible,
    sync::{RwLock, Weak},
};

use engineioxide::sid::Sid;

use crate::{
    ack::AckInnerStream,
    errors::{AdapterError, BroadcastError},
    extract::SocketRef,
    ns::Namespace,
    operators::RoomParam,
    packet::Packet,
    DisconnectError,
};

use super::{sync::Adapter, BroadcastFlags, BroadcastOptions, Room};

/// The default adapter. Store the state in memory.
#[derive(Debug)]
pub struct LocalAdapter {
    rooms: RwLock<HashMap<Room, HashSet<Sid>>>,
    ns: Weak<Namespace<Self>>,
}

impl From<Infallible> for AdapterError {
    fn from(_: Infallible) -> AdapterError {
        unreachable!()
    }
}

impl Adapter for LocalAdapter {
    type Error = Infallible;

    fn new(ns: Weak<Namespace<Self>>) -> Self {
        Self {
            rooms: HashMap::new().into(),
            ns,
        }
    }

    fn init(&self) -> Result<(), Infallible> {
        Ok(())
    }

    fn close(&self) -> Result<(), Infallible> {
        #[cfg(feature = "tracing")]
        tracing::debug!("closing local adapter: {}", self.ns.upgrade().unwrap().path);
        let mut rooms = self.rooms.write().unwrap();
        rooms.clear();
        rooms.shrink_to_fit();
        Ok(())
    }

    fn server_count(&self) -> Result<u16, Infallible> {
        Ok(1)
    }

    fn add_all(&self, sid: Sid, rooms: impl RoomParam) -> Result<(), Infallible> {
        let mut rooms_map = self.rooms.write().unwrap();
        for room in rooms.into_room_iter() {
            rooms_map.entry(room).or_default().insert(sid);
        }
        Ok(())
    }

    fn del(&self, sid: Sid, rooms: impl RoomParam) -> Result<(), Infallible> {
        let mut rooms_map = self.rooms.write().unwrap();
        for room in rooms.into_room_iter() {
            if let Some(room) = rooms_map.get_mut(&room) {
                room.remove(&sid);
            }
        }
        Ok(())
    }

    fn del_all(&self, sid: Sid) -> Result<(), Infallible> {
        let mut rooms_map = self.rooms.write().unwrap();
        for room in rooms_map.values_mut() {
            room.remove(&sid);
        }
        Ok(())
    }

    fn broadcast(&self, packet: Packet<'_>, opts: BroadcastOptions) -> Result<(), BroadcastError> {
        let sockets = self.apply_opts(opts);

        #[cfg(feature = "tracing")]
        tracing::debug!("broadcasting packet to {} sockets", sockets.len());
        let errors: Vec<_> = sockets
            .into_iter()
            .filter_map(|socket| socket.send(packet.clone()).err())
            .collect();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.into())
        }
    }

    fn broadcast_with_ack(
        &self,
        packet: Packet<'static>,
        opts: BroadcastOptions,
    ) -> AckInnerStream {
        let duration = opts.flags.iter().find_map(|flag| match flag {
            BroadcastFlags::Timeout(duration) => Some(*duration),
            _ => None,
        });
        let sockets = self.apply_opts(opts);
        #[cfg(feature = "tracing")]
        tracing::debug!(
            "broadcasting packet to {} sockets: {:?}",
            sockets.len(),
            sockets.iter().map(|s| s.id).collect::<Vec<_>>()
        );
        AckInnerStream::broadcast(packet, sockets, duration)
    }

    fn sockets(&self, rooms: impl RoomParam) -> Result<Vec<Sid>, Infallible> {
        let mut opts = BroadcastOptions::new(None);
        opts.rooms.extend(rooms.into_room_iter());
        Ok(self
            .apply_opts(opts)
            .into_iter()
            .map(|socket| socket.id)
            .collect())
    }

    //TODO: make this operation O(1)
    fn socket_rooms(&self, sid: Sid) -> Result<Vec<Cow<'static, str>>, Infallible> {
        let rooms_map = self.rooms.read().unwrap();
        Ok(rooms_map
            .iter()
            .filter(|(_, sockets)| sockets.contains(&sid))
            .map(|(room, _)| room.clone())
            .collect())
    }

    fn fetch_sockets(&self, opts: BroadcastOptions) -> Result<Vec<SocketRef<Self>>, Infallible> {
        Ok(self.apply_opts(opts))
    }

    fn add_sockets(&self, opts: BroadcastOptions, rooms: impl RoomParam) -> Result<(), Infallible> {
        let rooms: Vec<Room> = rooms.into_room_iter().collect();
        for socket in self.apply_opts(opts) {
            Adapter::add_all(self, socket.id, rooms.clone()).unwrap();
        }
        Ok(())
    }

    fn del_sockets(&self, opts: BroadcastOptions, rooms: impl RoomParam) -> Result<(), Infallible> {
        let rooms: Vec<Room> = rooms.into_room_iter().collect();
        for socket in self.apply_opts(opts) {
            Adapter::del(self, socket.id, rooms.clone()).unwrap();
        }
        Ok(())
    }

    fn disconnect_socket(&self, opts: BroadcastOptions) -> Result<(), Vec<DisconnectError>> {
        let mut errors: Vec<_> = Vec::new();

        for sock in self.apply_opts(opts) {
            if let Err(e) = sock.disconnect() {
                errors.push(e);
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

impl LocalAdapter {
    /// Applies the given `opts` and return the sockets that match.
    fn apply_opts(&self, opts: BroadcastOptions) -> Vec<SocketRef<Self>> {
        let rooms = opts.rooms;

        let except = self.get_except_sids(&opts.except);
        let ns = self.ns.upgrade().unwrap();
        if !rooms.is_empty() {
            let rooms_map = self.rooms.read().unwrap();
            rooms
                .iter()
                .filter_map(|room| rooms_map.get(room))
                .flatten()
                .filter(|sid| {
                    !except.contains(*sid)
                        && (!opts.flags.contains(&BroadcastFlags::Broadcast)
                            || opts.sid.map(|s| s != **sid).unwrap_or(true))
                })
                .filter_map(|sid| ns.get_socket(*sid).ok())
                .map(SocketRef::from)
                .collect()
        } else if opts.flags.contains(&BroadcastFlags::Broadcast) {
            let sockets = ns.get_sockets();
            sockets
                .into_iter()
                .filter(|socket| {
                    !except.contains(&socket.id) && opts.sid.map(|s| s != socket.id).unwrap_or(true)
                })
                .map(SocketRef::from)
                .collect()
        } else if let Some(sock) = opts.sid.and_then(|sid| ns.get_socket(sid).ok()) {
            vec![sock.into()]
        } else {
            vec![]
        }
    }

    fn get_except_sids(&self, except: &HashSet<Room>) -> HashSet<Sid> {
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
    use std::sync::Arc;

    macro_rules! hash_set {
        {$($v: expr),* $(,)?} => {
            std::collections::HashSet::from([$($v,)*])
        };
    }

    #[tokio::test]
    async fn test_server_count() {
        let ns = Namespace::new_dummy([]);
        let adapter = LocalAdapter::new(Arc::downgrade(&ns));
        assert_eq!(adapter.server_count().unwrap(), 1);
    }

    #[tokio::test]
    async fn test_add_all() {
        let socket = Sid::new();
        let ns = Namespace::new_dummy([socket]);
        let adapter = LocalAdapter::new(Arc::downgrade(&ns));
        adapter.add_all(socket, ["room1", "room2"]).unwrap();
        let rooms_map = adapter.rooms.read().unwrap();
        assert_eq!(rooms_map.len(), 2);
        assert_eq!(rooms_map.get("room1").unwrap().len(), 1);
        assert_eq!(rooms_map.get("room2").unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_del() {
        let socket = Sid::new();
        let ns = Namespace::new_dummy([socket]);
        let adapter = LocalAdapter::new(Arc::downgrade(&ns));
        adapter.add_all(socket, ["room1", "room2"]).unwrap();
        adapter.del(socket, "room1").unwrap();
        let rooms_map = adapter.rooms.read().unwrap();
        assert_eq!(rooms_map.len(), 2);
        assert_eq!(rooms_map.get("room1").unwrap().len(), 0);
        assert_eq!(rooms_map.get("room2").unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_del_all() {
        let socket = Sid::new();
        let ns = Namespace::new_dummy([socket]);
        let adapter = LocalAdapter::new(Arc::downgrade(&ns));
        adapter.add_all(socket, ["room1", "room2"]).unwrap();
        adapter.del_all(socket).unwrap();
        let rooms_map = adapter.rooms.read().unwrap();
        assert_eq!(rooms_map.len(), 2);
        assert_eq!(rooms_map.get("room1").unwrap().len(), 0);
        assert_eq!(rooms_map.get("room2").unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_socket_room() {
        let sid1 = Sid::new();
        let sid2 = Sid::new();
        let sid3 = Sid::new();
        let ns = Namespace::new_dummy([sid1, sid2, sid3]);
        let adapter = LocalAdapter::new(Arc::downgrade(&ns));
        adapter.add_all(sid1, ["room1", "room2"]).unwrap();
        adapter.add_all(sid2, ["room1"]).unwrap();
        adapter.add_all(sid3, ["room2"]).unwrap();
        assert!(adapter
            .socket_rooms(sid1)
            .unwrap()
            .contains(&"room1".into()));
        assert!(adapter
            .socket_rooms(sid1)
            .unwrap()
            .contains(&"room2".into()));
        assert_eq!(adapter.socket_rooms(sid2).unwrap(), ["room1"]);
        assert_eq!(adapter.socket_rooms(sid3).unwrap(), ["room2"]);
    }

    #[tokio::test]
    async fn test_add_socket() {
        let socket = Sid::new();
        let ns = Namespace::new_dummy([socket]);
        let adapter = LocalAdapter::new(Arc::downgrade(&ns));
        adapter.add_all(socket, ["room1"]).unwrap();

        let mut opts = BroadcastOptions::new(Some(socket));
        opts.rooms = hash_set!["room1".into()];
        adapter.add_sockets(opts, "room2").unwrap();
        let rooms_map = adapter.rooms.read().unwrap();

        assert_eq!(rooms_map.len(), 2);
        assert!(rooms_map.get("room1").unwrap().contains(&socket));
        assert!(rooms_map.get("room2").unwrap().contains(&socket));
    }

    #[tokio::test]
    async fn test_del_socket() {
        let socket = Sid::new();
        let ns = Namespace::new_dummy([socket]);
        let adapter = LocalAdapter::new(Arc::downgrade(&ns));
        adapter.add_all(socket, ["room1"]).unwrap();

        let mut opts = BroadcastOptions::new(Some(socket));
        opts.rooms = hash_set!["room1".into()];
        adapter.add_sockets(opts, "room2").unwrap();

        {
            let rooms_map = adapter.rooms.read().unwrap();

            assert_eq!(rooms_map.len(), 2);
            assert!(rooms_map.get("room1").unwrap().contains(&socket));
            assert!(rooms_map.get("room2").unwrap().contains(&socket));
        }

        let mut opts = BroadcastOptions::new(Some(socket));
        opts.rooms = hash_set!["room1".into()];
        adapter.del_sockets(opts, "room2").unwrap();

        {
            let rooms_map = adapter.rooms.read().unwrap();

            assert_eq!(rooms_map.len(), 2);
            assert!(rooms_map.get("room1").unwrap().contains(&socket));
            assert!(rooms_map.get("room2").unwrap().is_empty());
        }
    }

    #[tokio::test]
    async fn test_sockets() {
        let socket0 = Sid::new();
        let socket1 = Sid::new();
        let socket2 = Sid::new();
        let ns = Namespace::new_dummy([socket0, socket1, socket2]);
        let adapter = LocalAdapter::new(Arc::downgrade(&ns));
        adapter.add_all(socket0, ["room1", "room2"]).unwrap();
        adapter.add_all(socket1, ["room1", "room3"]).unwrap();
        adapter.add_all(socket2, ["room2", "room3"]).unwrap();

        let sockets = adapter.sockets("room1").unwrap();
        assert_eq!(sockets.len(), 2);
        assert!(sockets.contains(&socket0));
        assert!(sockets.contains(&socket1));

        let sockets = adapter.sockets("room2").unwrap();
        assert_eq!(sockets.len(), 2);
        assert!(sockets.contains(&socket0));
        assert!(sockets.contains(&socket2));

        let sockets = adapter.sockets("room3").unwrap();
        assert_eq!(sockets.len(), 2);
        assert!(sockets.contains(&socket1));
        assert!(sockets.contains(&socket2));
    }

    #[tokio::test]
    async fn test_disconnect_socket() {
        let socket0 = Sid::new();
        let socket1 = Sid::new();
        let socket2 = Sid::new();
        let ns = Namespace::new_dummy([socket0, socket1, socket2]);
        let adapter = LocalAdapter::new(Arc::downgrade(&ns));
        adapter
            .add_all(socket0, ["room1", "room2", "room4"])
            .unwrap();
        adapter
            .add_all(socket1, ["room1", "room3", "room5"])
            .unwrap();
        adapter
            .add_all(socket2, ["room2", "room3", "room6"])
            .unwrap();

        let mut opts = BroadcastOptions::new(Some(socket0));
        opts.rooms = hash_set!["room5".into()];
        adapter.disconnect_socket(opts).unwrap();

        let sockets = adapter.sockets("room2").unwrap();
        assert_eq!(sockets.len(), 2);
        assert!(sockets.contains(&socket2));
        assert!(sockets.contains(&socket0));
    }
    #[tokio::test]
    async fn test_apply_opts() {
        let socket0 = Sid::new();
        let socket1 = Sid::new();
        let socket2 = Sid::new();
        let ns = Namespace::new_dummy([socket0, socket1, socket2]);
        let adapter = LocalAdapter::new(Arc::downgrade(&ns));
        // Add socket 0 to room1 and room2
        adapter.add_all(socket0, ["room1", "room2"]).unwrap();
        // Add socket 1 to room1 and room3
        adapter.add_all(socket1, ["room1", "room3"]).unwrap();
        // Add socket 2 to room2 and room3
        adapter
            .add_all(socket2, ["room1", "room2", "room3"])
            .unwrap();

        // socket 2 is the sender
        let mut opts = BroadcastOptions::new(Some(socket2));
        opts.rooms = hash_set!["room1".into()];
        opts.except = hash_set!["room2".into()];
        let sockets = adapter.fetch_sockets(opts).unwrap();
        assert_eq!(sockets.len(), 1);
        assert_eq!(sockets[0].id, socket1);

        let mut opts = BroadcastOptions::new(Some(socket2));
        opts.flags.insert(BroadcastFlags::Broadcast);
        let sockets = adapter.fetch_sockets(opts).unwrap();
        assert_eq!(sockets.len(), 2);
        sockets.iter().for_each(|s| {
            assert!(s.id == socket0 || s.id == socket1);
        });

        let mut opts = BroadcastOptions::new(Some(socket2));
        opts.flags.insert(BroadcastFlags::Broadcast);
        opts.except = hash_set!["room2".into()];
        let sockets = adapter.fetch_sockets(opts).unwrap();
        assert_eq!(sockets.len(), 1);

        let opts = BroadcastOptions::new(Some(socket2));
        let sockets = adapter.fetch_sockets(opts).unwrap();
        assert_eq!(sockets.len(), 1);
        assert_eq!(sockets[0].id, socket2);

        let opts = BroadcastOptions::new(Some(Sid::new()));
        let sockets = adapter.fetch_sockets(opts).unwrap();
        assert_eq!(sockets.len(), 0);
    }
}
