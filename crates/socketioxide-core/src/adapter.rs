//! The adapter module contains the [`CoreAdapter`] trait and other related types.
//!
//! It is used to implement communication between socket.io servers to share messages and state.
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    error::Error as StdError,
    future::{self, Future},
    sync::{Arc, RwLock},
    time::Duration,
};

use engineioxide::{sid::Sid, Str};
use futures_core::{FusedStream, Stream};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use smallvec::SmallVec;

use crate::{
    errors::{AdapterError, BroadcastError, SocketError},
    packet::Packet,
    parser::Parse,
    Value,
};

/// A room identifier
pub type Room = Cow<'static, str>;

/// Flags that can be used to modify the behavior of the broadcast methods.
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub enum BroadcastFlags {
    /// Broadcast only to the current server
    Local = 0x01,
    /// Broadcast to all clients except the sender
    Broadcast = 0x02,
}

/// Options that can be used to modify the behavior of the broadcast methods.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct BroadcastOptions {
    /// The flags to apply to the broadcast represented as a bitflag.
    flags: u8,
    /// The rooms to broadcast to.
    pub rooms: SmallVec<[Room; 4]>,
    /// The rooms to exclude from the broadcast.
    pub except: SmallVec<[Room; 4]>,
    /// The socket id of the sender.
    pub sid: Option<Sid>,
}
impl BroadcastOptions {
    /// Add any flags to the options.
    pub fn add_flag(&mut self, flag: BroadcastFlags) {
        self.flags |= flag as u8;
    }
    /// Check if the options have a flag.
    pub fn has_flag(&self, flag: BroadcastFlags) -> bool {
        self.flags & flag as u8 == flag as u8
    }

    /// get the flags of the options.
    pub fn flags(&self) -> u8 {
        self.flags
    }

    /// Set the socket id of the sender.
    pub fn new(sid: Sid) -> Self {
        Self {
            sid: Some(sid),
            ..Default::default()
        }
    }
}

/// A trait for types that can be used as a room parameter.
///
/// [`String`], [`Vec<String>`], [`Vec<&str>`], [`&'static str`](str) and const arrays are implemented by default.
pub trait RoomParam: Send + 'static {
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
impl RoomParam for &'static [&'static str] {
    type IntoIter =
        std::iter::Map<std::slice::Iter<'static, &'static str>, fn(&'static &'static str) -> Room>;

    #[inline(always)]
    fn into_room_iter(self) -> Self::IntoIter {
        self.iter().map(|i| Cow::Borrowed(*i))
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

/// A item yield by the ack stream.
pub type AckStreamItem<E> = (Sid, Result<Value, E>);
/// The [`SocketEmitter`] will be implemented by the socketioxide library.
/// It is simply used as an abstraction to allow the adapter to communicate
/// with the socket server without the need to depend on the socketioxide lib.
pub trait SocketEmitter: Send + Sync + 'static {
    /// An error that can occur when sending data an acknowledgment.
    type AckError: StdError + Send + Serialize + DeserializeOwned + 'static;
    /// A stream that emits the acknowledgments of multiple sockets.
    type AckStream: Stream<Item = AckStreamItem<Self::AckError>> + FusedStream + Send + 'static;

    /// Get all the socket ids in the namespace.
    fn get_all_sids(&self) -> Vec<Sid>;
    /// Send data to the list of socket ids.
    fn send_many(&self, sids: &[Sid], data: Value) -> Result<(), Vec<SocketError>>;
    /// Send data to the list of socket ids and get a stream of acks.
    fn send_many_with_ack(
        &self,
        sids: &[Sid],
        packet: Packet,
        timeout: Option<Duration>,
    ) -> Self::AckStream;
    /// Disconnect all the sockets in the list.
    fn disconnect_many(&self, sid: &[Sid]) -> Result<(), Vec<SocketError>>;
    /// Get the path of the namespace.
    fn path(&self) -> &Str;
    /// Get the parser of the namespace.
    fn parser(&self) -> impl Parse;
}

/// An adapter is responsible for managing the state of the namespace.
/// This adapter can be implemented to share the state between multiple servers.
///
/// A [`CoreLocalAdapter`] instance will be given when constructing this type, it will allow
/// you to manipulate local sockets (emitting, fetching data, broadcasting).
pub trait CoreAdapter<E: SocketEmitter>: Sized + Send + Sync + 'static {
    /// An error that can occur when using the adapter.
    type Error: StdError + Into<AdapterError> + Send + 'static;
    /// A shared state between all the namespace [`CoreAdapter`].
    /// This can be used to share a connection for example.
    type State: Send + Sync + 'static;
    /// A stream that emits the acknowledgments of multiple sockets.
    type AckStream: Stream<Item = AckStreamItem<E::AckError>> + FusedStream + Send + 'static;

