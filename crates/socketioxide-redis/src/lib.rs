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

//! # A redis adapter implementation for the socketioxide crate.
//! The adapter is used to communicate with other nodes of the same application.
//! This allows to broadcast messages to sockets connected on other servers,
//! to get the list of rooms, to add or remove sockets from rooms, etc.
//!
//! To do so, the adapter uses a pub/sub system through redis to communicate with the other servers.
//!
//! The [`Driver`] abstraction allows to use any redis client.
//! The provided default implementation uses the [`redis`] crate.
//!
//! <div class="warning">
//!     The provided driver implementation is using <code>RESP3</code> for efficiency purposes.
//!     Make sure your redis server supports it  (redis v7 and above).
//!     If not, you can implement your own driver using the <code>RESP2</code> protocol.
//! </div>
//!
//! ## Example with the default redis driver
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
//! let adapter = RedisAdapterCtr::new(&client).await?;
//! let (layer, io) = SocketIo::builder()
//!     .with_adapter::<RedisAdapter<_>>(adapter)
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
//! This method subscribes to a *request* channel specific to the namespace with
//! the format `"{prefix}-request#{namespace}"`.
//! All requests are broadcasted to this channel and will be received by all the servers.
//! If the request is not for this local server, it will be ignored. Otherwise it will be handled.
//!
//! There are 6 types of requests:
//! * Broadcast a packet to all the matching sockets.
//! * Broadcast a packet to all the matching sockets and wait for a stream of acks.
//! * Disconnect matching sockets.
//! * Get all the rooms.
//! * Add matching sockets to rooms.
//! * Remove matching sockets to rooms.
//!
//! For requests expecting a response, the adapter will send a response to a *response* channel specific to the
//! request with the format `"{prefix}-response#{namespace}#{uid}#{req_id}"`. `uid` is the unique identifier of the
//! server that sent the request and `req_id` is the unique identifier of the request.
//! For ack streams, the adapter will first send a `BroadcastAckCount` response to the server that sent the request,
//! and then send the acks as they are received (more details in [`RedisAdapter::broadcast_with_ack`] fn).
//!
//! On the other side, each time an action has to be performed on the local server, the adapter will
//! first broadcast a request to all the servers and then perform the action locally.

use std::{
    borrow::Cow,
    collections::HashSet,
    fmt,
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{self, Poll},
    time::Duration,
};

use drivers::{redis::RedisDriver, Driver, MessageStream};
use futures_core::{FusedStream, Stream};
use futures_util::{stream::TakeUntil, StreamExt};
use pin_project_lite::pin_project;
use request::{RequestIn, RequestOut, RequestTypeIn, RequestTypeOut, Response, ResponseType};
use serde::de::DeserializeOwned;
use socketioxide_core::{
    adapter::{
        AckStreamItem, BroadcastFlags, BroadcastOptions, CoreAdapter, CoreLocalAdapter, Room,
        RoomParam, SocketEmitter,
    },
    errors::{AdapterError, BroadcastError},
    packet::Packet,
    Sid,
};
use tokio::time;

/// Drivers are an abstraction over the pub/sub backend used by the adapter.
/// You can use the provided implementation or implement your own.
pub mod drivers;

mod request;

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
}

impl Default for RedisAdapterConfig {
    fn default() -> Self {
        Self {
            request_timeout: Duration::from_secs(5),
            prefix: Cow::Borrowed("socket.io"),
        }
    }
}

/// The adapter constructor. For each namespace you define, a new adapter instance is created
/// from this constructor.
#[derive(Debug)]
pub struct RedisAdapterCtr<R = RedisDriver> {
    driver: R,
    config: RedisAdapterConfig,
}

