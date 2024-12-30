//! The adapter module contains the [`CoreAdapter`] trait and other related types.
//!
//! It is used to implement communication between socket.io servers to share messages and state.
//!
//! The [`CoreLocalAdapter`] provide a local implementation that will allow any implementors to apply local
//! operations (`broadcast_with_ack`, `broadcast`, `rooms`, etc...).
use std::{
    borrow::Cow,
    collections::{hash_set, HashMap, HashSet},
    error::Error as StdError,
    future::{self, Future},
    slice,
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
    fn get_all_sids(&self, filter: impl Fn(&Sid) -> bool) -> Vec<Sid>;
    /// Get the socket data that match the list of socket ids.
    fn get_remote_sockets(&self, sids: BroadcastIter<'_>) -> Vec<RemoteSocketData>;
    /// Send data to the list of socket ids.
    fn send_many(&self, sids: BroadcastIter<'_>, data: Value) -> Result<(), Vec<SocketError>>;
    /// Send data to the list of socket ids and get a stream of acks and the number of expected acks.
    fn send_many_with_ack(
        &self,
        sids: BroadcastIter<'_>,
        packet: Packet,
        timeout: Option<Duration>,
    ) -> (Self::AckStream, u32);
    /// Disconnect all the sockets in the list.
    /// TODO: take a [`BroadcastIter`]. Currently it is impossible because it may create deadlocks
    /// with Adapter::del_all call.
    fn disconnect_many(&self, sids: Vec<Sid>) -> Result<(), Vec<SocketError>>;
    /// Get the path of the namespace.
    fn path(&self) -> &Str;
    /// Get the parser of the namespace.
    fn parser(&self) -> impl Parse;
    /// Get the unique server id.
    fn server_id(&self) -> Sid;
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

    /// Fetches rooms that match the [`BroadcastOptions`]
    fn rooms(
        &self,
        opts: BroadcastOptions,
    ) -> impl Future<Output = Result<Vec<Room>, Self::Error>> + Send {
        future::ready(Ok(self.get_local().rooms(opts).into_iter().collect()))
    }

    /// Fetches remote sockets that match the [`BroadcastOptions`].
    fn fetch_sockets(
        &self,
        opts: BroadcastOptions,
    ) -> impl Future<Output = Result<Vec<RemoteSocketData>, Self::Error>> + Send {
        future::ready(Ok(self.get_local().fetch_sockets(opts)))
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
        let room_map = self.rooms.read().unwrap();
        let sids = self.apply_opts(&opts, &room_map);

        #[cfg(feature = "tracing")]
        tracing::debug!("broadcasting packet");
        if sids.is_empty() {
            return Ok(());
        }

        let data = self.sockets.parser().encode(packet);
        self.sockets.send_many(sids, data)
    }

    /// Broadcasts the packet to the sockets that match the [`BroadcastOptions`] and return a stream of ack responses.
    /// Also returns the number of local expected aknowledgements to know when to stop waiting.
    pub fn broadcast_with_ack(
        &self,
        packet: Packet,
        opts: BroadcastOptions,
        timeout: Option<Duration>,
    ) -> (E::AckStream, u32) {
        let room_map = self.rooms.read().unwrap();
        let sids = self.apply_opts(&opts, &room_map);
        #[cfg(feature = "tracing")]
        tracing::debug!("broadcasting packet");

        // We cannot pre-serialize the packet because we need to change the ack id.
        self.sockets.send_many_with_ack(sids, packet, timeout)
    }

    /// Returns the sockets ids that match the [`BroadcastOptions`].
    pub fn sockets(&self, opts: BroadcastOptions) -> Vec<Sid> {
        self.apply_opts(&opts, &self.rooms.read().unwrap())
            .collect()
    }

    /// Returns the sockets ids that match the [`BroadcastOptions`].
    pub fn fetch_sockets(&self, opts: BroadcastOptions) -> Vec<RemoteSocketData> {
        let rooms = self.rooms.read().unwrap();
        let sids = self.apply_opts(&opts, &rooms);
        self.sockets.get_remote_sockets(sids)
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
        // Here we have to collect sids, because we are going to modify the rooms map.
        let sids = self
            .apply_opts(&opts, &self.rooms.read().unwrap())
            .collect::<Vec<_>>();
        for sid in sids {
            self.add_all(sid, rooms.clone());
        }
    }

    /// Removes the sockets that match the [`BroadcastOptions`] from the rooms.
    pub fn del_sockets(&self, opts: BroadcastOptions, rooms: impl RoomParam) {
        let rooms: Vec<Room> = rooms.into_room_iter().collect();
        // Here we have to collect sids, because we are going to modify the rooms map.
        let sids = self
            .apply_opts(&opts, &self.rooms.read().unwrap())
            .collect::<Vec<_>>();
        for sid in sids {
            self.del(sid, rooms.clone());
        }
    }

    /// Disconnects the sockets that match the [`BroadcastOptions`].
    pub fn disconnect_socket(&self, opts: BroadcastOptions) -> Result<(), Vec<SocketError>> {
        let sids = self
            .apply_opts(&opts, &self.rooms.read().unwrap())
            .collect();
        self.sockets.disconnect_many(sids)
    }

    /// Returns all the rooms for this adapter.
    pub fn rooms(&self, opts: BroadcastOptions) -> HashSet<Room> {
        let rooms = self.rooms.read().unwrap();
        let sids = self.apply_opts(&opts, &rooms);
        let mut room_result = HashSet::new();
        for sid in sids {
            for (room, sockets) in rooms.iter() {
                if sockets.contains(&sid) {
                    room_result.insert(room.clone());
                }
            }
        }
        room_result
    }

    /// Get the namespace path.
    pub fn path(&self) -> &Str {
        self.sockets.path()
    }

    /// Get the parser of the namespace.
    pub fn parser(&self) -> impl Parse + '_ {
        self.sockets.parser()
    }
    /// Get the unique server identifier
    pub fn server_id(&self) -> Sid {
        self.sockets.server_id()
    }
}

/// The default broadcast iterator.
/// Extract, flatten and filter a list of sid from a room list
struct BroadcastRooms<'a> {
    rooms: slice::Iter<'a, Room>,
    rooms_map: &'a HashMap<Room, HashSet<Sid>>,
    except: HashSet<Sid>,
    flatten_iter: Option<hash_set::Iter<'a, Sid>>,
}
impl<'a> BroadcastRooms<'a> {
    fn new(
        rooms: &'a [Room],
        rooms_map: &'a HashMap<Room, HashSet<Sid>>,
        except: HashSet<Sid>,
    ) -> Self {
        BroadcastRooms {
            rooms: rooms.iter(),
            rooms_map,
            except,
            flatten_iter: None,
        }
    }
}
impl Iterator for BroadcastRooms<'_> {
    type Item = Sid;
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.flatten_iter.as_mut().and_then(Iterator::next) {
                Some(sid) if !self.except.contains(sid) => return Some(*sid),
                Some(_) => continue,
                None => self.flatten_iter = None,
            }

            let room = self.rooms.next()?;
            self.flatten_iter = self.rooms_map.get(room).map(HashSet::iter);
        }
    }
}

