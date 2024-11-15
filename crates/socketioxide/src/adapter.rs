//! Adapters are responsible for managing the internal state of the server (rooms, sockets, etc...).
//! When a socket joins or leaves a room, the adapter is responsible for updating the state.
//! The default adapter is the [`LocalAdapter`], which stores the state in memory.
//! Other adapters can be made to share the state between multiple servers.

use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    convert::Infallible,
    future::{self, Future},
    sync::RwLock,
    time::Duration,
};

use engineioxide::sid::Sid;
use futures_util::FutureExt;
use socketioxide_core::{
    adapter::{BroadcastFlags, BroadcastOptions, CoreAdapter, Room, RoomParam, SocketEmitter},
    errors::DisconnectError,
};

use crate::{ns::Emitter, packet::Packet, SocketError};

/// An adapter is responsible for managing the state of the namespace.
/// This adapter can be implemented to share the state between multiple servers.
/// The default adapter is the [`LocalAdapter`], which stores the state in memory.
pub trait Adapter: CoreAdapter<Emitter<Self>> + Sized {}
impl<T: CoreAdapter<Emitter<T>>> Adapter for T {}

// === LocalAdapter impls ===

/// The default adapter. Store the state in memory.
pub struct LocalAdapter {
    rooms: RwLock<HashMap<Room, HashSet<Sid>>>,
    sockets: Emitter<Self>,
}

impl CoreAdapter<Emitter<Self>> for LocalAdapter {
    type Error = Infallible;
    type State = ();

    fn new(_: &Self::State, sockets: Emitter<Self>) -> Self {
        Self {
            rooms: RwLock::new(HashMap::new()),
            sockets,
        }
    }

    async fn init(&self) -> Result<(), Infallible> {
        Ok(())
    }

    async fn close(&self) -> Result<(), Infallible> {
        #[cfg(feature = "tracing")]
        tracing::debug!("closing local adapter: {}", self.sockets.path());
        let mut rooms = self.rooms.write().unwrap();
        rooms.clear();
        rooms.shrink_to_fit();
        Ok(())
    }

    async fn server_count(&self) -> Result<u16, Infallible> {
        Ok(1)
    }

    fn add_all(
        &self,
        sid: Sid,
        rooms: impl RoomParam,
    ) -> impl Future<Output = Result<(), Infallible>> + Send {
        let mut rooms_map = self.rooms.write().unwrap();
        for room in rooms.into_room_iter() {
            rooms_map.entry(room).or_default().insert(sid);
        }
        future::ready(Ok(()))
    }

    fn del(
        &self,
        sid: Sid,
        rooms: impl RoomParam,
    ) -> impl Future<Output = Result<(), Infallible>> + Send {
        let mut rooms_map = self.rooms.write().unwrap();
        for room in rooms.into_room_iter() {
            if let Some(room) = rooms_map.get_mut(&room) {
                room.remove(&sid);
            }
        }
        future::ready(Ok(()))
    }

    fn del_all(&self, sid: Sid) -> impl Future<Output = Result<(), Infallible>> + Send {
        let mut rooms_map = self.rooms.write().unwrap();
        for room in rooms_map.values_mut() {
            room.remove(&sid);
        }
        future::ready(Ok(()))
    }

    fn broadcast(
        &self,
        packet: Packet,
        opts: BroadcastOptions,
    ) -> impl Future<Output = Result<(), Vec<SocketError>>> + Send {
        use socketioxide_core::parser::Parse;
        let sids = self.apply_opts(opts);

        #[cfg(feature = "tracing")]
        tracing::debug!("broadcasting packet to {} sockets", sids.len());
        let data = self.sockets.parser().encode(packet);
        let res = self.sockets.send_many(sids, data);
        future::ready(res)
    }

    async fn broadcast_with_ack(
        &self,
        packet: Packet,
        opts: BroadcastOptions,
        timeout: Option<Duration>,
    ) -> Result<<Emitter<Self> as SocketEmitter>::AckStream, Infallible> {
        let sids = self.apply_opts(opts);
        #[cfg(feature = "tracing")]
        tracing::debug!("broadcasting packet to {} sockets: {:?}", sids.len(), sids);

        Ok(self.sockets.send_many_with_ack(sids, packet, timeout))
    }