impl RedisAdapterCtr {
    /// Create a new adapter with the default [`redis`] driver and config.
    pub async fn new(client: &redis::Client) -> redis::RedisResult<Self> {
        let driver = RedisDriver::new(client).await?;
        let config = RedisAdapterConfig::default();
        Ok(Self::new_with_driver(driver, config))
    }
    /// Create a new adapter with the default [`redis`] driver and a custom config.
    pub async fn new_with_config(
        client: &redis::Client,
        config: RedisAdapterConfig,
    ) -> redis::RedisResult<RedisAdapterCtr> {
        let driver = RedisDriver::new(client).await?;
        Ok(Self::new_with_driver(driver, config))
    }
}
impl<R: Driver> RedisAdapterCtr<R> {
    /// Create a new adapter with a custom redis/valkey driver and a config.
    ///
    /// You can implement your own driver by implementing the [`Driver`] trait with any redis/valkey client.
    /// Check the [`drivers`] module for more information.
    pub fn new_with_driver(driver: R, config: RedisAdapterConfig) -> RedisAdapterCtr<R> {
        RedisAdapterCtr { driver, config }
    }
}

/// The redis adapter implementation.
/// It is generic over the [`Driver`] used to communicate with the redis server.
/// And over the [`SocketEmitter`] used to communicate with the local server. This allows to
/// avoid cyclic dependencies between the adapter, `socketioxide-core` and `socketioxide` crates.
pub struct RedisAdapter<E, R = RedisDriver> {
    /// The driver used by the adapter. This is used to communicate with the redis server.
    /// All the redis adapter instances share the same driver.
    driver: R,
    /// The configuration of the adapter.
    config: RedisAdapterConfig,
    /// A unique identifier for the adapter to identify itself in the redis server.
    uid: Sid,
    /// The local adapter, used to manage local rooms and socket stores.
    local: CoreLocalAdapter<E>,
    /// The request channel used to broadcast requests to all the servers.
    /// format: `{prefix}-request#{path}#`.
    req_chan: String,
}

pin_project! {
    /// A stream of acknowledgement messages received from the local and remote servers.
    /// It merges the local ack stream with the remote ack stream from all the servers.
    // The server_cnt is the number of servers that are expected to send a AckCount message.
    // It is decremented each time a AckCount message is received.
    //
    // The ack_cnt is the number of acks that are expected to be received. It is the sum of all the the ack counts.
    // And it is decremented each time an ack is received.
    //
    // Therefore an exhausted stream correspond to `ack_cnt == 0` and `server_cnt == 0`.
    pub struct AckStream<S> {
        #[pin]
        local: S,
        #[pin]
        remote: TakeUntil<MessageStream, time::Sleep>,
        ack_cnt: u32,

        serv_cnt: u16,
    }
}

impl<S> AckStream<S> {
    fn new(local: S, remote: MessageStream, timeout: Duration, serv_cnt: u16) -> Self {
        let remote = remote.take_until(time::sleep(timeout));
        Self {
            local,
            remote,
            ack_cnt: 0,
            serv_cnt,
        }
    }

    /// Poll the remote stream. First the count of acks is received, then the acks are received.
    /// We expect `serv_cnt` of `BroadcastAckCount` messages to be received, then we expect
    /// `ack_cnt` of `BroadcastAck` messages.
    fn poll_remote<Err: DeserializeOwned + fmt::Debug>(
        mut self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
    ) -> Poll<Option<AckStreamItem<Err>>> {
        let projection = self.as_mut().project();
        match projection.remote.poll_next(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Ready(Some(item)) => {
                let res = rmp_serde::from_slice::<Response<Err>>(&item);
                match res {
                    Ok(Response {
                        uid,
                        req_id,
                        r#type: ResponseType::BroadcastAckCount(count),
                    }) => {
                        tracing::trace!(?uid, ?req_id, "receiving broadcast ack count {count}");
                        *projection.ack_cnt += count;
                        *projection.serv_cnt -= 1;
                        self.poll_remote(cx)
                    }
                    Ok(Response {
                        uid,
                        req_id,
                        r#type: ResponseType::BroadcastAck((sid, res)),
                    }) => {
                        tracing::trace!(?uid, ?req_id, "receiving broadcast ack {sid} {:?}", res);
                        *projection.ack_cnt -= 1;
                        Poll::Ready(Some((sid, res)))
                    }
                    Ok(Response { uid, req_id, .. }) => {
                        tracing::warn!(?uid, ?req_id, "unexpected response type");
                        self.poll_remote(cx)
                    }
                    Err(e) => {
                        tracing::warn!("error decoding ack response: {e}");
                        self.poll_remote(cx)
                    }
                }
            }
        }
    }
}
impl<Err: DeserializeOwned + fmt::Debug, S: Stream<Item = AckStreamItem<Err>>> Stream
    for AckStream<S>
{
    type Item = AckStreamItem<Err>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> Poll<Option<Self::Item>> {
        match self.as_mut().project().local.poll_next(cx) {
            Poll::Pending | Poll::Ready(None) => self.poll_remote(cx),
            Poll::Ready(Some(item)) => Poll::Ready(Some(item)),
        }
    }
}

