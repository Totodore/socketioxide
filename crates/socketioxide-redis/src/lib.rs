#![cfg_attr(docsrs, feature(doc_cfg))]
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

//! # A redis/valkey adapter implementation for the socketioxide crate.
//! The adapter is used to communicate with other nodes of the same application.
//! This allows to broadcast messages to sockets connected on other servers,
//! to get the list of rooms, to add or remove sockets from rooms, etc.
//!
//! To achieve this, the adapter uses a [pub/sub](https://redis.io/docs/latest/develop/interact/pubsub/) system
//! through Redis to communicate with other servers.
//!
//! The [`Driver`] abstraction allows the use of any pub/sub client.
//! Three implementations are provided:
//! * [`RedisDriver`](crate::drivers::redis::RedisDriver) for the [`redis`] crate with a standalone redis.
//! * [`ClusterDriver`](crate::drivers::redis::ClusterDriver) for the [`redis`] crate with a redis cluster.
//! * [`FredDriver`](crate::drivers::fred::FredDriver) for the [`fred`] crate with a standalone/cluster redis.
//!
//! When using redis clusters, the drivers employ [sharded pub/sub](https://redis.io/docs/latest/develop/interact/pubsub/#sharded-pubsub)
//! to distribute the load across Redis nodes.
//!
//! You can also implement your own driver by implementing the [`Driver`] trait.
//!
//! <div class="warning">
//!     The provided driver implementations are using <code>RESP3</code> for efficiency purposes.
//!     Make sure your redis server supports it (redis v7 and above).
//!     If not, you can implement your own driver using the <code>RESP2</code> protocol.
//! </div>
//!
//! ## Example with the [`redis`] driver
//! ```rust
//! # use socketioxide::{SocketIo, extract::{SocketRef, Data}, adapter::Adapter};
//! # use socketioxide_redis::{RedisAdapterCtr, RedisAdapter};
//! # async fn doc_main() -> Result<(), Box<dyn std::error::Error>> {
//! async fn on_connect<A: Adapter>(socket: SocketRef<A>) {
//!     socket.join("room1");
//!     socket.on("event", on_event);
//!     let _ = socket.broadcast().emit("hello", "world").await.ok();
//! }
//! async fn on_event<A: Adapter>(socket: SocketRef<A>, Data(data): Data<String>) {}
//!
//! let client = redis::Client::open("redis://127.0.0.1:6379?protocol=RESP3")?;
//! let adapter = RedisAdapterCtr::new_with_redis(&client).await?;
//! let (layer, io) = SocketIo::builder()
//!     .with_adapter::<RedisAdapter<_>>(adapter)
//!     .build_layer();
//! Ok(())
//! # }
//! ```
//!
//!
//! ## Example with the [`fred`] driver
//! ```rust
//! # use socketioxide::{SocketIo, extract::{SocketRef, Data}, adapter::Adapter};
//! # use socketioxide_redis::{RedisAdapterCtr, FredAdapter};
//! # use fred::types::RespVersion;
//! # async fn doc_main() -> Result<(), Box<dyn std::error::Error>> {
//! async fn on_connect<A: Adapter>(socket: SocketRef<A>) {
//!     socket.join("room1");
//!     socket.on("event", on_event);
//!     let _ = socket.broadcast().emit("hello", "world").await.ok();
//! }
//! async fn on_event<A: Adapter>(socket: SocketRef<A>, Data(data): Data<String>) {}
//!
//! let mut config = fred::prelude::Config::from_url("redis://127.0.0.1:6379?protocol=resp3")?;
//! // We need to manually set the RESP3 version because
//! // the fred crate does not parse the protocol query parameter.
//! config.version = RespVersion::RESP3;
//! let client = fred::prelude::Builder::from_config(config).build_subscriber_client()?;
//! let adapter = RedisAdapterCtr::new_with_fred(client).await?;
//! let (layer, io) = SocketIo::builder()
//!     .with_adapter::<FredAdapter<_>>(adapter)
//!     .build_layer();
//! Ok(())
//! # }
//! ```
//!
//!
//! ## Example with the [`redis`] cluster driver
//! ```rust
//! # use socketioxide::{SocketIo, extract::{SocketRef, Data}, adapter::Adapter};
//! # use socketioxide_redis::{RedisAdapterCtr, ClusterAdapter};
//! # async fn doc_main() -> Result<(), Box<dyn std::error::Error>> {
//! async fn on_connect<A: Adapter>(socket: SocketRef<A>) {
//!     socket.join("room1");
//!     socket.on("event", on_event);
//!     let _ = socket.broadcast().emit("hello", "world").await.ok();
//! }
//! async fn on_event<A: Adapter>(socket: SocketRef<A>, Data(data): Data<String>) {}
//!
//! // single node cluster
//! let builder = redis::cluster::ClusterClient::builder(std::iter::once(
//!     "redis://127.0.0.1:6379?protocol=resp3",
//! ));
//! let adapter = RedisAdapterCtr::new_with_cluster(builder).await?;