impl<E: SocketEmitter> CoreLocalAdapter<E> {
    /// Applies the given `opts` and return the sockets that match.
    fn apply_opts<'a>(
        &self,
        opts: &'a BroadcastOptions,
        rooms: &'a HashMap<Room, HashSet<Sid>>,
    ) -> BroadcastIter<'a> {
        let is_broadcast = opts.has_flag(BroadcastFlags::Broadcast);

        let mut except = self.get_except_sids(&opts.except);
        // In case of broadcast flag + if the sender is set,
        // we should not broadcast to it.
        if is_broadcast && opts.sid.is_some() {
            except.insert(opts.sid.unwrap());
        }

        if !opts.rooms.is_empty() {
            let iter = BroadcastRooms::new(&opts.rooms, rooms, except);
            InnerBroadcastIter::BroadcastRooms(iter).into()
        } else if is_broadcast {
            let sids = self.sockets.get_all_sids(|id| !except.contains(id));
            InnerBroadcastIter::GlobalBroadcast(sids.into_iter()).into()
        } else if let Some(id) = opts.sid {
            InnerBroadcastIter::Single(id).into()
        } else {
            InnerBroadcastIter::None.into()
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

/// An iterator that yields the socket ids that match the broadcast options.
/// Used with the [`SocketEmitter`] interface.
pub struct BroadcastIter<'a> {
    inner: InnerBroadcastIter<'a>,
}
enum InnerBroadcastIter<'a> {
    BroadcastRooms(BroadcastRooms<'a>),
    GlobalBroadcast(<Vec<Sid> as IntoIterator>::IntoIter),
    Single(Sid),
    None,
}
impl BroadcastIter<'_> {
    fn is_empty(&self) -> bool {
        matches!(self.inner, InnerBroadcastIter::None)
    }
}
impl<'a> From<InnerBroadcastIter<'a>> for BroadcastIter<'a> {
    fn from(inner: InnerBroadcastIter<'a>) -> Self {
        BroadcastIter { inner }
    }
}

impl Iterator for BroadcastIter<'_> {
    type Item = Sid;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}