impl<Err: DeserializeOwned + fmt::Debug, S: Stream<Item = AckStreamItem<Err>> + FusedStream>
    FusedStream for AckStream<S>
{
    /// The stream is terminated if:
    /// * The local stream is terminated.
    /// * All the servers have sent the expected ack count.
    /// * We have received all the expected acks.
    fn is_terminated(&self) -> bool {
        // remote stream is terminated if the timeout is reached
        let remote_term = (self.ack_cnt == 0 && self.serv_cnt == 0) || self.remote.is_terminated();
        self.local.is_terminated() && remote_term
    }
}

impl<E: SocketEmitter, R: Driver> RedisAdapter<E, R> {
    /// Build a response channel for a request.
    ///
    /// The uid is used to identify the server that sent the request.
    /// The req_id is used to identify the request.
    fn get_res_chan(&self, uid: Sid, req_id: Sid) -> String {
        let path = self.local.path();
        let prefix = &self.config.prefix;
        format!("{}-response#{}#{}#{}#", prefix, path, uid, req_id)
    }

    /// Handle a generic request received from the request channel.
    fn recv_req(self: &Arc<Self>, item: Vec<u8>) -> Result<(), Error<R>> {
        let req: RequestIn = rmp_serde::from_slice(&item)?;
        if req.uid == self.uid {
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
        };
        Ok(())
    }

    fn recv_broadcast(&self, opts: BroadcastOptions, packet: Packet) {
        if let Err(e) = self.local.broadcast(packet, opts) {
            let ns = self.local.path();
            let uid = self.uid;
            tracing::warn!(?uid, ?ns, "remote request broadcast handler: {:?}", e);
        }
    }

