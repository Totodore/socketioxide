//! Adapters are responsible for managing the state of the server.
//! When a socket joins or leaves a room, the adapter is responsible for updating the state.
//! The default adapter is the [`LocalAdapter`], which stores the state in memory.
//! Other adapters can be made to share the state between multiple servers.

use std::{
    collections::{HashMap, HashSet},
    convert::Infallible,
    sync::{Arc, RwLock, Weak},
    time::Duration,
};

use engineioxide::sid::Sid;
use futures::{
    stream::{self, BoxStream},
    StreamExt,
};
use itertools::Itertools;
use serde::de::DeserializeOwned;

use crate::{
    errors::{AckError, AdapterError, BroadcastError},
    handler::AckResponse,
    ns::Namespace,
    operators::RoomParam,
    packet::Packet,
    socket::Socket,
};

/// A room identifier
pub type Room = String;

/// Flags that can be used to modify the behavior of the broadcast methods.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum BroadcastFlags {
    /// Broadcast only to the current server
    Local,
    /// Broadcast to all servers
    Broadcast,
    /// Add a custom timeout to the ack callback
    Timeout(Duration),
}

/// Options that can be used to modify the behavior of the broadcast methods.
#[derive(Clone, Debug)]
pub struct BroadcastOptions {
    /// The flags to apply to the broadcast.
    pub flags: HashSet<BroadcastFlags>,
    /// The rooms to broadcast to.
    pub rooms: Vec<Room>,
    /// The rooms to exclude from the broadcast.
    pub except: Vec<Room>,
    /// The socket id of the sender.
    pub sid: Option<Sid>,
}
impl BroadcastOptions {
    pub fn new(sid: Option<Sid>) -> Self {
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
    type Error: std::error::Error + Into<AdapterError> + Send + 'static;

    /// Create a new adapter and give the namespace ref to retrieve sockets.
    fn new(ns: Weak<Namespace<Self>>) -> Self
    where
        Self: Sized;

    /// Initialize the adapter.
    fn init(&self) -> Result<(), Self::Error>;
    /// Close the adapter.
    fn close(&self) -> Result<(), Self::Error>;

    /// Return the number of servers.
    fn server_count(&self) -> Result<u16, Self::Error>;

    /// Add the socket to all the rooms.
    fn add_all(&self, sid: Sid, rooms: impl RoomParam) -> Result<(), Self::Error>;
    /// Remove the socket from the rooms.
    fn del(&self, sid: Sid, rooms: impl RoomParam) -> Result<(), Self::Error>;
    /// Remove the socket from all the rooms.
    fn del_all(&self, sid: Sid) -> Result<(), Self::Error>;

    /// Broadcast the packet to the sockets that match the [`BroadcastOptions`].
    fn broadcast(&self, packet: Packet, opts: BroadcastOptions) -> Result<(), BroadcastError>;

    /// Broadcast the packet to the sockets that match the [`BroadcastOptions`] and return a stream of ack responses.
    fn broadcast_with_ack<V: DeserializeOwned>(
        &self,
        packet: Packet,
        opts: BroadcastOptions,
    ) -> Result<BoxStream<'static, Result<AckResponse<V>, AckError>>, BroadcastError>;

    /// Return the sockets ids that match the [`BroadcastOptions`].
    fn sockets(&self, rooms: impl RoomParam) -> Result<Vec<Sid>, Self::Error>;

    /// Return the rooms of the socket.
    fn socket_rooms(&self, sid: Sid) -> Result<Vec<Room>, Self::Error>;

    /// Return the sockets that match the [`BroadcastOptions`].
    fn fetch_sockets(&self, opts: BroadcastOptions) -> Result<Vec<Arc<Socket<Self>>>, Self::Error>
    where
        Self: Sized;

    /// Add the sockets that match the [`BroadcastOptions`] to the rooms.
    fn add_sockets(&self, opts: BroadcastOptions, rooms: impl RoomParam)
        -> Result<(), Self::Error>;
    /// Remove the sockets that match the [`BroadcastOptions`] from the rooms.
    fn del_sockets(&self, opts: BroadcastOptions, rooms: impl RoomParam)
        -> Result<(), Self::Error>;
    /// Disconnect the sockets that match the [`BroadcastOptions`].
    fn disconnect_socket(&self, opts: BroadcastOptions) -> Result<(), BroadcastError>;

    //TODO: implement
    // fn server_side_emit(&self, packet: Packet, opts: BroadcastOptions) -> Result<u64, Error>;
    // fn persist_session(&self, sid: i64);
    // fn restore_session(&self, sid: i64) -> Session;
}

/// The default adapter. store the state in memory.
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