//! let (layer, io) = SocketIo::builder()
//!     .with_adapter::<ClusterAdapter<_>>(adapter)
//!     .build_layer();
//! Ok(())
//! # }
//! ```
//!
//! ## How does it work?
//!
//! An adapter is created for each created namespace and it takes a corresponding [`CoreLocalAdapter`].
//! The [`CoreLocalAdapter`] allows to manage the local rooms and local sockets. The default `LocalAdapter`
//! is simply a wrapper around this [`CoreLocalAdapter`].
//!
//! The adapter is then initialized with the [`RedisAdapter::init`] method.
//! This will subscribe to 3 channels:
//! * `"{prefix}-request#{namespace}#"`: A global channel to receive broadcasted requests.
//! * `"{prefix}-request#{namespace}#{uid}#"`: A specific channel to receive requests only for this server.
//! * `"{prefix}-response#{namespace}#{uid}#"`: A specific channel to receive responses only for this server.
//!     Messages sent to this channel will be always in the form `[req_id, data]`. This will allow the adapter to extract the request id
//!     and route the response to the approriate stream before deserializing the data.
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
//!
//! For ack streams, the adapter will first send a `BroadcastAckCount` response to the server that sent the request,
//! and then send the acks as they are received (more details in [`RedisAdapter::broadcast_with_ack`] fn).
//!
//! On the other side, each time an action has to be performed on the local server, the adapter will
//! first broadcast a request to all the servers and then perform the action locally.

use std::{
    borrow::Cow,
    collections::HashMap,
    fmt,
    future::{self, Future},
    sync::{Arc, Mutex},
    time::Duration,
};

use drivers::{ChanItem, Driver, MessageStream};
use futures_core::Stream;
use futures_util::StreamExt;
use request::{
    read_req_id, RequestIn, RequestOut, RequestTypeIn, RequestTypeOut, Response, ResponseType,
};
use serde::{de::DeserializeOwned, Serialize};
use socketioxide_core::{
    adapter::{
        BroadcastFlags, BroadcastOptions, CoreAdapter, CoreLocalAdapter, RemoteSocketData, Room,
        RoomParam, SocketEmitter,
    },
    errors::{AdapterError, BroadcastError},
    packet::Packet,
    Sid, Uid,
};
use stream::{AckStream, DropStream};
use tokio::{sync::mpsc, time};

/// Drivers are an abstraction over the pub/sub backend used by the adapter.
/// You can use the provided implementation or implement your own.
pub mod drivers;

mod request;
mod stream;