    fn recv_disconnect_sockets(&self, req: RequestIn) {
        if let Err(e) = self.local.disconnect_socket(req.opts) {
            let ns = self.local.path();
            let uid = self.uid;
            tracing::warn!(
                ?uid,
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
                let uid = self.uid;
                tracing::warn!(
                    ?uid,
                    ?ns,
                    "remote request broadcast with ack handler errors: {:?}",
                    err
                );
            };
            // First send the count of expected acks to the server that sent the request.
            // This is used to keep track of the number of expected acks.
            let res = Response {
                req_id: req.req_id,
                r#type: ResponseType::BroadcastAckCount(count),
                uid: self.uid,
            };
            if let Err(err) = self.send_res(req.uid, res).await {
                on_err(err);
                return;
            }

            // Then send the acks as they are received.
            futures_util::pin_mut!(stream);
            while let Some(ack) = stream.next().await {
                let res = Response {
                    req_id: req.req_id,
                    r#type: ResponseType::BroadcastAck(ack),
                    uid: self.uid,
                };
                if let Err(err) = self.send_res(req.uid, res).await {
                    on_err(err);
                    return;
                }
            }
        });
    }

    fn recv_rooms(&self, req: RequestIn) {
        let rooms = self.local.rooms();
        let res = Response {
            req_id: req.req_id,
            r#type: ResponseType::AllRooms(rooms),
            uid: self.uid,
        };
        let fut = self.send_res(req.uid, res);
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

    async fn send_req(&self, req: RequestOut<'_>) -> Result<(), Error<R>> {
        tracing::trace!(?req, "sending request");
        let req = rmp_serde::to_vec(&req)?;
        self.driver
            .publish(&self.req_chan, req)
            .await
            .map_err(Error::from_driver)?;

        Ok(())
    }

    fn send_res(
        &self,
        uid: Sid,
        res: Response<E::AckError>,
    ) -> impl Future<Output = Result<(), Error<R>>> + Send + 'static {
        let chan = self.get_res_chan(uid, res.req_id);
        tracing::trace!(?res, "sending response to {}", &chan);
        let res = rmp_serde::to_vec(&res);
        let driver = self.driver.clone();
        async move {
            driver
                .publish(&chan, res?)
                .await
                .map_err(Error::from_driver)?;
            Ok(())
        }
    }

    /// Await for all the responses from the remote servers.
    async fn get_res(
        &self,
        uid: Sid,
        req_id: Sid,
    ) -> Result<impl Stream<Item = Result<Response<E::AckError>, rmp_serde::decode::Error>>, Error<R>>
    {
        let remote_serv_cnt = self.server_count().await? as usize - 1;
        let chan = self.get_res_chan(uid, req_id);
        let stream = self
            .subscribe(chan)
            .await?
            .take(remote_serv_cnt)
            .take_until(time::sleep(self.config.request_timeout))
            .map(|item| rmp_serde::from_slice(&item));
        Ok(stream)
    }

    /// Little wrapper to map the error type.
    #[inline]
    async fn subscribe(&self, pat: String) -> Result<MessageStream, Error<R>> {
        self.driver.subscribe(pat).await.map_err(Error::from_driver)
    }
}

impl<E: SocketEmitter, R: Driver> CoreAdapter<E> for RedisAdapter<E, R> {
    type Error = Error<R>;
    type State = RedisAdapterCtr<R>;
    type AckStream = AckStream<E::AckStream>;

    fn new(state: &Self::State, local: CoreLocalAdapter<E>) -> Self {
        let req_chan = format!("{}-request#{}#", state.config.prefix, local.path());
        Self {
            local,
            req_chan,
            uid: Sid::new(),
            driver: state.driver.clone(),
            config: state.config.clone(),
        }
    }

    async fn init(self: Arc<Self>) -> Result<(), Self::Error> {
        let mut stream = self.subscribe(self.req_chan.clone()).await?;
        tracing::trace!(?self.req_chan, "subscribing to request channel");
        tokio::spawn(async move {
            while let Some(item) = stream.next().await {
                if let Err(e) = self.recv_req(item) {
                    let ns = self.local.path();
                    let uid = self.uid;
                    tracing::warn!(?uid, ?ns, "request handler: {e}");
                }
            }
        });
        Ok(())
    }

