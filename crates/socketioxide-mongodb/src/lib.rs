#![cfg_attr(docsrs, feature(doc_auto_cfg))]
#![warn(
    clippy::all,
    clippy::todo,
    clippy::empty_enum,
    clippy::mem_forget,
    clippy::unused_self,
    clippy::filter_map_next,
    clippy::needless_continue,
    clippy::needless_borrow,
    clippy::match_wildcard_for_single_variants,
    clippy::if_let_mutex,
    clippy::await_holding_lock,
    clippy::match_on_vec_items,
    clippy::imprecise_flops,
    clippy::suboptimal_flops,
    clippy::lossy_float_literal,
    clippy::rest_pat_in_fully_bound_structs,
    clippy::fn_params_excessive_bools,
    clippy::exit,
    clippy::inefficient_to_string,
    clippy::linkedlist,
    clippy::macro_use_imports,
    clippy::option_option,
    clippy::verbose_file_reads,
    clippy::unnested_or_patterns,
    rust_2018_idioms,
    future_incompatible,
    nonstandard_style,
    missing_docs
)]
//! # A mongodb adapter implementation for the socketioxide crate.
//! The adapter is used to communicate with other nodes of the same application.
//! This allows to broadcast messages to sockets connected on other servers,
//! to get the list of rooms, to add or remove sockets from rooms, etc.
//!
//! To achieve this, the adapter uses [change streams](https://www.mongodb.com/docs/manual/changeStreams/)
//! on a collection. The message expiration process is either handled with TTL-indexes or a capped collection.
//! If you change the message expiration strategy, make sure to first drop the collection.
//! MongoDB doesn't support switching from capped to TTL-indexes on an existing collection.
//!
//! The [`Driver`] abstraction allows the use of any mongodb client.
//! One implementation is provided:
//! * [`MongoDbDriver`](crate::drivers::mongodb::MongoDbDriver) for the [`mongodb`] crate.
//!
//! You can also implement your own driver by implementing the [`Driver`] trait.
//!
//! <div class="warning">
//!     The provided driver implementation is using change streams.
//!     They are only available on replica sets and sharded clusters.
//!     Make sure your mongodb server/cluster is configured accordingly.
//! </div>
//!
//! <div class="warning">
//!     Socketioxide-mongodb is not compatible with <code>@socketio/mongodb-adapter</code>
//!     and <code>@socketio/mongodb-emitter</code>. They use completely different protocols and
//!     cannot be used together. Do not mix socket.io JS servers with socketioxide rust servers.
//! </div>
//!
//! ## Example with the default mongodb driver
//! ```rust
//! # use socketioxide::{SocketIo, extract::{SocketRef, Data}, adapter::Adapter};
//! # use socketioxide_mongodb::{MongoDbAdapterCtr, MongoDbAdapter};
//! # async fn doc_main() -> Result<(), Box<dyn std::error::Error>> {
//! async fn on_connect<A: Adapter>(socket: SocketRef<A>) {
//!     socket.join("room1");
//!     socket.on("event", on_event);
//!     let _ = socket.broadcast().emit("hello", "world").await.ok();
//! }
//! async fn on_event<A: Adapter>(socket: SocketRef<A>, Data(data): Data<String>) {}
//!
//! const URI: &str = "mongodb://127.0.0.1:27017/?replicaSet=rs0&directConnection=true";
//! let client = mongodb::Client::with_uri_str(URI).await?;
//! let adapter = MongoDbAdapterCtr::new_with_mongodb(client.database("test")).await?;
//! let (layer, io) = SocketIo::builder()
//!     .with_adapter::<MongoDbAdapter<_>>(adapter)
//!     .build_layer();
//! Ok(())
//! # }
//! ```
//!
//! Check the [`chat example`](https://github.com/Totodore/socketioxide/tree/main/examples/chat)
//! for more complete examples.
//!
//! ## How does it work?
//!
//! The [`MongoDbAdapterCtr`] is a constructor for the [`MongoDbAdapter`] which is an implementation of
//! the [`Adapter`](https://docs.rs/socketioxide/latest/socketioxide/adapter/trait.Adapter.html) trait.
//! The constructor takes a [`mongodb::Database`] as an argument and will configure a collection
//! according to the chosen message expiration strategy (TTL indexes or capped collection).
//!
//! Then, for each namespace, an adapter is created and it takes a corresponding [`CoreLocalAdapter`].
//! The [`CoreLocalAdapter`] allows to manage the local rooms and local sockets. The default `LocalAdapter`
//! is simply a wrapper around this [`CoreLocalAdapter`].
//!
//! Once it is created the adapter is initialized with the [`MongoDbAdapter::init`] method.
//! It will listen to changes on the event collection and emit heartbeats,
//! messages are composed of a header (in bson) and a binary payload encoded with msgpack.
//! Headers are used to filter and route messages to server/namespaces/event handlers.
//!
//! All messages are encoded with msgpack.
//!
//! There are 7 types of requests:
//! * Broadcast a packet to all the matching sockets.
//! * Broadcast a packet to all the matching sockets and wait for a stream of acks.
//! * Disconnect matching sockets.
//! * Get all the rooms.
//! * Add matching sockets to rooms.
//! * Remove matching sockets to rooms.
//! * Fetch all the remote sockets matching the options.
//! * Heartbeat
//! * Initial heartbeat. When receiving a initial heartbeat all other servers reply a heartbeat immediately.
//!
//! For ack streams, the adapter will first send a `BroadcastAckCount` response to the server that sent the request,
//! and then send the acks as they are received (more details in [`MongoDbAdapter::broadcast_with_ack`] fn).
//!
//! On the other side, each time an action has to be performed on the local server, the adapter will
//! first broadcast a request to all the servers and then perform the action locally.