    /// Creates a new adapter with the given state and local adapter.
    ///
    /// The state is used to share a common state between all your adapters. E.G. a connection to a remote system.
    /// The local adapter is used to manipulate the local sockets.
    fn new(state: &Self::State, local: CoreLocalAdapter<E>) -> Self;

    /// Initializes the adapter.
    fn init(self: Arc<Self>) -> impl Future<Output = Result<(), Self::Error>> + Send {
        future::ready(Ok(()))
    }

    /// Closes the adapter.
    fn close(&self) -> impl Future<Output = Result<(), Self::Error>> + Send {
        future::ready(Ok(()))
    }

    /// Returns the number of servers.
    fn server_count(&self) -> impl Future<Output = Result<u16, Self::Error>> + Send {
        future::ready(Ok(1))
    }

    /// Broadcasts the packet to the sockets that match the [`BroadcastOptions`].
    fn broadcast(
        &self,
        packet: Packet,
        opts: BroadcastOptions,
    ) -> impl Future<Output = Result<(), BroadcastError>> + Send {
        future::ready(
            self.get_local()
                .broadcast(packet, opts)
                .map_err(BroadcastError::from),
        )
    }

    /// Broadcasts the packet to the sockets that match the [`BroadcastOptions`]
    /// and return a stream of ack responses.
    ///
    /// This method does not have default implementation because GAT cannot have default impls.
    /// <https://github.com/rust-lang/rust/issues/29661>
    fn broadcast_with_ack(
        &self,
        packet: Packet,
        opts: BroadcastOptions,
        timeout: Option<Duration>,
    ) -> impl Future<Output = Result<Self::AckStream, Self::Error>> + Send;

    /// Adds the sockets that match the [`BroadcastOptions`] to the rooms.
    fn add_sockets(
        &self,
        opts: BroadcastOptions,
        rooms: impl RoomParam,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        self.get_local().add_sockets(opts, rooms);
        future::ready(Ok(()))
    }

    /// Removes the sockets that match the [`BroadcastOptions`] from the rooms.
    fn del_sockets(
        &self,
        opts: BroadcastOptions,
        rooms: impl RoomParam,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        self.get_local().del_sockets(opts, rooms);
        future::ready(Ok(()))
    }

    /// Disconnects the sockets that match the [`BroadcastOptions`].
    fn disconnect_socket(
        &self,
        opts: BroadcastOptions,
    ) -> impl Future<Output = Result<(), BroadcastError>> + Send {
        future::ready(
            self.get_local()
                .disconnect_socket(opts)
                .map_err(BroadcastError::Socket),
        )
    }

    /// Returns all the rooms for this adapter.
    fn rooms(&self) -> impl Future<Output = Result<Vec<Room>, Self::Error>> + Send {
        future::ready(Ok(self.get_local().rooms()))
    }

    /// Returns the local adapter. Used to enable default behaviors.
    fn get_local(&self) -> &CoreLocalAdapter<E>;

    //TODO: implement
    // fn fetch_sockets(&self, opts: BroadcastOptions) -> Result<Vec<RemoteSocket>, Error>;
    // fn server_side_emit(&self, packet: Packet, opts: BroadcastOptions) -> Result<u64, Error>;
    // fn persist_session(&self, sid: i64);
    // fn restore_session(&self, sid: i64) -> Session;
}

/// The default adapter. Store the state in memory.
pub struct CoreLocalAdapter<E> {
    rooms: RwLock<HashMap<Room, HashSet<Sid>>>,
    sockets: E,
}