    fn sockets(
        &self,
        opts: BroadcastOptions,
    ) -> impl Future<Output = Result<Vec<Sid>, Infallible>> + Send {
        let sids = self.apply_opts(opts);
        future::ready(Ok(sids))
    }

    //TODO: make this operation O(1)
    async fn socket_rooms(&self, sid: Sid) -> Result<Vec<Cow<'static, str>>, Infallible> {
        let rooms_map = self.rooms.read().unwrap();
        Ok(rooms_map
            .iter()
            .filter(|(_, sockets)| sockets.contains(&sid))
            .map(|(room, _)| room.clone())
            .collect())
    }

    fn add_sockets(
        &self,
        opts: BroadcastOptions,
        rooms: impl RoomParam,
    ) -> impl Future<Output = Result<(), Infallible>> + Send {
        let rooms: Vec<Room> = rooms.into_room_iter().collect();
        for sid in self.apply_opts(opts) {
            self.add_all(sid, rooms.clone())
                .now_or_never()
                .unwrap()
                .unwrap();
        }
        future::ready(Ok(()))
    }

    fn del_sockets(
        &self,
        opts: BroadcastOptions,
        rooms: impl RoomParam,
    ) -> impl Future<Output = Result<(), Infallible>> + Send {
        let rooms: Vec<Room> = rooms.into_room_iter().collect();
        for sid in self.apply_opts(opts) {
            self.del(sid, rooms.clone())
                .now_or_never()
                .unwrap()
                .unwrap();
        }
        future::ready(Ok(()))
    }

    async fn disconnect_socket(&self, opts: BroadcastOptions) -> Result<(), Vec<DisconnectError>> {
        let sids = self.apply_opts(opts);
        self.sockets.disconnect_many(sids)
    }

    async fn rooms(&self) -> Result<Vec<Room>, Self::Error> {
        Ok(self.rooms.read().unwrap().keys().cloned().collect())
    }
}