use std::{
    borrow::Cow,
    collections::HashMap,
    fmt, future,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::{Duration, Instant},
};

use futures_core::{Stream, future::Future};
use futures_util::StreamExt;
use serde::{Serialize, de::DeserializeOwned};
use socketioxide_core::{
    Sid, Uid,
    adapter::{
        BroadcastOptions, CoreAdapter, CoreLocalAdapter, DefinedAdapter, RemoteSocketData, Room,
        RoomParam, SocketEmitter, Spawnable,
    },
    errors::{AdapterError, BroadcastError},
    packet::Packet,
};
use stream::{AckStream, ChanStream, DropStream};
use tokio::sync::mpsc;

use drivers::{Driver, Item, ItemHeader};
use request::{
    RequestIn, RequestOut, RequestTypeIn, RequestTypeOut, Response, ResponseType, ResponseTypeId,
};

/// Drivers are an abstraction over the pub/sub backend used by the adapter.
/// You can use the provided implementation or implement your own.
pub mod drivers;

mod request;
mod stream;

/// The configuration of the [`MongoDbAdapter`].
#[derive(Debug, Clone)]
pub struct MongoDbAdapterConfig {
    /// The heartbeat timeout duration. If a remote node does not respond within this duration,
    /// it will be considered disconnected. Default is 60 seconds.
    pub hb_timeout: Duration,
    /// The heartbeat interval duration. The current node will broadcast a heartbeat to the
    /// remote nodes at this interval. Default is 10 seconds.
    pub hb_interval: Duration,
    /// The request timeout. When expecting a response from remote nodes, if they do not respond within
    /// this duration, the request will be considered failed. Default is 5 seconds.
    pub request_timeout: Duration,
    /// The channel size used to receive ack responses. Default is 255.
    ///
    /// If you have a lot of servers/sockets and that you may miss acknowledgement because they arrive faster
    /// than you poll them with the returned stream, you might want to increase this value.
    pub ack_response_buffer: usize,
    /// The collection name used to store socket.io data. Default is "socket.io-adapter".
    pub collection: Cow<'static, str>,
    /// The [`MessageExpirationStrategy`] used to remove old documents.
    /// Default is `Ttl(Duration::from_secs(60))`.
    pub expiration_strategy: MessageExpirationStrategy,
}

/// The strategy used to remove old documents in the mongodb collection.
/// The default mongodb driver supports both [TTL indexes](https://www.mongodb.com/docs/manual/core/index-ttl/)
/// and [capped collections](https://www.mongodb.com/docs/manual/core/capped-collections/).
///
/// Prefer the [`MessageExpirationStrategy::TtlIndex`] strategy for better performance and usability.
#[derive(Debug, Clone)]
pub enum MessageExpirationStrategy {
    /// Use a TTL index to expire documents after a certain duration.
    #[cfg(feature = "ttl-index")]
    TtlIndex(Duration),
    /// Use a capped collection to limit the size in bytes of the collection. Older messages are removed.
    ///
    /// Be aware that if you send a message that is bigger than your capped collection's size,
    /// it will be rejected and won't be broadcast.
    CappedCollection(u64),
}