impl<E: SocketEmitter> CoreLocalAdapter<E> {
    /// Create a new local adapter with the given sockets interface.
    pub fn new(sockets: E) -> Self {
        Self {
            rooms: RwLock::new(HashMap::new()),
            sockets,
        }
    }

    /// Clears all the rooms and sockets.
    pub fn close(&self) {
        #[cfg(feature = "tracing")]
        tracing::debug!("closing local adapter: {}", self.path());

        let mut rooms = self.rooms.write().unwrap();
        rooms.clear();
        rooms.shrink_to_fit();
    }

    /// Adds the socket to all the rooms.
    pub fn add_all(&self, sid: Sid, rooms: impl RoomParam) {
        let mut rooms_map = self.rooms.write().unwrap();
        for room in rooms.into_room_iter() {
            rooms_map.entry(room).or_default().insert(sid);
        }
    }

    /// Removes the socket from the rooms.
    pub fn del(&self, sid: Sid, rooms: impl RoomParam) {
        let mut rooms_map = self.rooms.write().unwrap();
        for room in rooms.into_room_iter() {
            let room_empty = if let Some(room) = rooms_map.get_mut(&room) {
                room.remove(&sid);
                room.is_empty()
            } else {
                false
            };
            if room_empty {
                rooms_map.remove(&room);
            }
        }
    }

    /// Removes the socket from all the rooms.
    pub fn del_all(&self, sid: Sid) {
        let mut rooms_map = self.rooms.write().unwrap();
        for room in rooms_map.values_mut() {
            room.remove(&sid);
        }
        //TODO: avoid re-iterating
        for (room, sockets) in rooms_map.clone() {
            if sockets.is_empty() {
                rooms_map.remove(&room);
            }
        }
    }

    /// Broadcasts the packet to the sockets that match the [`BroadcastOptions`].
    pub fn broadcast(
        &self,
        packet: Packet,
        opts: BroadcastOptions,
    ) -> Result<(), Vec<SocketError>> {
        use crate::parser::Parse;
        let sids = self.apply_opts(opts);

        #[cfg(feature = "tracing")]
        tracing::debug!("broadcasting packet to {} sockets", sids.len());
        if sids.is_empty() {
            return Ok(());
        }

        let data = self.sockets.parser().encode(packet);
        self.sockets.send_many(&sids, data)
    }

    /// Broadcasts the packet to the sockets that match the [`BroadcastOptions`] and return a stream of ack responses.
    /// Also returns the number of local expected aknowledgements to know when to stop waiting.
    pub fn broadcast_with_ack(
        &self,
        packet: Packet,
        opts: BroadcastOptions,
        timeout: Option<Duration>,
    ) -> (E::AckStream, u32) {
        let sids = self.apply_opts(opts);
        #[cfg(feature = "tracing")]
        tracing::debug!("broadcasting packet to {} sockets: {:?}", sids.len(), sids);

        let count = sids.len() as u32;
        // We cannot pre-serialize the packet because we need to change the ack id.
        let stream = self.sockets.send_many_with_ack(&sids, packet, timeout);
        (stream, count)
    }

    /// Returns the sockets ids that match the [`BroadcastOptions`].
    pub fn sockets(&self, opts: BroadcastOptions) -> Vec<Sid> {
        self.apply_opts(opts).into_vec()
    }

