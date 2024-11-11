//! Adapters are responsible for managing the internal state of the server (rooms, sockets, etc...).
//! When a socket joins or leaves a room, the adapter is responsible for updating the state.
//! The default adapter is the [`LocalAdapter`], which stores the state in memory.
//! Other adapters can be made to share the state between multiple servers.

use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    convert::Infallible,
    future::{self, Future},
    sync::{RwLock, Weak},
    time::Duration,
};

use engineioxide::sid::Sid;
use futures_util::FutureExt;

use crate::{
    ack::AckInnerStream,
    errors::{AdapterError, BroadcastError},
    extract::SocketRef,
    ns::Namespace,
    operators::RoomParam,
    packet::Packet,
    DisconnectError,
};

/// A room identifier
pub type Room = Cow<'static, str>;

/// Flags that can be used to modify the behavior of the broadcast methods.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum BroadcastFlags {
    /// Broadcast only to the current server
    Local,
    /// Broadcast to all clients except the sender
    Broadcast,
}

/// Options that can be used to modify the behavior of the broadcast methods.
#[derive(Clone, Debug, Default)]
pub struct BroadcastOptions {
    /// The flags to apply to the broadcast.
    pub flags: HashSet<BroadcastFlags>,
    /// The rooms to broadcast to.
    pub rooms: HashSet<Room>,
    /// The rooms to exclude from the broadcast.
    pub except: HashSet<Room>,
    /// The socket id of the sender.
    pub sid: Option<Sid>,
}

pub trait AdapterCtr {
    type Adapter: Adapter;
    fn new(&self, ns: Weak<Namespace<Self::Adapter>>) -> Self::Adapter;
}