impl MongoDbAdapterConfig {
    /// Create a new [`MongoDbAdapterConfig`] with default values.
    pub fn new() -> Self {
        Self {
            hb_timeout: Duration::from_secs(60),
            hb_interval: Duration::from_secs(10),
            request_timeout: Duration::from_secs(5),
            ack_response_buffer: 255,
            collection: Cow::Borrowed("socket.io-adapter"),
            #[cfg(feature = "ttl-index")]
            expiration_strategy: MessageExpirationStrategy::TtlIndex(Duration::from_secs(60)),
            #[cfg(not(feature = "ttl-index"))]
            expiration_strategy: MessageExpirationStrategy::CappedCollection(1024 * 1024), // 1MB
        }
    }
    /// The heartbeat timeout duration. If a remote node does not respond within this duration,
    /// it will be considered disconnected. Default is 60 seconds.
    pub fn with_hb_timeout(mut self, hb_timeout: Duration) -> Self {
        self.hb_timeout = hb_timeout;
        self
    }
    /// The heartbeat interval duration. The current node will broadcast a heartbeat to the
    /// remote nodes at this interval. Default is 10 seconds.
    pub fn with_hb_interval(mut self, hb_interval: Duration) -> Self {
        self.hb_interval = hb_interval;
        self
    }
    /// The request timeout. When expecting a response from remote nodes, if they do not respond within
    /// this duration, the request will be considered failed. Default is 5 seconds.
    pub fn with_request_timeout(mut self, request_timeout: Duration) -> Self {
        self.request_timeout = request_timeout;
        self
    }
    /// The channel size used to receive ack responses. Default is 255.
    ///
    /// If you have a lot of servers/sockets and that you may miss acknowledgement because they arrive faster
    /// than you poll them with the returned stream, you might want to increase this value.
    pub fn with_ack_response_buffer(mut self, ack_response_buffer: usize) -> Self {
        self.ack_response_buffer = ack_response_buffer;
        self
    }
    /// The collection name used to store socket.io data. Default is "socket.io-adapter".
    pub fn with_collection(mut self, collection: impl Into<Cow<'static, str>>) -> Self {
        self.collection = collection.into();
        self
    }
    /// The [`MessageExpirationStrategy`] used to remove old documents.
    /// Default is `TtlIndex(Duration::from_secs(60))` with the `ttl-index` feature enabled.
    /// Otherwise it is `CappedCollection(1MB)`
    pub fn with_expiration_strategy(
        mut self,
        expiration_strategy: MessageExpirationStrategy,
    ) -> Self {
        self.expiration_strategy = expiration_strategy;
        self
    }
}

impl Default for MongoDbAdapterConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The adapter constructor. For each namespace you define, a new adapter instance is created
/// from this constructor.
#[derive(Debug, Clone)]
pub struct MongoDbAdapterCtr<D> {
    driver: D,
    config: MongoDbAdapterConfig,
}

#[cfg(feature = "mongodb")]
impl MongoDbAdapterCtr<drivers::mongodb::MongoDbDriver> {
    /// Create a new adapter constructor with the [`mongodb`](drivers::mongodb) driver
    /// and a default config.
    pub async fn new_with_mongodb(
        db: mongodb::Database,
    ) -> Result<Self, drivers::mongodb::mongodb_client::error::Error> {
        Self::new_with_mongodb_config(db, MongoDbAdapterConfig::default()).await
    }
    /// Create a new adapter constructor with the [`mongodb`](drivers::mongodb) driver
    /// and a custom config.
    pub async fn new_with_mongodb_config(
        db: mongodb::Database,
        config: MongoDbAdapterConfig,
    ) -> Result<Self, drivers::mongodb::mongodb_client::error::Error> {
        use drivers::mongodb::MongoDbDriver;
        let driver =
            MongoDbDriver::new(db, &config.collection, &config.expiration_strategy).await?;
        Ok(Self { driver, config })
    }
}
impl<D: Driver> MongoDbAdapterCtr<D> {
    /// Create a new adapter constructor with a custom mongodb driver and a config.
    ///
    /// You can implement your own driver by implementing the [`Driver`] trait with any mongodb client.
    /// Check the [`drivers`] module for more information.
    pub fn new_with_driver(driver: D, config: MongoDbAdapterConfig) -> Self {
        Self { driver, config }
    }
}