/// Represent any error that might happen when using this adapter.
#[derive(thiserror::Error)]
pub enum Error<R: Driver> {
    /// Redis driver error
    #[error("driver error: {0}")]
    Driver(R::Error),
    /// Packet encoding error
    #[error("packet encoding error: {0}")]
    Decode(#[from] rmp_serde::decode::Error),
    /// Packet decoding error
    #[error("packet decoding error: {0}")]
    Encode(#[from] rmp_serde::encode::Error),
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

/// The configuration of the [`RedisAdapter`].
#[derive(Debug, Clone)]
pub struct RedisAdapterConfig {
    /// The request timeout. It is mainly used when expecting response such as when using
    /// `broadcast_with_ack` or `rooms`. Default is 5 seconds.
    pub request_timeout: Duration,

    /// The prefix used for the channels. Default is "socket.io".
    pub prefix: Cow<'static, str>,

    /// The channel size used to receive ack responses. Default is 255.
    ///
    /// If you have a lot of servers/sockets and that you may miss acknowledgement because they arrive faster
    /// than you poll them with the returned stream, you might want to increase this value.
    pub ack_response_buffer: usize,

    /// The channel size used to receive messages. Default is 1024.
    ///
    /// If your server is under heavy load, you might want to increase this value.
    pub stream_buffer: usize,
}
impl RedisAdapterConfig {
    /// Create a new config.
    pub fn new() -> Self {
        Self::default()
    }
    /// Set the request timeout. Default is 5 seconds.
    pub fn with_request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    /// Set the prefix used for the channels. Default is "socket.io".
    pub fn with_prefix(mut self, prefix: impl Into<Cow<'static, str>>) -> Self {
        self.prefix = prefix.into();
        self
    }

    /// Set the channel size used to send ack responses. Default is 255.
    ///
    /// If you have a lot of servers/sockets and that you may miss acknowledgement because they arrive faster
    /// than you poll them with the returned stream, you might want to increase this value.
    pub fn with_ack_response_buffer(mut self, buffer: usize) -> Self {
        assert!(buffer > 0, "buffer size must be greater than 0");
        self.ack_response_buffer = buffer;
        self
    }

    /// Set the channel size used to receive messages. Default is 1024.
    ///
    /// If your server is under heavy load, you might want to increase this value.
    pub fn with_stream_buffer(mut self, buffer: usize) -> Self {
        assert!(buffer > 0, "buffer size must be greater than 0");
        self.stream_buffer = buffer;
        self
    }
}

impl Default for RedisAdapterConfig {
    fn default() -> Self {
        Self {
            request_timeout: Duration::from_secs(5),
            prefix: Cow::Borrowed("socket.io"),
            ack_response_buffer: 255,
            stream_buffer: 1024,
        }
    }
}

/// The adapter constructor. For each namespace you define, a new adapter instance is created
/// from this constructor.
#[derive(Debug)]
pub struct RedisAdapterCtr<R> {
    driver: R,
    config: RedisAdapterConfig,
}

#[cfg(feature = "redis")]
impl RedisAdapterCtr<drivers::redis::RedisDriver> {
    /// Create a new adapter constructor with the [`redis`] driver and a default config.
    #[cfg_attr(docsrs, doc(cfg(feature = "redis")))]
    pub async fn new_with_redis(client: &redis::Client) -> redis::RedisResult<Self> {
        Self::new_with_redis_config(client, RedisAdapterConfig::default()).await
    }
    /// Create a new adapter constructor with the [`redis`] driver and a custom config.
    #[cfg_attr(docsrs, doc(cfg(feature = "redis")))]
    pub async fn new_with_redis_config(
        client: &redis::Client,
        config: RedisAdapterConfig,
    ) -> redis::RedisResult<Self> {
        let driver = drivers::redis::RedisDriver::new(client).await?;
        Ok(Self::new_with_driver(driver, config))
    }
}
#[cfg(feature = "redis-cluster")]
impl RedisAdapterCtr<drivers::redis::ClusterDriver> {
    /// Create a new adapter constructor with the [`redis`] driver and a default config.
    #[cfg_attr(docsrs, doc(cfg(feature = "redis-cluster")))]
    pub async fn new_with_cluster(
        builder: redis::cluster::ClusterClientBuilder,
    ) -> redis::RedisResult<Self> {
        Self::new_with_cluster_config(builder, RedisAdapterConfig::default()).await
    }

    /// Create a new adapter constructor with the [`redis`] driver and a default config.
    #[cfg_attr(docsrs, doc(cfg(feature = "redis-cluster")))]
    pub async fn new_with_cluster_config(
        builder: redis::cluster::ClusterClientBuilder,
        config: RedisAdapterConfig,
    ) -> redis::RedisResult<Self> {
        let driver = drivers::redis::ClusterDriver::new(builder).await?;
        Ok(Self::new_with_driver(driver, config))
    }
}
#[cfg(feature = "fred")]
impl RedisAdapterCtr<drivers::fred::FredDriver> {
    /// Create a new adapter constructor with the default [`fred`] driver and a default config.
    #[cfg_attr(docsrs, doc(cfg(feature = "fred")))]
    pub async fn new_with_fred(
        client: fred::clients::SubscriberClient,
    ) -> fred::prelude::FredResult<Self> {
        Self::new_with_fred_config(client, RedisAdapterConfig::default()).await
    }
    /// Create a new adapter constructor with the default [`fred`] driver and a custom config.
    #[cfg_attr(docsrs, doc(cfg(feature = "fred")))]
    pub async fn new_with_fred_config(
        client: fred::clients::SubscriberClient,
        config: RedisAdapterConfig,
    ) -> fred::prelude::FredResult<Self> {
        let driver = drivers::fred::FredDriver::new(client).await?;
        Ok(Self::new_with_driver(driver, config))
    }
}
impl<R: Driver> RedisAdapterCtr<R> {
    /// Create a new adapter constructor with a custom redis/valkey driver and a config.
    ///
    /// You can implement your own driver by implementing the [`Driver`] trait with any redis/valkey client.
    /// Check the [`drivers`] module for more information.
    pub fn new_with_driver(driver: R, config: RedisAdapterConfig) -> RedisAdapterCtr<R> {
        RedisAdapterCtr { driver, config }
    }
}

pub(crate) type ResponseHandlers = HashMap<Sid, mpsc::Sender<Vec<u8>>>;

/// The redis adapter with the fred driver.
#[cfg_attr(docsrs, doc(cfg(feature = "fred")))]
#[cfg(feature = "fred")]
pub type FredAdapter<E> = CustomRedisAdapter<E, drivers::fred::FredDriver>;

/// The redis adapter with the redis driver.
#[cfg_attr(docsrs, doc(cfg(feature = "redis")))]
#[cfg(feature = "redis")]
pub type RedisAdapter<E> = CustomRedisAdapter<E, drivers::redis::RedisDriver>;

/// The redis adapter with the redis cluster driver.
#[cfg_attr(docsrs, doc(cfg(feature = "redis-cluster")))]
#[cfg(feature = "redis-cluster")]
pub type ClusterAdapter<E> = CustomRedisAdapter<E, drivers::redis::ClusterDriver>;

/// The redis adapter implementation.
/// It is generic over the [`Driver`] used to communicate with the redis server.
/// And over the [`SocketEmitter`] used to communicate with the local server. This allows to
/// avoid cyclic dependencies between the adapter, `socketioxide-core` and `socketioxide` crates.
pub struct CustomRedisAdapter<E, R> {
    /// The driver used by the adapter. This is used to communicate with the redis server.
    /// All the redis adapter instances share the same driver.
    driver: R,
    /// The configuration of the adapter.
    config: RedisAdapterConfig,
    /// A unique identifier for the adapter to identify itself in the redis server.
    uid: Uid,
    /// The local adapter, used to manage local rooms and socket stores.
    local: CoreLocalAdapter<E>,
    /// The request channel used to broadcast requests to all the servers.
    /// format: `{prefix}-request#{path}#`.
    req_chan: String,
    /// A map of response handlers used to await for responses from the remote servers.
    responses: Arc<Mutex<ResponseHandlers>>,
}

impl<E: SocketEmitter, R: Driver> CoreAdapter<E> for CustomRedisAdapter<E, R> {
    type Error = Error<R>;
    type State = RedisAdapterCtr<R>;
    type AckStream = AckStream<E::AckStream>;

    fn new(state: &Self::State, local: CoreLocalAdapter<E>) -> Self {
        let req_chan = format!("{}-request#{}#", state.config.prefix, local.path());
        let uid = local.server_id();
        Self {
            local,
            req_chan,
            uid,
            driver: state.driver.clone(),
            config: state.config.clone(),
            responses: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    async fn init(self: Arc<Self>) -> Result<(), Self::Error> {
        let global_stream = self.subscribe(self.req_chan.clone()).await?;
        let specific_stream = self.subscribe(self.get_req_chan(Some(self.uid))).await?;
        let response_chan = format!(
            "{}-response#{}#{}#",
            &self.config.prefix,
            self.local.path(),
            self.uid
        );

        let response_stream = self.subscribe(response_chan.clone()).await?;
        tokio::spawn(async move {
            let stream = futures_util::stream::select(global_stream, specific_stream);
            let mut stream = futures_util::stream::select(stream, response_stream);
            while let Some((chan, item)) = stream.next().await {
                if chan.starts_with(&self.req_chan) {
                    if let Err(e) = self.recv_req(item) {
                        let ns = self.local.path();
                        let uid = self.uid;
                        tracing::warn!(?uid, ?ns, "request handler error: {e}");
                    }
                } else if chan == response_chan {
                    let req_id = read_req_id(&item);
                    tracing::trace!(?req_id, ?chan, ?response_chan, "extracted sid");
                    let handlers = self.responses.lock().unwrap();
                    if let Some(tx) = req_id.and_then(|id| handlers.get(&id)) {
                        if let Err(e) = tx.try_send(item) {
                            tracing::warn!("error sending response to handler: {e}");
                        }
                    } else {
                        tracing::warn!(?req_id, "could not find req handler");
                    }
                } else {
                    tracing::warn!("unexpected message/channel: {chan}");
                }
            }
        });
        Ok(())
    }

    async fn close(&self) -> Result<(), Self::Error> {
        let response_chan = format!(
            "{}-response#{}#{}#",
            &self.config.prefix,
            self.local.path(),
            self.uid
        );
        tokio::try_join!(
            self.driver.unsubscribe(self.req_chan.clone()),
            self.driver.unsubscribe(self.get_req_chan(Some(self.uid))),
            self.driver.unsubscribe(response_chan)
        )
        .map_err(Error::from_driver)?;

        Ok(())
    }

    /// Get the number of servers by getting the number of subscribers to the request channel.
    async fn server_count(&self) -> Result<u16, Self::Error> {
        let count = self
            .driver
            .num_serv(&self.req_chan)
            .await
            .map_err(Error::from_driver)?;

        Ok(count)
    }

    /// Broadcast a packet to all the servers to send them through their sockets.
    async fn broadcast(
        &self,
        packet: Packet,
        opts: BroadcastOptions,
    ) -> Result<(), BroadcastError> {
        if !is_local_op(self.uid, &opts) {
            let req = RequestOut::new(self.uid, RequestTypeOut::Broadcast(&packet), &opts);
            self.send_req(req, opts.server_id)
                .await
                .map_err(AdapterError::from)?;
        }

        self.local.broadcast(packet, opts)?;
        Ok(())
    }

    /// Broadcast a packet to all the servers to send them through their sockets.
    ///
    /// Returns a Stream that is a combination of the local ack stream and a remote [`MessageStream`].
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
        if is_local_op(self.uid, &opts) {
            let (local, _) = self.local.broadcast_with_ack(packet, opts, timeout);
            let stream = AckStream::new_local(local);
            return Ok(stream);
        }
        let req = RequestOut::new(self.uid, RequestTypeOut::BroadcastWithAck(&packet), &opts);
        let req_id = req.id;

        let remote_serv_cnt = self.server_count().await?.saturating_sub(1);

        let (tx, rx) = mpsc::channel(self.config.ack_response_buffer + remote_serv_cnt as usize);
        self.responses.lock().unwrap().insert(req_id, tx);
        let remote = MessageStream::new(rx);

        self.send_req(req, opts.server_id).await?;
        let (local, _) = self.local.broadcast_with_ack(packet, opts, timeout);

        Ok(AckStream::new(
            local,
            remote,
            self.config.request_timeout,
            remote_serv_cnt,
            req_id,
            self.responses.clone(),
        ))
    }

    async fn disconnect_socket(&self, opts: BroadcastOptions) -> Result<(), BroadcastError> {
        if !is_local_op(self.uid, &opts) {
            let req = RequestOut::new(self.uid, RequestTypeOut::DisconnectSockets, &opts);
            self.send_req(req, opts.server_id)
                .await
                .map_err(AdapterError::from)?;
        }
        self.local
            .disconnect_socket(opts)
            .map_err(BroadcastError::Socket)?;

        Ok(())
    }

    async fn rooms(&self, opts: BroadcastOptions) -> Result<Vec<Room>, Self::Error> {
        const PACKET_IDX: u8 = 2;

        if is_local_op(self.uid, &opts) {
            return Ok(self.local.rooms(opts).into_iter().collect());
        }
        let req = RequestOut::new(self.uid, RequestTypeOut::AllRooms, &opts);
        let req_id = req.id;

        // First get the remote stream because redis might send
        // the responses before subscription is done.
        let stream = self.get_res::<()>(req_id, PACKET_IDX).await?;
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
        if !is_local_op(self.uid, &opts) {
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
        if !is_local_op(self.uid, &opts) {
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
        if is_local_op(self.uid, &opts) {
            return Ok(self.local.fetch_sockets(opts));
        }
        const PACKET_IDX: u8 = 3;
        let req = RequestOut::new(self.uid, RequestTypeOut::FetchSockets, &opts);
        let req_id = req.id;
        // First get the remote stream because redis might send
        // the responses before subscription is done.
        let remote = self.get_res::<RemoteSocketData>(req_id, PACKET_IDX).await?;

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

impl<E: SocketEmitter, R: Driver> CustomRedisAdapter<E, R> {
    /// Build a response channel for a request.
    ///
    /// The uid is used to identify the server that sent the request.
    /// The req_id is used to identify the request.
    fn get_res_chan(&self, uid: Uid) -> String {
        let path = self.local.path();
        let prefix = &self.config.prefix;
        format!("{}-response#{}#{}#", prefix, path, uid)
    }
    /// Build a request channel for a request.
    ///
    /// If we know the target server id, we can build a channel specific to this server.
    /// Otherwise, we use the default request channel that will broadcast the request to all the servers.
    fn get_req_chan(&self, node_id: Option<Uid>) -> String {
        match node_id {
            Some(uid) => format!("{}{}#", self.req_chan, uid),
            None => self.req_chan.clone(),
        }
    }

    /// Handle a generic request received from the request channel.
    fn recv_req(self: &Arc<Self>, item: Vec<u8>) -> Result<(), Error<R>> {
        let req: RequestIn = rmp_serde::from_slice(&item)?;
        if req.node_id == self.uid {
            return Ok(());
        }

        tracing::trace!(?req, "handling request");

        match req.r#type {
            RequestTypeIn::Broadcast(p) => self.recv_broadcast(req.opts, p),
            RequestTypeIn::BroadcastWithAck(_) => self.clone().recv_broadcast_with_ack(req),
            RequestTypeIn::DisconnectSockets => self.recv_disconnect_sockets(req),
            RequestTypeIn::AllRooms => self.recv_rooms(req),
            RequestTypeIn::AddSockets(rooms) => self.recv_add_sockets(req.opts, rooms),
            RequestTypeIn::DelSockets(rooms) => self.recv_del_sockets(req.opts, rooms),
            RequestTypeIn::FetchSockets => self.recv_fetch_sockets(req),
        };
        Ok(())
    }

    fn recv_broadcast(&self, opts: BroadcastOptions, packet: Packet) {
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
            if let Err(err) = self.send_res(req.node_id, req.id, res).await {
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
                if let Err(err) = self.send_res(req.node_id, req.id, res).await {
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
        let fut = self.send_res(req.node_id, req.id, res);
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
        let fut = self.send_res(req.node_id, req.id, res);
        let ns = self.local.path().clone();
        let uid = self.uid;
        tokio::spawn(async move {
            if let Err(err) = fut.await {
                tracing::warn!(?uid, ?ns, "remote request fetch sockets handler: {:?}", err);
            }
        });
    }

    async fn send_req(&self, req: RequestOut<'_>, target_uid: Option<Uid>) -> Result<(), Error<R>> {
        tracing::trace!(?req, "sending request");
        let req = rmp_serde::to_vec(&req)?;
        let chan = self.get_req_chan(target_uid);
        self.driver
            .publish(chan, req)
            .await
            .map_err(Error::from_driver)?;

        Ok(())
    }

    fn send_res<D: Serialize + fmt::Debug>(
        &self,
        req_node_id: Uid,
        req_id: Sid,
        res: Response<D>,
    ) -> impl Future<Output = Result<(), Error<R>>> + Send + 'static {
        let chan = self.get_res_chan(req_node_id);
        tracing::trace!(?res, "sending response to {}", &chan);
        // We send the req_id separated from the response object.
        // This allows to partially decode the response and route by the req_id
        // before fully deserializing it.
        let res = rmp_serde::to_vec(&(req_id, res));
        let driver = self.driver.clone();
        async move {
            driver
                .publish(chan, res?)
                .await
                .map_err(Error::from_driver)?;
            Ok(())
        }
    }

    /// Await for all the responses from the remote servers.
    async fn get_res<D: DeserializeOwned + fmt::Debug>(
        &self,
        req_id: Sid,
        response_idx: u8,
    ) -> Result<impl Stream<Item = Response<D>>, Error<R>> {
        let remote_serv_cnt = self.server_count().await?.saturating_sub(1) as usize;
        let (tx, rx) = mpsc::channel(std::cmp::max(remote_serv_cnt, 1));
        self.responses.lock().unwrap().insert(req_id, tx);
        let stream = MessageStream::new(rx)
            .filter_map(|item| {
                let data = match rmp_serde::from_slice::<(Sid, Response<D>)>(&item) {
                    Ok((_, data)) => Some(data),
                    Err(e) => {
                        tracing::warn!("error decoding response: {e}");
                        None
                    }
                };
                future::ready(data)
            })
            .filter(move |item| future::ready(item.r#type.to_u8() == response_idx))
            .take(remote_serv_cnt)
            .take_until(time::sleep(self.config.request_timeout));
        let stream = DropStream::new(stream, self.responses.clone(), req_id);
        Ok(stream)
    }

    /// Little wrapper to map the error type.
    #[inline]
    async fn subscribe(&self, pat: String) -> Result<MessageStream<ChanItem>, Error<R>> {
        tracing::trace!(?pat, "subscribing to");
        self.driver
            .subscribe(pat, self.config.stream_buffer)
            .await
            .map_err(Error::from_driver)
    }
}

/// A local operator is either something that is flagged as local or a request that should be specifically
/// sent to the current server.
#[inline]
fn is_local_op(uid: Uid, opts: &BroadcastOptions) -> bool {
    opts.has_flag(BroadcastFlags::Local)
        || (!opts.has_flag(BroadcastFlags::Broadcast)
            && opts.server_id == Some(uid)
            && opts.rooms.is_empty()
            && opts.sid.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::stream::{self, FusedStream, StreamExt};
    use socketioxide_core::{adapter::AckStreamItem, Str, Value};
    use std::convert::Infallible;

    #[derive(Clone)]
    struct StubDriver;
    impl Driver for StubDriver {
        type Error = Infallible;

        async fn publish(&self, _: String, _: Vec<u8>) -> Result<(), Self::Error> {
            Ok(())
        }

        async fn subscribe(
            &self,
            _: String,
            _: usize,
        ) -> Result<MessageStream<ChanItem>, Self::Error> {
            Ok(MessageStream::new_empty())
        }

        async fn unsubscribe(&self, _: String) -> Result<(), Self::Error> {
            Ok(())
        }

        async fn num_serv(&self, _: &str) -> Result<u16, Self::Error> {
            Ok(0)
        }
    }
    fn new_stub_ack_stream(
        remote: MessageStream<Vec<u8>>,
        timeout: Duration,
    ) -> AckStream<stream::Empty<AckStreamItem<()>>> {
        AckStream::new(
            stream::empty::<AckStreamItem<()>>(),
            remote,
            timeout,
            2,
            Sid::new(),
            Arc::new(Mutex::new(HashMap::new())),
        )
    }

    //TODO: test weird behaviours, packets out of orders, etc
    #[tokio::test]
    async fn ack_stream() {
        let (tx, rx) = tokio::sync::mpsc::channel(255);
        let remote = MessageStream::new(rx);
        let stream = new_stub_ack_stream(remote, Duration::from_secs(10));
        let node_id = Uid::new();
        let req_id = Sid::new();

        // The two servers will send 2 acks each.
        let ack_cnt_res = Response::<()> {
            node_id,
            r#type: ResponseType::BroadcastAckCount(2),
        };
        tx.try_send(rmp_serde::to_vec(&(req_id, &ack_cnt_res)).unwrap())
            .unwrap();
        tx.try_send(rmp_serde::to_vec(&(req_id, &ack_cnt_res)).unwrap())
            .unwrap();

        let ack_res = Response::<String> {
            node_id,
            r#type: ResponseType::BroadcastAck((Sid::new(), Ok(Value::Str(Str::from(""), None)))),
        };
        for _ in 0..4 {
            tx.try_send(rmp_serde::to_vec(&(req_id, &ack_res)).unwrap())
                .unwrap();
        }
        futures_util::pin_mut!(stream);
        for _ in 0..4 {
            assert!(stream.next().await.is_some());
        }
        assert!(stream.is_terminated());
    }

    #[tokio::test]
    async fn ack_stream_timeout() {
        let (tx, rx) = tokio::sync::mpsc::channel(255);
        let remote = MessageStream::new(rx);
        let stream = new_stub_ack_stream(remote, Duration::from_millis(50));
        let node_id = Uid::new();
        let req_id = Sid::new();
        // There will be only one ack count and then the stream will timeout.
        let ack_cnt_res = Response::<()> {
            node_id,
            r#type: ResponseType::BroadcastAckCount(2),
        };
        tx.try_send(rmp_serde::to_vec(&(req_id, ack_cnt_res)).unwrap())
            .unwrap();

        futures_util::pin_mut!(stream);
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(stream.next().await.is_none());
        assert!(stream.is_terminated());
    }

    #[tokio::test]
    async fn ack_stream_drop() {
        let (tx, rx) = tokio::sync::mpsc::channel(255);
        let remote = MessageStream::new(rx);
        let handlers = Arc::new(Mutex::new(HashMap::new()));
        let id = Sid::new();
        handlers.lock().unwrap().insert(id, tx);
        let stream = AckStream::new(
            stream::empty::<AckStreamItem<()>>(),
            remote,
            Duration::from_secs(10),
            2,
            id,
            handlers.clone(),
        );
        drop(stream);
        assert!(handlers.lock().unwrap().is_empty(),);
    }

    #[test]
    fn test_is_local_op() {
        let server_id = Uid::new();
        let remote = RemoteSocketData {
            id: Sid::new(),
            server_id,
            ns: "/".into(),
        };
        let opts = BroadcastOptions::new_remote(&remote);
        assert!(is_local_op(server_id, &opts));
        assert!(!is_local_op(Uid::new(), &opts));
        let opts = BroadcastOptions::new(Sid::new());
        assert!(!is_local_op(Uid::new(), &opts));
    }
}