/// An adapter is responsible for managing the state of the namespace.
/// This adapter can be implemented to share the state between multiple servers.
/// The default adapter is the [`LocalAdapter`], which stores the state in memory.
pub trait Adapter: Send + Sync + 'static {
    /// An error that can occur when using the adapter. The default [`LocalAdapter`] has an [`Infallible`] error.
    type Error: std::error::Error + Into<AdapterError> + Send + 'static;
    type Ctr: AdapterCtr;

    /// Create a new adapter and give the namespace ref to retrieve sockets.
    fn new(ns: Weak<Namespace<Self>>) -> Self
    where
        Self: Sized;

    /// Initializes the adapter.
    fn init(&self) -> impl Future<Output = Result<(), Self::Error>> + Send;
    /// Closes the adapter.
    fn close(&self) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Returns the number of servers.
    fn server_count(&self) -> impl Future<Output = Result<u16, Self::Error>> + Send;

    /// Adds the socket to all the rooms.
    fn add_all(
        &self,
        sid: Sid,
        rooms: impl RoomParam,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
    /// Removes the socket from the rooms.
    fn del(
        &self,
        sid: Sid,
        rooms: impl RoomParam,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
    /// Removes the socket from all the rooms.
    fn del_all(&self, sid: Sid) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Broadcasts the packet to the sockets that match the [`BroadcastOptions`].
    fn broadcast(
        &self,
        packet: Packet,
        opts: BroadcastOptions,
    ) -> impl Future<Output = Result<(), BroadcastError>> + Send;

    /// Broadcasts the packet to the sockets that match the [`BroadcastOptions`] and return a stream of ack responses.
    fn broadcast_with_ack(
        &self,
        packet: Packet,
        opts: BroadcastOptions,
        timeout: Option<Duration>,
    ) -> impl Future<Output = AckInnerStream> + Send;

    /// Returns the sockets ids that match the [`BroadcastOptions`].
    fn sockets(
        &self,
        opts: BroadcastOptions,
    ) -> impl Future<Output = Result<Vec<Sid>, Self::Error>> + Send;

    /// Returns the rooms of the socket.
    fn socket_rooms(&self, sid: Sid)
        -> impl Future<Output = Result<Vec<Room>, Self::Error>> + Send;

    /// Returns the sockets that match the [`BroadcastOptions`].
    fn fetch_sockets(
        &self,
        opts: BroadcastOptions,
    ) -> impl Future<Output = Result<Vec<SocketRef<Self>>, Self::Error>> + Send
    where
        Self: Sized;

    /// Adds the sockets that match the [`BroadcastOptions`] to the rooms.
    fn add_sockets(
        &self,
        opts: BroadcastOptions,
        rooms: impl RoomParam,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
    /// Removes the sockets that match the [`BroadcastOptions`] from the rooms.
    fn del_sockets(
        &self,
        opts: BroadcastOptions,
        rooms: impl RoomParam,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Disconnects the sockets that match the [`BroadcastOptions`].
    fn disconnect_socket(
        &self,
        opts: BroadcastOptions,
    ) -> impl Future<Output = Result<(), Vec<DisconnectError>>> + Send;

    /// Returns all the rooms for this adapter.
    fn rooms(&self) -> impl Future<Output = Result<Vec<Room>, Self::Error>> + Send;

    //TODO: implement
    // fn server_side_emit(&self, packet: Packet, opts: BroadcastOptions) -> Result<u64, Error>;
    // fn persist_session(&self, sid: i64);
    // fn restore_session(&self, sid: i64) -> Session;
}

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

pub struct LocalAdapterCtr;
impl AdapterCtr for LocalAdapterCtr {
    type Adapter = LocalAdapter;
    fn new(&self, ns: Weak<Namespace<Self::Adapter>>) -> Self::Adapter {
        LocalAdapter::new(ns)
    }
}

impl Adapter for LocalAdapter {
    type Error = Infallible;
    type Ctr = LocalAdapterCtr;

    fn new(ns: Weak<Namespace<Self>>) -> Self {
        Self {
            rooms: HashMap::new().into(),
            ns,
        }
    }

    async fn init(&self) -> Result<(), Infallible> {
        Ok(())
    }

    async fn close(&self) -> Result<(), Infallible> {
        #[cfg(feature = "tracing")]
        tracing::debug!("closing local adapter: {}", self.ns.upgrade().unwrap().path);
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
    ) -> impl Future<Output = Result<(), BroadcastError>> + Send {
        use socketioxide_core::parser::Parse;
        let sockets = self.apply_opts(opts);

        #[cfg(feature = "tracing")]
        tracing::debug!("broadcasting packet to {} sockets", sockets.len());
        let parser = match sockets.first() {
            Some(socket) => socket.parser(),
            None => return future::ready(Ok(())),
        };
        let data = parser.encode(packet);
        let errors: Vec<_> = sockets
            .into_iter()
            .filter_map(|socket| socket.send_raw(data.clone()).err())
            .collect();
        if errors.is_empty() {
            future::ready(Ok(()))
        } else {
            future::ready(Err(errors.into()))
        }
    }

    async fn broadcast_with_ack(
        &self,
        packet: Packet,
        opts: BroadcastOptions,
        timeout: Option<Duration>,
    ) -> AckInnerStream {
        let sockets = self.apply_opts(opts);
        #[cfg(feature = "tracing")]
        tracing::debug!(
            "broadcasting packet to {} sockets: {:?}",
            sockets.len(),
            sockets.iter().map(|s| s.id).collect::<Vec<_>>()
        );
        AckInnerStream::broadcast(packet, sockets, timeout)
    }

    fn sockets(
        &self,
        opts: BroadcastOptions,
    ) -> impl Future<Output = Result<Vec<Sid>, Infallible>> + Send {
        let sockets = self
            .apply_opts(opts)
            .into_iter()
            .map(|socket| socket.id)
            .collect();
        future::ready(Ok(sockets))
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

    async fn fetch_sockets(
        &self,
        opts: BroadcastOptions,
    ) -> Result<Vec<SocketRef<Self>>, Infallible> {
        Ok(self.apply_opts(opts))
    }

    fn add_sockets(
        &self,
        opts: BroadcastOptions,
        rooms: impl RoomParam,
    ) -> impl Future<Output = Result<(), Infallible>> + Send {
        let rooms: Vec<Room> = rooms.into_room_iter().collect();
        for socket in self.apply_opts(opts) {
            self.add_all(socket.id, rooms.clone())
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
        for socket in self.apply_opts(opts) {
            self.del(socket.id, rooms.clone())
                .now_or_never()
                .unwrap()
                .unwrap();
        }
        future::ready(Ok(()))
    }

    async fn disconnect_socket(&self, opts: BroadcastOptions) -> Result<(), Vec<DisconnectError>> {
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

    async fn rooms(&self) -> Result<Vec<Room>, Self::Error> {
        Ok(self.rooms.read().unwrap().keys().cloned().collect())
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
        (LocalAdapter::new(Arc::downgrade(&ns)), ns)
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
        let sockets = now!(adapter.fetch_sockets(opts)).unwrap();
        assert_eq!(sockets.len(), 1);
        assert_eq!(sockets[0].id, socket1);

        let mut opts = BroadcastOptions {
            sid: Some(socket2),
            ..Default::default()
        };
        opts.flags.insert(BroadcastFlags::Broadcast);
        let sockets = now!(adapter.fetch_sockets(opts)).unwrap();
        assert_eq!(sockets.len(), 2);
        sockets.iter().for_each(|s| {
            assert!(s.id == socket0 || s.id == socket1);
        });

        let mut opts = BroadcastOptions {
            sid: Some(socket2),
            ..Default::default()
        };
        opts.flags.insert(BroadcastFlags::Broadcast);
        opts.except = hash_set!["room2".into()];
        let sockets = now!(adapter.fetch_sockets(opts)).unwrap();
        assert_eq!(sockets.len(), 1);

        let opts = BroadcastOptions {
            sid: Some(socket2),
            ..Default::default()
        };
        let sockets = now!(adapter.fetch_sockets(opts)).unwrap();
        assert_eq!(sockets.len(), 1);
        assert_eq!(sockets[0].id, socket2);

        let opts = BroadcastOptions {
            sid: Some(Sid::new()),
            ..Default::default()
        };
        let sockets = now!(adapter.fetch_sockets(opts)).unwrap();
        assert_eq!(sockets.len(), 0);
    }
}