/// Represent any error that might happen when using this adapter.
#[derive(thiserror::Error)]
pub enum Error<D: Driver> {
    /// Mongo driver error
    #[error("driver error: {0}")]
    Driver(D::Error),
    /// Packet encoding error
    #[error("packet encoding error: {0}")]
    Encode(#[from] rmp_serde::encode::Error),
    /// Packet decoding error
    #[error("packet decoding error: {0}")]
    Decode(#[from] rmp_serde::decode::Error),
}

impl<R: Driver> Error<R> {
    fn from_driver(err: R::Error) -> Self {
        Self::Driver(err)
    }
}
impl<R: Driver> fmt::Debug for Error<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Driver(err) => write!(f, "Driver error: {:?}", err),
            Self::Decode(err) => write!(f, "Decode error: {:?}", err),
            Self::Encode(err) => write!(f, "Encode error: {:?}", err),
        }
    }
}

impl<R: Driver> From<Error<R>> for AdapterError {
    fn from(err: Error<R>) -> Self {
        AdapterError::from(Box::new(err) as Box<dyn std::error::Error + Send>)
    }
}

pub(crate) type ResponseHandlers = HashMap<Sid, mpsc::Sender<Item>>;

/// The mongodb adapter with the [mongodb](drivers::mongodb::mongodb_client) driver.
#[cfg(feature = "mongodb")]
pub type MongoDbAdapter<E> = CustomMongoDbAdapter<E, drivers::mongodb::MongoDbDriver>;

/// The mongodb adapter implementation.
/// It is generic over the [`Driver`] used to communicate with the mongodb server.
/// And over the [`SocketEmitter`] used to communicate with the local server. This allows to
/// avoid cyclic dependencies between the adapter, `socketioxide-core` and `socketioxide` crates.
pub struct CustomMongoDbAdapter<E, D> {
    /// The driver used by the adapter. This is used to communicate with the mongodb server.
    /// All the mongodb adapter instances share the same driver.
    driver: D,
    /// The configuration of the adapter.
    config: MongoDbAdapterConfig,
    /// A unique identifier for the adapter to identify itself in the mongodb server.
    uid: Uid,
    /// The local adapter, used to manage local rooms and socket stores.
    local: CoreLocalAdapter<E>,
    /// A map of nodes liveness, with the last time remote nodes were seen alive.
    nodes_liveness: Mutex<Vec<(Uid, std::time::Instant)>>,
    /// A map of response handlers used to await for responses from the remote servers.
    responses: Arc<Mutex<ResponseHandlers>>,
}

impl<E, D> DefinedAdapter for CustomMongoDbAdapter<E, D> {}
impl<E: SocketEmitter, D: Driver> CoreAdapter<E> for CustomMongoDbAdapter<E, D> {
    type Error = Error<D>;
    type State = MongoDbAdapterCtr<D>;
    type AckStream = AckStream<E::AckStream>;
    type InitRes = InitRes<D>;