    async fn close(&self) -> Result<(), Self::Error> {
        self.driver
            .unsubscribe(self.local.path())
            .await
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
        if !opts.has_flag(BroadcastFlags::Local) {
            let req = RequestOut::new(self.uid, RequestTypeOut::Broadcast(&packet), &opts);
            self.send_req(req).await.map_err(AdapterError::from)?;
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
        if opts.has_flag(BroadcastFlags::Local) {
            let (local, _) = self.local.broadcast_with_ack(packet, opts, timeout);
            let stream = AckStream::new(local, MessageStream::new_empty(), Duration::default(), 0);
            return Ok(stream);
        }
        let req = RequestOut::new(self.uid, RequestTypeOut::BroadcastWithAck(&packet), &opts);
        let req_id = req.req_id;
        self.send_req(req).await?;

        let remote_serv_cnt = self.server_count().await? - 1;
        let chan = self.get_res_chan(self.uid, req_id);
        let remote = self
            .driver
            .subscribe(chan)
            .await
            .map_err(Error::from_driver)?;
        let (local, _) = self.local.broadcast_with_ack(packet, opts, timeout);
        let timeout = self.config.request_timeout;
        Ok(AckStream::new(local, remote, timeout, remote_serv_cnt))
    }

    async fn disconnect_socket(&self, opts: BroadcastOptions) -> Result<(), BroadcastError> {
        if !opts.has_flag(BroadcastFlags::Local) {
            let req = RequestOut::new(self.uid, RequestTypeOut::DisconnectSockets, &opts);
            self.send_req(req).await.map_err(AdapterError::from)?;
        }
        self.local
            .disconnect_socket(opts)
            .map_err(BroadcastError::Socket)?;

        Ok(())
    }

    async fn rooms(&self) -> Result<Vec<Room>, Self::Error> {
        let opts = BroadcastOptions::default();
        let req = RequestOut::new(self.uid, RequestTypeOut::AllRooms, &opts);
        let req_id = req.req_id;
        self.send_req(req).await?;

        let local = self.local.rooms(); // TODO: return directly an hashset or hashmap key iterator.
        let local = HashSet::<Room>::from_iter(local);
        let rooms = self
            .get_res(self.uid, req_id)
            .await?
            .filter_map(|item| async move { item.ok() }) // discard serde errors
            .filter_map(|res| async move {
                // discard non-AllRooms responses
                if let ResponseType::AllRooms(rooms) = res.r#type {
                    Some(rooms)
                } else {
                    None
                }
            })
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
        if !opts.has_flag(BroadcastFlags::Local) {
            let req = RequestOut::new(self.uid, RequestTypeOut::AddSockets(&rooms), &opts);
            self.send_req(req).await?;
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
        if !opts.has_flag(BroadcastFlags::Local) {
            let req = RequestOut::new(self.uid, RequestTypeOut::DelSockets(&rooms), &opts);
            self.send_req(req).await?;
        }
        self.local.del_sockets(opts, rooms);
        Ok(())
    }

    fn get_local(&self) -> &CoreLocalAdapter<E> {
        &self.local
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::{drivers::MessageStream, AckStream, Response, ResponseType};
    use futures_core::{FusedStream, Stream};
    use futures_util::StreamExt;
    use rmp_serde::to_vec;
    use socketioxide_core::{Sid, Str, Value};

    struct EmptyStream;
    impl Stream for EmptyStream {
        type Item = (Sid, Result<Value, String>);

        fn poll_next(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Option<Self::Item>> {
            std::task::Poll::Ready(None)
        }
    }
    impl FusedStream for EmptyStream {
        fn is_terminated(&self) -> bool {
            true
        }
    }

    //TODO: test weird behaviours, packets out of orders, etc
    #[tokio::test]
    async fn ack_stream() {
        let (tx, rx) = tokio::sync::mpsc::channel(255);
        let remote = MessageStream::new(rx);
        let stream = AckStream::new(EmptyStream, remote, Duration::from_secs(10), 2);
        let uid = Sid::new();
        let req_id = Sid::new();

        // The two servers will send 2 acks each.
        let ack_cnt_res = Response::<()> {
            uid,
            req_id,
            r#type: ResponseType::BroadcastAckCount(2),
        };
        tx.try_send(to_vec(&ack_cnt_res).unwrap()).unwrap();
        tx.try_send(to_vec(&ack_cnt_res).unwrap()).unwrap();

        let ack_res = Response::<String> {
            uid,
            req_id,
            r#type: ResponseType::BroadcastAck((Sid::new(), Ok(Value::Str(Str::from(""), None)))),
        };
        for _ in 0..4 {
            tx.try_send(to_vec(&ack_res).unwrap()).unwrap();
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
        let stream = AckStream::new(EmptyStream, remote, Duration::from_millis(50), 2);
        let uid = Sid::new();
        let req_id = Sid::new();

        // There will be only one ack count and then the stream will timeout.
        let ack_cnt_res = Response::<()> {
            uid,
            req_id,
            r#type: ResponseType::BroadcastAckCount(2),
        };
        tx.try_send(to_vec(&ack_cnt_res).unwrap()).unwrap();

        futures_util::pin_mut!(stream);
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(stream.next().await.is_none());
        assert!(stream.is_terminated());
    }
}