    fn broadcast(&self, packet: Packet, opts: BroadcastOptions) -> Result<(), BroadcastError> {
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

    fn broadcast_with_ack<V: DeserializeOwned>(
        &self,
        packet: Packet,
        opts: BroadcastOptions,
    ) -> Result<BoxStream<'static, Result<AckResponse<V>, AckError>>, BroadcastError> {
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
        let count = sockets.len();
        let ack_futs = sockets.into_iter().map(move |socket| {
            let packet = packet.clone();
            async move { socket.clone().send_with_ack(packet, duration).await }
        });
        Ok(stream::iter(ack_futs).buffer_unordered(count).boxed())
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
    fn socket_rooms(&self, sid: Sid) -> Result<Vec<String>, Infallible> {
        let rooms_map = self.rooms.read().unwrap();
        Ok(rooms_map
            .iter()
            .filter(|(_, sockets)| sockets.contains(&sid))
            .map(|(room, _)| room.clone())
            .collect())
    }

    fn fetch_sockets(
        &self,
        opts: BroadcastOptions,
    ) -> Result<Vec<Arc<Socket<LocalAdapter>>>, Infallible> {
        Ok(self.apply_opts(opts))
    }

    fn add_sockets(&self, opts: BroadcastOptions, rooms: impl RoomParam) -> Result<(), Infallible> {
        let rooms: Vec<Room> = rooms.into_room_iter().collect();
        for socket in self.apply_opts(opts) {
            self.add_all(socket.id, rooms.clone()).unwrap();
        }
        Ok(())
    }

    fn del_sockets(&self, opts: BroadcastOptions, rooms: impl RoomParam) -> Result<(), Infallible> {
        let rooms: Vec<Room> = rooms.into_room_iter().collect();
        for socket in self.apply_opts(opts) {
            self.del(socket.id, rooms.clone()).unwrap();
        }
        Ok(())
    }

    fn disconnect_socket(&self, opts: BroadcastOptions) -> Result<(), BroadcastError> {
        let errors: Vec<_> = self
            .apply_opts(opts)
            .into_iter()
            .filter_map(|socket| socket.disconnect().err())
            .collect();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.into())
        }
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
                        && (!opts.flags.contains(&BroadcastFlags::Broadcast)
                            || opts.sid.map(|s| s != **sid).unwrap_or_default())
                })
                .filter_map(|sid| ns.get_socket(*sid).ok())
                .collect()
        } else if opts.flags.contains(&BroadcastFlags::Broadcast) {
            let sockets = ns.get_sockets();
            sockets
                .into_iter()
                .filter(|socket| !except.contains(&socket.id))
                .collect()
        } else if let Some(sock) = opts.sid.and_then(|sid| ns.get_socket(sid).ok()) {
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
        opts.rooms = vec!["room1".to_string()];
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
        opts.rooms = vec!["room1".to_string()];
        adapter.add_sockets(opts, "room2").unwrap();

        {
            let rooms_map = adapter.rooms.read().unwrap();

            assert_eq!(rooms_map.len(), 2);
            assert!(rooms_map.get("room1").unwrap().contains(&socket));
            assert!(rooms_map.get("room2").unwrap().contains(&socket));
        }

        let mut opts = BroadcastOptions::new(Some(socket));
        opts.rooms = vec!["room1".to_string()];
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
        opts.rooms = vec!["room5".to_string()];
        match adapter.disconnect_socket(opts) {
            // todo it returns Ok, in previous commits it also returns Ok
            Err(BroadcastError::SendError(_)) | Ok(_) => {}
            e => panic!(
                "should return an EngineGone error as it is a stub namespace: {:?}",
                e
            ),
        }

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
        opts.rooms = vec!["room1".to_string()];
        opts.except = vec!["room2".to_string()];
        let sockets = adapter.fetch_sockets(opts).unwrap();
        assert_eq!(sockets.len(), 1);
        assert_eq!(sockets[0].id, socket1);

        let mut opts = BroadcastOptions::new(Some(socket2));
        opts.flags.insert(BroadcastFlags::Broadcast);
        opts.except = vec!["room2".to_string()];
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