    fn new(state: &Self::State, local: CoreLocalAdapter<E>) -> Self {
        let uid = local.server_id();
        Self {
            local,
            uid,
            driver: state.driver.clone(),
            config: state.config.clone(),
            nodes_liveness: Mutex::new(Vec::new()),
            responses: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn init(self: Arc<Self>, on_success: impl FnOnce() + Send + 'static) -> Self::InitRes {
        let fut = async move {
            let stream = self.driver.watch(self.uid, self.local.path()).await?;
            tokio::spawn(self.clone().handle_ev_stream(stream));
            tokio::spawn(self.clone().heartbeat_job());

            // Send initial heartbeat when starting.
            self.emit_init_heartbeat().await.map_err(|e| match e {
                Error::Driver(e) => e,
                Error::Encode(_) | Error::Decode(_) => unreachable!(),
            })?;

            on_success();
            Ok(())
        };
        InitRes(Box::pin(fut))
    }

    async fn close(&self) -> Result<(), Self::Error> {
        Ok(())
    }

    /// Get the number of servers by iterating over the node liveness heartbeats.
    async fn server_count(&self) -> Result<u16, Self::Error> {
        let treshold = std::time::Instant::now() - self.config.hb_timeout;
        let mut nodes_liveness = self.nodes_liveness.lock().unwrap();
        nodes_liveness.retain(|(_, v)| v > &treshold);
        Ok((nodes_liveness.len() + 1) as u16)
    }

    /// Broadcast a packet to all the servers to send them through their sockets.
    async fn broadcast(
        &self,
        packet: Packet,
        opts: BroadcastOptions,
    ) -> Result<(), BroadcastError> {
        if !opts.is_local(self.uid) {
            let req = RequestOut::new(self.uid, RequestTypeOut::Broadcast(&packet), &opts);
            self.send_req(req, None).await.map_err(AdapterError::from)?;
        }

        self.local.broadcast(packet, opts)?;
        Ok(())
    }

    /// Broadcast a packet to all the servers to send them through their sockets.
    ///
    /// Returns a Stream that is a combination of the local ack stream and a remote ack stream.
    /// Here is a specific protocol in order to know how many message the server expect to close
    /// the stream at the right time:
    /// * Get the number `n` of remote servers.
    /// * Send the broadcast request.
    /// * Expect `n` `BroadcastAckCount` response in the stream to know the number `m` of expected ack responses.
    /// * Expect `sum(m)` broadcast counts sent by the servers.
    ///
    /// Example with 3 remote servers (n = 3):
    /// ```text
    /// +---+                   +---+                   +---+
    /// | A |                   | B |                   | C |
    /// +---+                   +---+                   +---+
    ///   |                       |                       |
    ///   |---BroadcastWithAck--->|                       |
    ///   |---BroadcastWithAck--------------------------->|
    ///   |                       |                       |
    ///   |<-BroadcastAckCount(2)-|     (n = 2; m = 2)    |
    ///   |<-BroadcastAckCount(2)-------(n = 2; m = 4)----|
    ///   |                       |                       |
    ///   |<----------------Ack---------------------------|
    ///   |<----------------Ack---|                       |
    ///   |                       |                       |
    ///   |<----------------Ack---------------------------|
    ///   |<----------------Ack---|                       |
    async fn broadcast_with_ack(
        &self,
        packet: Packet,
        opts: BroadcastOptions,
        timeout: Option<Duration>,
    ) -> Result<Self::AckStream, Self::Error> {
        if opts.is_local(self.uid) {
            tracing::debug!(?opts, "broadcast with ack is local");
            let (local, _) = self.local.broadcast_with_ack(packet, opts, timeout);
            let stream = AckStream::new_local(local);
            return Ok(stream);
        }
        let req = RequestOut::new(self.uid, RequestTypeOut::BroadcastWithAck(&packet), &opts);
        let req_id = req.id;

        let remote_serv_cnt = self.server_count().await?.saturating_sub(1);
        tracing::trace!(?remote_serv_cnt, "expecting acks from remote servers");

        let (tx, rx) = mpsc::channel(self.config.ack_response_buffer + remote_serv_cnt as usize);
        self.responses.lock().unwrap().insert(req_id, tx);
        self.send_req(req, None).await?;
        let (local, _) = self.local.broadcast_with_ack(packet, opts, timeout);

        Ok(AckStream::new(
            local,
            rx,
            self.config.request_timeout,
            remote_serv_cnt,
            req_id,
            self.responses.clone(),
        ))
    }

    async fn disconnect_socket(&self, opts: BroadcastOptions) -> Result<(), BroadcastError> {
        if !opts.is_local(self.uid) {
            let req = RequestOut::new(self.uid, RequestTypeOut::DisconnectSockets, &opts);
            self.send_req(req, None).await.map_err(AdapterError::from)?;
        }
        self.local
            .disconnect_socket(opts)
            .map_err(BroadcastError::Socket)?;

        Ok(())
    }

    async fn rooms(&self, opts: BroadcastOptions) -> Result<Vec<Room>, Self::Error> {
        if opts.is_local(self.uid) {
            return Ok(self.local.rooms(opts).into_iter().collect());
        }
        let req = RequestOut::new(self.uid, RequestTypeOut::AllRooms, &opts);
        let req_id = req.id;

        // First get the remote stream because mongodb might send
        // the responses before subscription is done.
        let stream = self
            .get_res::<()>(req_id, ResponseTypeId::AllRooms, opts.server_id)
            .await?;
        self.send_req(req, opts.server_id).await?;
        let local = self.local.rooms(opts);
        let rooms = stream
            .filter_map(|item| future::ready(item.into_rooms()))
            .fold(local, |mut acc, item| async move {
                acc.extend(item);
                acc
            })
            .await;
        Ok(Vec::from_iter(rooms))
    }

    async fn add_sockets(
        &self,
        opts: BroadcastOptions,
        rooms: impl RoomParam,
    ) -> Result<(), Self::Error> {
        let rooms: Vec<Room> = rooms.into_room_iter().collect();
        if !opts.is_local(self.uid) {
            let req = RequestOut::new(self.uid, RequestTypeOut::AddSockets(&rooms), &opts);
            self.send_req(req, opts.server_id).await?;
        }
        self.local.add_sockets(opts, rooms);
        Ok(())
    }

    async fn del_sockets(
        &self,
        opts: BroadcastOptions,
        rooms: impl RoomParam,
    ) -> Result<(), Self::Error> {
        let rooms: Vec<Room> = rooms.into_room_iter().collect();
        if !opts.is_local(self.uid) {
            let req = RequestOut::new(self.uid, RequestTypeOut::DelSockets(&rooms), &opts);
            self.send_req(req, opts.server_id).await?;
        }
        self.local.del_sockets(opts, rooms);
        Ok(())
    }

    async fn fetch_sockets(
        &self,
        opts: BroadcastOptions,
    ) -> Result<Vec<RemoteSocketData>, Self::Error> {
        if opts.is_local(self.uid) {
            return Ok(self.local.fetch_sockets(opts));
        }
        let req = RequestOut::new(self.uid, RequestTypeOut::FetchSockets, &opts);
        // First get the remote stream because mongodb might send
        // the responses before subscription is done.
        let remote = self
            .get_res::<RemoteSocketData>(req.id, ResponseTypeId::FetchSockets, opts.server_id)
            .await?;

        self.send_req(req, opts.server_id).await?;
        let local = self.local.fetch_sockets(opts);
        let sockets = remote
            .filter_map(|item| future::ready(item.into_fetch_sockets()))
            .fold(local, |mut acc, item| async move {
                acc.extend(item);
                acc
            })
            .await;
        Ok(sockets)
    }

    fn get_local(&self) -> &CoreLocalAdapter<E> {
        &self.local
    }
}

impl<E: SocketEmitter, D: Driver> CustomMongoDbAdapter<E, D> {
    async fn heartbeat_job(self: Arc<Self>) -> Result<(), Error<D>> {
        let mut interval = tokio::time::interval(self.config.hb_interval);
        interval.tick().await; // first tick yields immediately
        loop {
            interval.tick().await;
            self.emit_heartbeat(None).await?;
        }
    }

    async fn handle_ev_stream(
        self: Arc<Self>,
        mut stream: impl Stream<Item = Result<Item, D::Error>> + Unpin,
    ) {
        while let Some(item) = stream.next().await {
            match item {
                Ok(Item {
                    header: ItemHeader::Req { target, .. },
                    data,
                    ..
                }) if target.is_none_or(|id| id == self.uid) => {
                    tracing::debug!(?target, "request header");
                    if let Err(e) = self.recv_req(data).await {
                        tracing::warn!("error receiving request from driver: {e}");
                    }
                }
                Ok(Item {
                    header: ItemHeader::Req { target, .. },
                    ..
                }) => {
                    tracing::debug!(
                        ?target,
                        "receiving request which is not for us, skipping..."
                    );
                }
                Ok(
                    item @ Item {
                        header: ItemHeader::Res { request, .. },
                        ..
                    },
                ) => {
                    tracing::trace!(?request, "received response");
                    let handlers = self.responses.lock().unwrap();
                    if let Some(tx) = handlers.get(&request) {
                        if let Err(e) = tx.try_send(item) {
                            tracing::warn!("error sending response to handler: {e}");
                        }
                    } else {
                        tracing::warn!(?request, ?handlers, "could not find req handler");
                    }
                }
                Err(e) => {
                    tracing::warn!("error receiving event from driver: {e}");
                }
            }
        }
    }

    async fn recv_req(self: &Arc<Self>, req: Vec<u8>) -> Result<(), Error<D>> {
        let req = rmp_serde::from_slice::<RequestIn>(&req)?;
        tracing::trace!(?req, "incoming request");
        match req.r#type {
            RequestTypeIn::Broadcast(p) => self.recv_broadcast(req.opts, p),
            RequestTypeIn::BroadcastWithAck(_) => self.clone().recv_broadcast_with_ack(req),
            RequestTypeIn::DisconnectSockets => self.recv_disconnect_sockets(req),
            RequestTypeIn::AllRooms => self.recv_rooms(req),
            RequestTypeIn::AddSockets(rooms) => self.recv_add_sockets(req.opts, rooms),
            RequestTypeIn::DelSockets(rooms) => self.recv_del_sockets(req.opts, rooms),
            RequestTypeIn::FetchSockets => self.recv_fetch_sockets(req),
            RequestTypeIn::Heartbeat | RequestTypeIn::InitHeartbeat => self.recv_heartbeat(req),
        }
        Ok(())
    }

    fn recv_broadcast(&self, opts: BroadcastOptions, packet: Packet) {
        tracing::trace!(?opts, "incoming broadcast");
        if let Err(e) = self.local.broadcast(packet, opts) {
            let ns = self.local.path();
            tracing::warn!(?self.uid, ?ns, "remote request broadcast handler: {:?}", e);
        }
    }

    fn recv_disconnect_sockets(&self, req: RequestIn) {
        if let Err(e) = self.local.disconnect_socket(req.opts) {
            let ns = self.local.path();
            tracing::warn!(
                ?self.uid,
                ?ns,
                "remote request disconnect sockets handler: {:?}",
                e
            );
        }
    }

    fn recv_broadcast_with_ack(self: Arc<Self>, req: RequestIn) {
        let packet = match req.r#type {
            RequestTypeIn::BroadcastWithAck(p) => p,
            _ => unreachable!(),
        };
        let (stream, count) = self.local.broadcast_with_ack(packet, req.opts, None);
        tokio::spawn(async move {
            let on_err = |err| {
                let ns = self.local.path();
                tracing::warn!(
                    ?self.uid,
                    ?ns,
                    "remote request broadcast with ack handler errors: {:?}",
                    err
                );
            };
            // First send the count of expected acks to the server that sent the request.
            // This is used to keep track of the number of expected acks.
            let res = Response {
                r#type: ResponseType::<()>::BroadcastAckCount(count),
                node_id: self.uid,
            };
            if let Err(err) = self.send_res(req.id, req.node_id, res).await {
                on_err(err);
                return;
            }

            // Then send the acks as they are received.
            futures_util::pin_mut!(stream);
            while let Some(ack) = stream.next().await {
                let res = Response {
                    r#type: ResponseType::BroadcastAck(ack),
                    node_id: self.uid,
                };
                if let Err(err) = self.send_res(req.id, req.node_id, res).await {
                    on_err(err);
                    return;
                }
            }
        });
    }