impl Iterator for InnerBroadcastIter<'_> {
    type Item = Sid;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            InnerBroadcastIter::BroadcastRooms(inner) => inner.next(),
            InnerBroadcastIter::GlobalBroadcast(inner) => inner.next(),
            InnerBroadcastIter::Single(sid) => {
                let sid = *sid;
                *self = InnerBroadcastIter::None;
                Some(sid)
            }
            InnerBroadcastIter::None => None,
        }
    }
}

/// Represent the data of a remote socket.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Default, Clone)]
pub struct RemoteSocketData {
    /// The id of the remote socket.
    pub id: Sid,
    /// The server id this socket is connected to.
    pub server_id: Sid,
    /// The namespace this socket is connected to.
    pub ns: Str,
}

#[cfg(test)]
mod test {

    use smallvec::smallvec;
    use std::{
        array,
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
        fn get_all_sids(&self, filter: impl Fn(&Sid) -> bool) -> Vec<Sid> {
            self.sockets
                .iter()
                .copied()
                .filter(|id| filter(id))
                .collect()
        }

        fn get_remote_sockets(&self, sids: BroadcastIter<'_>) -> Vec<RemoteSocketData> {
            sids.map(|id| RemoteSocketData {
                id,
                server_id: Sid::ZERO,
                ns: self.path.clone(),
            })
            .collect()
        }

        fn send_many(&self, _: BroadcastIter<'_>, _: Value) -> Result<(), Vec<SocketError>> {
            Ok(())
        }

        fn send_many_with_ack(
            &self,
            _: BroadcastIter<'_>,
            _: Packet,
            _: Option<Duration>,
        ) -> (Self::AckStream, u32) {
            (StubAckStream, 0)
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
        fn server_id(&self) -> Sid {
            Sid::ZERO
        }
    }

    fn create_adapter<const S: usize>(sockets: [Sid; S]) -> CoreLocalAdapter<StubSockets> {
        CoreLocalAdapter::new(StubSockets::new(&sockets))
    }

    #[test]
    fn add_all() {
        let socket = Sid::new();
        let adapter = create_adapter([socket]);
        adapter.add_all(socket, ["room1", "room2"]);
        let rooms_map = adapter.rooms.read().unwrap();
        assert_eq!(rooms_map.len(), 2);
        assert_eq!(rooms_map.get("room1").unwrap().len(), 1);
        assert_eq!(rooms_map.get("room2").unwrap().len(), 1);
    }

    #[test]
    fn del() {
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
    fn del_all() {
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
    fn socket_room() {
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
    fn add_socket() {
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
    fn del_socket() {
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
    fn sockets() {
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
    fn disconnect_socket() {
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
    fn disconnect_empty_opts() {
        let adapter = create_adapter([]);
        let opts = BroadcastOptions::default();
        adapter.disconnect_socket(opts).unwrap();
    }

    #[test]
    fn apply_opts() {
        let mut sockets: [Sid; 3] = array::from_fn(|_| Sid::new());
        sockets.sort();
        let adapter = create_adapter(sockets);

        adapter.add_all(sockets[0], ["room1", "room2"]);
        adapter.add_all(sockets[1], ["room1", "room3"]);
        adapter.add_all(sockets[2], ["room1", "room2", "room3"]);

        // socket 2 is the sender
        let mut opts = BroadcastOptions::new(sockets[2]);
        opts.rooms = smallvec!["room1".into()];
        opts.except = smallvec!["room2".into()];
        let sids = adapter
            .apply_opts(&opts, &adapter.rooms.read().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(sids, [sockets[1]]);

        let mut opts = BroadcastOptions::new(sockets[2]);
        opts.add_flag(BroadcastFlags::Broadcast);
        let mut sids = adapter
            .apply_opts(&opts, &adapter.rooms.read().unwrap())
            .collect::<Vec<_>>();
        sids.sort();
        assert_eq!(sids, [sockets[0], sockets[1]]);

        let mut opts = BroadcastOptions::new(sockets[2]);
        opts.add_flag(BroadcastFlags::Broadcast);
        opts.except = smallvec!["room2".into()];
        let sids = adapter
            .apply_opts(&opts, &adapter.rooms.read().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(sids.len(), 1);

        let opts = BroadcastOptions::new(sockets[2]);
        let sids = adapter
            .apply_opts(&opts, &adapter.rooms.read().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(sids.len(), 1);
        assert_eq!(sids[0], sockets[2]);

        let opts = BroadcastOptions::new(Sid::new());
        let sids = adapter
            .apply_opts(&opts, &adapter.rooms.read().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(sids.len(), 1);
    }
}