    //TODO: make this operation O(1)
    /// Returns the rooms of the socket.
    pub fn socket_rooms(&self, sid: Sid) -> Vec<Cow<'static, str>> {
        let rooms_map = self.rooms.read().unwrap();
        rooms_map
            .iter()
            .filter(|(_, sockets)| sockets.contains(&sid))
            .map(|(room, _)| room.clone())
            .collect()
    }

    /// Adds the sockets that match the [`BroadcastOptions`] to the rooms.
    pub fn add_sockets(&self, opts: BroadcastOptions, rooms: impl RoomParam) {
        let rooms: Vec<Room> = rooms.into_room_iter().collect();
        for sid in self.apply_opts(opts) {
            self.add_all(sid, rooms.clone());
        }
    }

    /// Removes the sockets that match the [`BroadcastOptions`] from the rooms.
    pub fn del_sockets(&self, opts: BroadcastOptions, rooms: impl RoomParam) {
        let rooms: Vec<Room> = rooms.into_room_iter().collect();
        for sid in self.apply_opts(opts) {
            self.del(sid, rooms.clone());
        }
    }

    /// Disconnects the sockets that match the [`BroadcastOptions`].
    pub fn disconnect_socket(&self, opts: BroadcastOptions) -> Result<(), Vec<SocketError>> {
        let sids = self.apply_opts(opts);
        self.sockets.disconnect_many(&sids)
    }

    /// Returns all the rooms for this adapter.
    pub fn rooms(&self) -> Vec<Room> {
        self.rooms.read().unwrap().keys().cloned().collect()
    }

    /// Get the namespace path.
    pub fn path(&self) -> &Str {
        self.sockets.path()
    }

    /// Get the parser of the namespace.
    pub fn parser(&self) -> impl Parse + '_ {
        self.sockets.parser()
    }
}

impl<E: SocketEmitter> CoreLocalAdapter<E> {
    /// Applies the given `opts` and return the sockets that match.
    fn apply_opts(&self, opts: BroadcastOptions) -> SmallVec<[Sid; 16]> {
        let is_broadcast = opts.has_flag(BroadcastFlags::Broadcast);
        let rooms = opts.rooms;

        let except = self.get_except_sids(&opts.except);
        let is_socket_current = |id| opts.sid.map(|s| s != id).unwrap_or(true);
        if !rooms.is_empty() {
            let rooms_map = self.rooms.read().unwrap();
            rooms
                .iter()
                .filter_map(|room| rooms_map.get(room))
                .flatten()
                .copied()
                .filter(|id| !except.contains(id) && (!is_broadcast || is_socket_current(*id)))
                .collect()
        } else if is_broadcast {
            self.sockets
                .get_all_sids()
                .into_iter()
                .filter(|id| !except.contains(id) && is_socket_current(*id))
                .collect()
        } else if let Some(id) = opts.sid {
            smallvec::smallvec![id]
        } else {
            smallvec::smallvec![]
        }
    }

    fn get_except_sids(&self, except: &SmallVec<[Room; 4]>) -> HashSet<Sid> {
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

    use smallvec::smallvec;
    use std::{
        pin::Pin,
        task::{Context, Poll},
    };

    use super::*;

    struct StubSockets {
        sockets: HashSet<Sid>,
        path: Str,
    }
    impl StubSockets {
        fn new(sockets: &[Sid]) -> Self {
            let sockets = HashSet::from_iter(sockets.iter().copied());
            Self {
                sockets,
                path: Str::from("/"),
            }
        }
    }

    struct StubAckStream;
    impl Stream for StubAckStream {
        type Item = (Sid, Result<Value, StubError>);
        fn poll_next(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            Poll::Ready(None)
        }
    }
    impl FusedStream for StubAckStream {
        fn is_terminated(&self) -> bool {
            true
        }
    }
    #[derive(Debug, Serialize, Deserialize)]
    struct StubError;
    impl std::fmt::Display for StubError {
        fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            Ok(())
        }
    }
    impl std::error::Error for StubError {}

    impl SocketEmitter for StubSockets {
        type AckError = StubError;
        type AckStream = StubAckStream;

        fn get_all_sids(&self) -> Vec<Sid> {
            self.sockets.iter().copied().collect()
        }

        fn send_many(&self, _: Vec<Sid>, _: Value) -> Result<(), Vec<SocketError>> {
            Ok(())
        }

        fn send_many_with_ack(
            &self,
            _: Vec<Sid>,
            _: Packet,
            _: Option<Duration>,
        ) -> Self::AckStream {
            StubAckStream
        }

        fn disconnect_many(&self, _: Vec<Sid>) -> Result<(), Vec<SocketError>> {
            Ok(())
        }

        fn path(&self) -> &Str {
            &self.path
        }
        fn parser(&self) -> impl Parse {
            crate::parser::test::StubParser
        }
    }