    fn recv_rooms(&self, req: RequestIn) {
        let rooms = self.local.rooms(req.opts);
        let res = Response {
            r#type: ResponseType::<()>::AllRooms(rooms),
            node_id: self.uid,
        };
        let fut = self.send_res(req.id, req.node_id, res);
        let ns = self.local.path().clone();
        let uid = self.uid;
        tokio::spawn(async move {
            if let Err(err) = fut.await {
                tracing::warn!(?uid, ?ns, "remote request rooms handler: {:?}", err);
            }
        });
    }

    fn recv_add_sockets(&self, opts: BroadcastOptions, rooms: Vec<Room>) {
        self.local.add_sockets(opts, rooms);
    }

    fn recv_del_sockets(&self, opts: BroadcastOptions, rooms: Vec<Room>) {
        self.local.del_sockets(opts, rooms);
    }
    fn recv_fetch_sockets(&self, req: RequestIn) {
        let sockets = self.local.fetch_sockets(req.opts);
        let res = Response {
            node_id: self.uid,
            r#type: ResponseType::FetchSockets(sockets),
        };
        let fut = self.send_res(req.id, req.node_id, res);
        let ns = self.local.path().clone();
        let uid = self.uid;
        tokio::spawn(async move {
            if let Err(err) = fut.await {
                tracing::warn!(?uid, ?ns, "remote request fetch sockets handler: {:?}", err);
            }
        });
    }

