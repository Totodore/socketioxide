//! The adapter module contains the [`CoreAdapter`] trait and other related types.
//!
//! It is used to implement communication between socket.io servers to share messages and state.
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    error::Error as StdError,
    future::{self, Future},
    sync::RwLock,
    time::Duration,
};

use engineioxide::{sid::Sid, Str};
use futures_core::Stream;

use crate::{
    errors::{AdapterError, DisconnectError, SocketError},
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
#[derive(Clone, Debug, Default)]
pub struct BroadcastOptions {
    /// The flags to apply to the broadcast represented as a bitflag.
    flags: u8,
    /// The rooms to broadcast to.
    pub rooms: HashSet<Room>,
    /// The rooms to exclude from the broadcast.
    pub except: HashSet<Room>,
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

/// The [`SocketEmitter`] will be implmented by the socketioxide library.
/// It is simply used as an abstraction to allow the adapter to communicate
/// with the socket server without the need to depend on the socketioxide lib.
pub trait SocketEmitter: Send + Sync + 'static {
    /// An error that can occur when sending data an acknowledgment.
    type AckError: StdError + Send + 'static;
    /// A stream that emits the acknowledgments of multiple sockets.
    type AckStream: Stream<Item = (Sid, Result<Value, Self::AckError>)> + Send + 'static;

    /// Get all the socket ids in the namespace.
    fn get_all_sids(&self) -> Vec<Sid>;
    /// Send data to the list of socket ids.
    fn send_many(&self, sids: Vec<Sid>, data: Value) -> Result<(), Vec<SocketError>>;
    /// Send data to the list of socket ids and get a stream of acks.
    fn send_many_with_ack(
        &self,
        sids: Vec<Sid>,
        packet: Packet,
        timeout: Option<Duration>,
    ) -> Self::AckStream;
    /// Disconnect all the sockets in the list.
    fn disconnect_many(&self, sid: Vec<Sid>) -> Result<(), Vec<DisconnectError>>;
    /// Get the path of the namespace.
    fn path(&self) -> &Str;
    /// Get the parser of the namespace.
    fn parser(&self) -> impl Parse;
}

/// An adapter is responsible for managing the state of the namespace.
/// This adapter can be implemented to share the state between multiple servers.
/// The default adapter is the [`LocalAdapter`], which stores the state in memory.
pub trait CoreAdapter<E: SocketEmitter>: Sized + Send + Sync + 'static {
    /// An error that can occur when using the adapter. The default [`LocalAdapter`] has an [`Infallible`] error.
    type Error: StdError + Into<AdapterError> + Send + 'static;
    /// A shared state between all the namespace [`CoreAdapter`].
    /// This can be used to share a connection for example.
    type State: Send + Sync + 'static;

    /// Creates a new adapter with the given state and socket server.
    fn new(state: &Self::State, local: CoreLocalAdapter<E>) -> Self;

    /// Initializes the adapter.
    fn init(&self) -> impl Future<Output = Result<(), Self::Error>> + Send {
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

    /// Adds the socket to all the rooms.
    fn add_all(
        &self,
        sid: Sid,
        rooms: impl RoomParam,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        future::ready(Ok(self.get_local().add_all(sid, rooms)))
    }
    /// Removes the socket from the rooms.
    fn del(
        &self,
        sid: Sid,
        rooms: impl RoomParam,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        future::ready(Ok(self.get_local().del(sid, rooms)))
    }
    /// Removes the socket from all the rooms.
    fn del_all(&self, sid: Sid) -> impl Future<Output = Result<(), Self::Error>> + Send {
        future::ready(Ok(self.get_local().del_all(sid)))
    }

    /// Broadcasts the packet to the sockets that match the [`BroadcastOptions`].
    fn broadcast(
        &self,
        packet: Packet,
        opts: BroadcastOptions,
    ) -> impl Future<Output = Result<(), Vec<SocketError>>> + Send {
        future::ready(self.get_local().broadcast(packet, opts))
    }

    /// Broadcasts the packet to the sockets that match the [`BroadcastOptions`] and return a stream of ack responses.
    fn broadcast_with_ack(
        &self,
        packet: Packet,
        opts: BroadcastOptions,
        timeout: Option<Duration>,
    ) -> impl Future<Output = Result<E::AckStream, Self::Error>> + Send {
        future::ready(Ok(self
            .get_local()
            .broadcast_with_ack(packet, opts, timeout)))
    }

    /// Returns the sockets ids that match the [`BroadcastOptions`].
    fn sockets(
        &self,
        opts: BroadcastOptions,
    ) -> impl Future<Output = Result<Vec<Sid>, Self::Error>> + Send {
        future::ready(Ok(self.get_local().sockets(opts)))
    }

    /// Returns the rooms of the socket.
    fn socket_rooms(
        &self,
        sid: Sid,
    ) -> impl Future<Output = Result<Vec<Room>, Self::Error>> + Send {
        future::ready(Ok(self.get_local().socket_rooms(sid)))
    }

    /// Adds the sockets that match the [`BroadcastOptions`] to the rooms.
    fn add_sockets(
        &self,
        opts: BroadcastOptions,
        rooms: impl RoomParam,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        future::ready(Ok(self.get_local().add_sockets(opts, rooms)))
    }

    /// Removes the sockets that match the [`BroadcastOptions`] from the rooms.
    fn del_sockets(
        &self,
        opts: BroadcastOptions,
        rooms: impl RoomParam,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        future::ready(Ok(self.get_local().del_sockets(opts, rooms)))
    }

    /// Disconnects the sockets that match the [`BroadcastOptions`].
    fn disconnect_socket(
        &self,
        opts: BroadcastOptions,
    ) -> impl Future<Output = Result<(), Vec<DisconnectError>>> + Send {
        future::ready(self.get_local().disconnect_socket(opts))
    }

    /// Returns all the rooms for this adapter.
    fn rooms(&self) -> impl Future<Output = Result<Vec<Room>, Self::Error>> + Send {
        future::ready(Ok(self.get_local().rooms()))
    }

    /// Returns the local adapter. Used to enable default behaviors.
    fn get_local(&self) -> &CoreLocalAdapter<E>;

    //TODO: implement
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
            if let Some(room) = rooms_map.get_mut(&room) {
                room.remove(&sid);
            }
        }
    }

    /// Removes the socket from all the rooms.
    pub fn del_all(&self, sid: Sid) {
        let mut rooms_map = self.rooms.write().unwrap();
        for room in rooms_map.values_mut() {
            room.remove(&sid);
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
        let data = self.sockets.parser().encode(packet);
        self.sockets.send_many(sids, data)
    }

    /// Broadcasts the packet to the sockets that match the [`BroadcastOptions`] and return a stream of ack responses.
    pub fn broadcast_with_ack(
        &self,
        packet: Packet,
        opts: BroadcastOptions,
        timeout: Option<Duration>,
    ) -> E::AckStream {
        let sids = self.apply_opts(opts);
        #[cfg(feature = "tracing")]
        tracing::debug!("broadcasting packet to {} sockets: {:?}", sids.len(), sids);

        self.sockets.send_many_with_ack(sids, packet, timeout)
    }

    /// Returns the sockets ids that match the [`BroadcastOptions`].
    pub fn sockets(&self, opts: BroadcastOptions) -> Vec<Sid> {
        self.apply_opts(opts)
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
    pub fn disconnect_socket(&self, opts: BroadcastOptions) -> Result<(), Vec<DisconnectError>> {
        let sids = self.apply_opts(opts);
        self.sockets.disconnect_many(sids)
    }

    /// Returns all the rooms for this adapter.
    pub fn rooms(&self) -> Vec<Room> {
        self.rooms.read().unwrap().keys().cloned().collect()
    }

    /// Get the namespace path.
    pub fn path(&self) -> &Str {
        self.sockets.path()
    }
}

impl<E: SocketEmitter> CoreLocalAdapter<E> {
    /// Applies the given `opts` and return the sockets that match.
    fn apply_opts(&self, opts: BroadcastOptions) -> Vec<Sid> {
        let is_broadcast = opts.has_flag(BroadcastFlags::Broadcast);
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
                        && (!is_broadcast || opts.sid.map(|s| &s != id).unwrap_or(true))
                })
                .collect()
        } else if is_broadcast {
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

    use std::{
        convert::Infallible,
        pin::Pin,
        task::{Context, Poll},
    };

    use super::*;
    macro_rules! hash_set {
        {$($v: expr),* $(,)?} => {
            std::collections::HashSet::from([$($v,)*])
        };
    }

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
        type Item = (Sid, Result<Value, Infallible>);
        fn poll_next(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            Poll::Ready(None)
        }
    }

    impl SocketEmitter for StubSockets {
        type AckError = Infallible;
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

        fn disconnect_many(&self, _: Vec<Sid>) -> Result<(), Vec<DisconnectError>> {
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
        adapter.del(socket, "room1");
        let rooms_map = adapter.rooms.read().unwrap();
        assert_eq!(rooms_map.len(), 2);
        assert_eq!(rooms_map.get("room1").unwrap().len(), 0);
        assert_eq!(rooms_map.get("room2").unwrap().len(), 1);
    }

    #[test]
    fn test_del_all() {
        let socket = Sid::new();
        let adapter = create_adapter([socket]);
        adapter.add_all(socket, ["room1", "room2"]);
        adapter.del_all(socket);
        let rooms_map = adapter.rooms.read().unwrap();
        assert_eq!(rooms_map.len(), 2);
        assert_eq!(rooms_map.get("room1").unwrap().len(), 0);
        assert_eq!(rooms_map.get("room2").unwrap().len(), 0);
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
        opts.rooms = hash_set!["room1".into()];
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
        opts.rooms = hash_set!["room1".into()];
        adapter.add_sockets(opts, "room2");

        {
            let rooms_map = adapter.rooms.read().unwrap();

            assert_eq!(rooms_map.len(), 2);
            assert!(rooms_map.get("room1").unwrap().contains(&socket));
            assert!(rooms_map.get("room2").unwrap().contains(&socket));
        }

        let mut opts = BroadcastOptions::new(socket);
        opts.rooms = hash_set!["room1".into()];
        adapter.del_sockets(opts, "room2");

        {
            let rooms_map = adapter.rooms.read().unwrap();

            assert_eq!(rooms_map.len(), 2);
            assert!(rooms_map.get("room1").unwrap().contains(&socket));
            assert!(rooms_map.get("room2").unwrap().is_empty());
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

        let mut opts = BroadcastOptions::default();
        opts.rooms = hash_set!["room1".into()];
        let sockets = adapter.sockets(opts.clone());
        assert_eq!(sockets.len(), 2);
        assert!(sockets.contains(&socket0));
        assert!(sockets.contains(&socket1));

        opts.rooms = hash_set!["room2".into()];
        let sockets = adapter.sockets(opts.clone());
        assert_eq!(sockets.len(), 2);
        assert!(sockets.contains(&socket0));
        assert!(sockets.contains(&socket2));

        opts.rooms = hash_set!["room3".into()];
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
        opts.rooms = hash_set!["room5".into()];
        adapter.disconnect_socket(opts).unwrap();

        let mut opts = BroadcastOptions::default();
        opts.rooms.insert("room2".into());
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
        opts.rooms = hash_set!["room1".into()];
        opts.except = hash_set!["room2".into()];
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
        opts.except = hash_set!["room2".into()];
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