    fn create_adapter<const S: usize>(sockets: [Sid; S]) -> CoreLocalAdapter<StubSockets> {
        CoreLocalAdapter::new(StubSockets::new(&sockets))
    }

    #[test]
    fn test_add_all() {
        let socket = Sid::new();
        let adapter = create_adapter([socket]);
        adapter.add_all(socket, ["room1", "room2"]);
        let rooms_map = adapter.rooms.read().unwrap();
        assert_eq!(rooms_map.len(), 2);
        assert_eq!(rooms_map.get("room1").unwrap().len(), 1);
        assert_eq!(rooms_map.get("room2").unwrap().len(), 1);
    }

    #[test]
    fn test_del() {
        let socket = Sid::new();
        let adapter = create_adapter([socket]);
        adapter.add_all(socket, ["room1", "room2"]);
        {
            let rooms_map = adapter.rooms.read().unwrap();
            assert_eq!(rooms_map.len(), 2);
            assert_eq!(rooms_map.get("room1").unwrap().len(), 1);
            assert_eq!(rooms_map.get("room2").unwrap().len(), 1);
        }
        adapter.del(socket, "room1");
        let rooms_map = adapter.rooms.read().unwrap();
        assert_eq!(rooms_map.len(), 1);
        assert!(rooms_map.get("room1").is_none());
        assert_eq!(rooms_map.get("room2").unwrap().len(), 1);
    }

    #[test]
    fn test_del_all() {
        let socket = Sid::new();
        let adapter = create_adapter([socket]);
        adapter.add_all(socket, ["room1", "room2"]);
        {
            let rooms_map = adapter.rooms.read().unwrap();
            assert_eq!(rooms_map.len(), 2);
            assert_eq!(rooms_map.get("room1").unwrap().len(), 1);
            assert_eq!(rooms_map.get("room2").unwrap().len(), 1);
        }

        adapter.del_all(socket);
        let rooms_map = adapter.rooms.read().unwrap();
        assert_eq!(rooms_map.len(), 0);
    }

    #[test]
    fn test_socket_room() {
        let sid1 = Sid::new();
        let sid2 = Sid::new();
        let sid3 = Sid::new();
        let adapter = create_adapter([sid1, sid2, sid3]);
        adapter.add_all(sid1, ["room1", "room2"]);
        adapter.add_all(sid2, ["room1"]);
        adapter.add_all(sid3, ["room2"]);
        assert!(adapter.socket_rooms(sid1).contains(&"room1".into()));
        assert!(adapter.socket_rooms(sid1).contains(&"room2".into()));
        assert_eq!(adapter.socket_rooms(sid2), ["room1"]);
        assert_eq!(adapter.socket_rooms(sid3), ["room2"]);
    }

    #[test]
    fn test_add_socket() {
        let socket = Sid::new();
        let adapter = create_adapter([socket]);
        adapter.add_all(socket, ["room1"]);

        let mut opts = BroadcastOptions::new(socket);
        opts.rooms = smallvec!["room1".into()];
        adapter.add_sockets(opts, "room2");
        let rooms_map = adapter.rooms.read().unwrap();

        assert_eq!(rooms_map.len(), 2);
        assert!(rooms_map.get("room1").unwrap().contains(&socket));
        assert!(rooms_map.get("room2").unwrap().contains(&socket));
    }

    #[test]
    fn test_del_socket() {
        let socket = Sid::new();
        let adapter = create_adapter([socket]);
        adapter.add_all(socket, ["room1"]);

        let mut opts = BroadcastOptions::new(socket);
        opts.rooms = smallvec!["room1".into()];
        adapter.add_sockets(opts, "room2");

        {
            let rooms_map = adapter.rooms.read().unwrap();

            assert_eq!(rooms_map.len(), 2);
            assert!(rooms_map.get("room1").unwrap().contains(&socket));
            assert!(rooms_map.get("room2").unwrap().contains(&socket));
        }

        let mut opts = BroadcastOptions::new(socket);
        opts.rooms = smallvec!["room1".into()];
        adapter.del_sockets(opts, "room2");

        {
            let rooms_map = adapter.rooms.read().unwrap();

            assert_eq!(rooms_map.len(), 1);
            assert!(rooms_map.get("room1").unwrap().contains(&socket));
            assert!(rooms_map.get("room2").is_none());
        }
    }