    /// Receive a heartbeat from a remote node.
    /// It might be a FirstHeartbeat packet, in which case we are re-emitting a heartbeat to the remote node.
    fn recv_heartbeat(self: &Arc<Self>, req: RequestIn) {
        tracing::debug!(?req.node_id, "{:?} received", req.r#type);
        let mut node_liveness = self.nodes_liveness.lock().unwrap();
        // Even with a FirstHeartbeat packet we first consume the node liveness to
        // ensure that the node is not already in the list.
        for (id, liveness) in node_liveness.iter_mut() {
            if *id == req.node_id {
                *liveness = Instant::now();
                return;
            }
        }

        node_liveness.push((req.node_id, Instant::now()));

        if matches!(req.r#type, RequestTypeIn::InitHeartbeat) {
            tracing::debug!(?req.node_id, "initial heartbeat detected, saying hello to the new node");

            let this = self.clone();
            tokio::spawn(async move {
                if let Err(err) = this.emit_heartbeat(Some(req.node_id)).await {
                    tracing::warn!(
                        "could not re-emit heartbeat after new node detection: {:?}",
                        err
                    );
                }
            });
        }
    }

    /// Send a request to a specific target node or broadcast it to all nodes if no target is specified.
    async fn send_req(&self, req: RequestOut<'_>, target: Option<Uid>) -> Result<(), Error<D>> {
        tracing::trace!(?req, "sending request");
        let head = ItemHeader::Req { target };
        let req = self.new_packet(head, &req)?;
        self.driver.emit(&req).await.map_err(Error::from_driver)?;
        Ok(())
    }

    /// Send a response to the node that sent the request.
    fn send_res<T: Serialize + fmt::Debug>(
        &self,
        req_id: Sid,
        req_origin: Uid,
        res: Response<T>,
    ) -> impl Future<Output = Result<(), Error<D>>> + Send + 'static {
        tracing::trace!(?res, "sending response for {req_id} req to {req_origin}");
        let driver = self.driver.clone();
        let head = ItemHeader::Res {
            request: req_id,
            target: req_origin,
        };
        let res = self.new_packet(head, &res);

        async move {
            driver.emit(&res?).await.map_err(Error::from_driver)?;
            Ok(())
        }
    }