impl LocalAdapter {
    /// Applies the given `opts` and return the sockets that match.
    fn apply_opts(&self, opts: BroadcastOptions) -> Vec<Sid> {
        let rooms = opts.rooms;

        let except = self.get_except_sids(&opts.except);
        if !rooms.is_empty() {
            let rooms_map = self.rooms.read().unwrap();
            rooms
                .iter()
                .filter_map(|room| rooms_map.get(room))
                .flatten()
                .copied()
                .filter(|id| {
                    !except.contains(id)
                        && (!opts.flags.contains(&BroadcastFlags::Broadcast)
                            || opts.sid.map(|s| &s != id).unwrap_or(true))
                })
                .collect()
        } else if opts.flags.contains(&BroadcastFlags::Broadcast) {
            self.sockets
                .get_all_sids()
                .into_iter()
                .filter(|id| !except.contains(id) && opts.sid.map(|s| &s != id).unwrap_or(true))
                .collect()
        } else if let Some(id) = opts.sid {
            vec![id]
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
    use crate::ns::Namespace;

    use super::*;
    use futures_util::FutureExt;
    use std::sync::Arc;
    macro_rules! hash_set {
        {$($v: expr),* $(,)?} => {
            std::collections::HashSet::from([$($v,)*])
        };
    }
    macro_rules! now {
        ($expr:expr) => {
            $expr.now_or_never().unwrap()
        };
    }

    fn create_adapter<const S: usize>(
        sockets: [Sid; S],
    ) -> (LocalAdapter, Arc<Namespace<LocalAdapter>>) {
        let ns = Namespace::new_dummy(sockets);
        let sockets = Emitter::new(Arc::downgrade(&ns), Default::default());
        (LocalAdapter::new(&(), sockets), ns)
    }

    #[tokio::test]
    async fn test_server_count() {
        let (adapter, _ns) = create_adapter([]);
        assert_eq!(now!(adapter.server_count()).unwrap(), 1);
    }

    #[tokio::test]
    async fn test_add_all() {
        let socket = Sid::new();
        let (adapter, _ns) = create_adapter([socket]);
        now!(adapter.add_all(socket, ["room1", "room2"])).unwrap();
        let rooms_map = adapter.rooms.read().unwrap();
        assert_eq!(rooms_map.len(), 2);
        assert_eq!(rooms_map.get("room1").unwrap().len(), 1);
        assert_eq!(rooms_map.get("room2").unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_del() {
        let socket = Sid::new();
        let (adapter, _ns) = create_adapter([socket]);
        now!(adapter.add_all(socket, ["room1", "room2"])).unwrap();
        now!(adapter.del(socket, "room1")).unwrap();
        let rooms_map = adapter.rooms.read().unwrap();
        assert_eq!(rooms_map.len(), 2);
        assert_eq!(rooms_map.get("room1").unwrap().len(), 0);
        assert_eq!(rooms_map.get("room2").unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_del_all() {
        let socket = Sid::new();
        let (adapter, _ns) = create_adapter([socket]);
        now!(adapter.add_all(socket, ["room1", "room2"])).unwrap();
        now!(adapter.del_all(socket)).unwrap();
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
        let (adapter, _ns) = create_adapter([sid1, sid2, sid3]);
        now!(adapter.add_all(sid1, ["room1", "room2"])).unwrap();
        now!(adapter.add_all(sid2, ["room1"])).unwrap();
        now!(adapter.add_all(sid3, ["room2"])).unwrap();
        assert!(now!(adapter.socket_rooms(sid1))
            .unwrap()
            .contains(&"room1".into()));
        assert!(now!(adapter.socket_rooms(sid1))
            .unwrap()
            .contains(&"room2".into()));
        assert_eq!(now!(adapter.socket_rooms(sid2)).unwrap(), ["room1"]);
        assert_eq!(now!(adapter.socket_rooms(sid3)).unwrap(), ["room2"]);
    }

    #[tokio::test]
    async fn test_add_socket() {
        let socket = Sid::new();
        let (adapter, _ns) = create_adapter([socket]);
        now!(adapter.add_all(socket, ["room1"])).unwrap();

        let mut opts = BroadcastOptions {
            sid: Some(socket),
            ..Default::default()
        };
        opts.rooms = hash_set!["room1".into()];
        now!(adapter.add_sockets(opts, "room2")).unwrap();
        let rooms_map = adapter.rooms.read().unwrap();

        assert_eq!(rooms_map.len(), 2);
        assert!(rooms_map.get("room1").unwrap().contains(&socket));
        assert!(rooms_map.get("room2").unwrap().contains(&socket));
    }

    #[tokio::test]
    async fn test_del_socket() {
        let socket = Sid::new();
        let (adapter, _ns) = create_adapter([socket]);
        now!(adapter.add_all(socket, ["room1"])).unwrap();

        let mut opts = BroadcastOptions {
            sid: Some(socket),
            ..Default::default()
        };
        opts.rooms = hash_set!["room1".into()];
        now!(adapter.add_sockets(opts, "room2")).unwrap();

        {
            let rooms_map = adapter.rooms.read().unwrap();

            assert_eq!(rooms_map.len(), 2);
            assert!(rooms_map.get("room1").unwrap().contains(&socket));
            assert!(rooms_map.get("room2").unwrap().contains(&socket));
        }

        let mut opts = BroadcastOptions {
            sid: Some(socket),
            ..Default::default()
        };
        opts.rooms = hash_set!["room1".into()];
        now!(adapter.del_sockets(opts, "room2")).unwrap();

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
        let (adapter, _ns) = create_adapter([socket0, socket1, socket2]);
        now!(adapter.add_all(socket0, ["room1", "room2"])).unwrap();
        now!(adapter.add_all(socket1, ["room1", "room3"])).unwrap();
        now!(adapter.add_all(socket2, ["room2", "room3"])).unwrap();

        let mut opts = BroadcastOptions::default();
        opts.rooms = hash_set!["room1".into()];
        let sockets = now!(adapter.sockets(opts.clone())).unwrap();
        assert_eq!(sockets.len(), 2);
        assert!(sockets.contains(&socket0));
        assert!(sockets.contains(&socket1));

        opts.rooms = hash_set!["room2".into()];
        let sockets = now!(adapter.sockets(opts.clone())).unwrap();
        assert_eq!(sockets.len(), 2);
        assert!(sockets.contains(&socket0));
        assert!(sockets.contains(&socket2));

        opts.rooms = hash_set!["room3".into()];
        let sockets = now!(adapter.sockets(opts.clone())).unwrap();
        assert_eq!(sockets.len(), 2);
        assert!(sockets.contains(&socket1));
        assert!(sockets.contains(&socket2));
    }

    #[tokio::test]
    async fn test_disconnect_socket() {
        let socket0 = Sid::new();
        let socket1 = Sid::new();
        let socket2 = Sid::new();
        let (adapter, _ns) = create_adapter([socket0, socket1, socket2]);
        now!(adapter.add_all(socket0, ["room1", "room2", "room4"])).unwrap();
        now!(adapter.add_all(socket1, ["room1", "room3", "room5"])).unwrap();
        now!(adapter.add_all(socket2, ["room2", "room3", "room6"])).unwrap();

        let mut opts = BroadcastOptions {
            sid: Some(socket0),
            ..Default::default()
        };
        opts.rooms = hash_set!["room5".into()];
        now!(adapter.disconnect_socket(opts)).unwrap();

        let mut opts = BroadcastOptions::default();
        opts.rooms.insert("room2".into());
        let sockets = now!(adapter.sockets(opts.clone())).unwrap();
        assert_eq!(sockets.len(), 2);
        assert!(sockets.contains(&socket2));
        assert!(sockets.contains(&socket0));
    }
    #[tokio::test]
    async fn test_apply_opts() {
        let socket0 = Sid::new();
        let socket1 = Sid::new();
        let socket2 = Sid::new();
        let (adapter, _ns) = create_adapter([socket0, socket1, socket2]);
        // Add socket 0 to room1 and room2
        now!(adapter.add_all(socket0, ["room1", "room2"])).unwrap();
        // Add socket 1 to room1 and room3
        now!(adapter.add_all(socket1, ["room1", "room3"])).unwrap();
        // Add socket 2 to room2 and room3
        now!(adapter.add_all(socket2, ["room1", "room2", "room3"])).unwrap();

        // socket 2 is the sender
        let mut opts = BroadcastOptions {
            sid: Some(socket2),
            ..Default::default()
        };
        opts.rooms = hash_set!["room1".into()];
        opts.except = hash_set!["room2".into()];
        let sids = now!(adapter.sockets(opts)).unwrap();
        assert_eq!(sids.len(), 1);
        assert_eq!(sids[0], socket1);

        let mut opts = BroadcastOptions {
            sid: Some(socket2),
            ..Default::default()
        };
        opts.flags.insert(BroadcastFlags::Broadcast);
        let sids = now!(adapter.sockets(opts)).unwrap();
        assert_eq!(sids.len(), 2);
        sids.into_iter().for_each(|id| {
            assert!(id == socket0 || id == socket1);
        });

        let mut opts = BroadcastOptions {
            sid: Some(socket2),
            ..Default::default()
        };
        opts.flags.insert(BroadcastFlags::Broadcast);
        opts.except = hash_set!["room2".into()];
        let sids = now!(adapter.sockets(opts)).unwrap();
        assert_eq!(sids.len(), 1);

        let opts = BroadcastOptions {
            sid: Some(socket2),
            ..Default::default()
        };
        let sids = now!(adapter.sockets(opts)).unwrap();
        assert_eq!(sids.len(), 1);
        assert_eq!(sids[0], socket2);

        let opts = BroadcastOptions {
            sid: Some(Sid::new()),
            ..Default::default()
        };
        let sids = now!(adapter.sockets(opts)).unwrap();
        assert_eq!(sids.len(), 0);
    }
}