    #[test]
    fn test_sockets() {
        let socket0 = Sid::new();
        let socket1 = Sid::new();
        let socket2 = Sid::new();
        let adapter = create_adapter([socket0, socket1, socket2]);
        adapter.add_all(socket0, ["room1", "room2"]);
        adapter.add_all(socket1, ["room1", "room3"]);
        adapter.add_all(socket2, ["room2", "room3"]);

        let mut opts = BroadcastOptions {
            rooms: smallvec!["room1".into()],
            ..Default::default()
        };
        let sockets = adapter.sockets(opts.clone());
        assert_eq!(sockets.len(), 2);
        assert!(sockets.contains(&socket0));
        assert!(sockets.contains(&socket1));

        opts.rooms = smallvec!["room2".into()];
        let sockets = adapter.sockets(opts.clone());
        assert_eq!(sockets.len(), 2);
        assert!(sockets.contains(&socket0));
        assert!(sockets.contains(&socket2));

        opts.rooms = smallvec!["room3".into()];
        let sockets = adapter.sockets(opts.clone());
        assert_eq!(sockets.len(), 2);
        assert!(sockets.contains(&socket1));
        assert!(sockets.contains(&socket2));
    }

    #[test]
    fn test_disconnect_socket() {
        let socket0 = Sid::new();
        let socket1 = Sid::new();
        let socket2 = Sid::new();
        let adapter = create_adapter([socket0, socket1, socket2]);
        adapter.add_all(socket0, ["room1", "room2", "room4"]);
        adapter.add_all(socket1, ["room1", "room3", "room5"]);
        adapter.add_all(socket2, ["room2", "room3", "room6"]);

        let mut opts = BroadcastOptions::new(socket0);
        opts.rooms = smallvec!["room5".into()];
        adapter.disconnect_socket(opts).unwrap();

        let mut opts = BroadcastOptions::default();
        opts.rooms.push("room2".into());
        let sockets = adapter.sockets(opts.clone());
        assert_eq!(sockets.len(), 2);
        assert!(sockets.contains(&socket2));
        assert!(sockets.contains(&socket0));
    }
    #[test]
    fn test_apply_opts() {
        let socket0 = Sid::new();
        let socket1 = Sid::new();
        let socket2 = Sid::new();
        let adapter = create_adapter([socket0, socket1, socket2]);
        // Add socket 0 to room1 and room2
        adapter.add_all(socket0, ["room1", "room2"]);
        // Add socket 1 to room1 and room3
        adapter.add_all(socket1, ["room1", "room3"]);
        // Add socket 2 to room2 and room3
        adapter.add_all(socket2, ["room1", "room2", "room3"]);

        // socket 2 is the sender
        let mut opts = BroadcastOptions::new(socket2);
        opts.rooms = smallvec!["room1".into()];
        opts.except = smallvec!["room2".into()];
        let sids = adapter.sockets(opts);
        assert_eq!(sids.len(), 1);
        assert_eq!(sids[0], socket1);

        let mut opts = BroadcastOptions::new(socket2);
        opts.add_flag(BroadcastFlags::Broadcast);
        let sids = adapter.sockets(opts);
        assert_eq!(sids.len(), 2);
        sids.into_iter().for_each(|id| {
            assert!(id == socket0 || id == socket1);
        });

        let mut opts = BroadcastOptions::new(socket2);
        opts.add_flag(BroadcastFlags::Broadcast);
        opts.except = smallvec!["room2".into()];
        let sids = adapter.sockets(opts);
        assert_eq!(sids.len(), 1);

        let opts = BroadcastOptions::new(socket2);
        let sids = adapter.sockets(opts);
        assert_eq!(sids.len(), 1);
        assert_eq!(sids[0], socket2);

        let opts = BroadcastOptions::new(Sid::new());
        let sids = adapter.sockets(opts);
        assert_eq!(sids.len(), 1);
    }
}