    /// Await for all the responses from the remote servers.
    /// If the target node is specified, only await for the response from that node.
    async fn get_res<T: DeserializeOwned + fmt::Debug>(
        &self,
        req_id: Sid,
        response_type: ResponseTypeId,
        target: Option<Uid>,
    ) -> Result<impl Stream<Item = Response<T>>, Error<D>> {
        // Check for specific target node
        let remote_serv_cnt = if target.is_none() {
            self.server_count().await?.saturating_sub(1) as usize
        } else {
            1
        };
        let (tx, rx) = mpsc::channel(std::cmp::max(remote_serv_cnt, 1));
        self.responses.lock().unwrap().insert(req_id, tx);
        let stream = ChanStream::new(rx)
            .filter_map(|Item { header, data, .. }| {
                let data = match rmp_serde::from_slice::<Response<T>>(&data) {
                    Ok(data) => Some(data),
                    Err(e) => {
                        tracing::warn!(header = ?header, "error decoding response: {e}");
                        None
                    }
                };
                future::ready(data)
            })
            .filter(move |item| future::ready(ResponseTypeId::from(&item.r#type) == response_type))
            .take(remote_serv_cnt)
            .take_until(tokio::time::sleep(self.config.request_timeout));
        let stream = DropStream::new(stream, self.responses.clone(), req_id);
        Ok(stream)
    }

    /// Emit a heartbeat to the specified target node or broadcast to all nodes.
    async fn emit_heartbeat(&self, target: Option<Uid>) -> Result<(), Error<D>> {
        // Send heartbeat when starting.
        const HB_OPTS: BroadcastOptions = BroadcastOptions::new_empty();
        self.send_req(
            RequestOut::new(self.uid, RequestTypeOut::Heartbeat, &HB_OPTS),
            target,
        )
        .await
    }

    /// Emit an initial heartbeat to all nodes.
    async fn emit_init_heartbeat(&self) -> Result<(), Error<D>> {
        // Send initial heartbeat when starting.
        const HB_OPTS: BroadcastOptions = BroadcastOptions::new_empty();
        self.send_req(
            RequestOut::new(self.uid, RequestTypeOut::InitHeartbeat, &HB_OPTS),
            None,
        )
        .await
    }
    fn new_packet(&self, head: ItemHeader, data: &impl Serialize) -> Result<Item, Error<D>> {
        let ns = &self.local.path();
        let uid = self.uid;
        match self.config.expiration_strategy {
            #[cfg(feature = "ttl-index")]
            MessageExpirationStrategy::TtlIndex(_) => Ok(Item::new_ttl(head, data, uid, ns)?),
            MessageExpirationStrategy::CappedCollection(_) => Ok(Item::new(head, data, uid, ns)?),
        }
    }
}

/// The result of the init future.
#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct InitRes<D: Driver>(futures_core::future::BoxFuture<'static, Result<(), D::Error>>);

impl<D: Driver> Future for InitRes<D> {
    type Output = Result<(), D::Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.0.as_mut().poll(cx)
    }
}
impl<D: Driver> Spawnable for InitRes<D> {
    fn spawn(self) {
        tokio::spawn(async move {
            if let Err(e) = self.0.await {
                tracing::error!("error initializing adapter: {e}");
            }
        });
    }
}
