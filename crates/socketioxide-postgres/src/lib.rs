#![warn(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]

//! # A PostgreSQL adapter implementation for the socketioxide crate.
//! The adapter is used to communicate with other nodes of the same application.
//! This allows to broadcast messages to sockets connected on other servers,
//! to get the list of rooms, to add or remove sockets from rooms, etc.
//!
//! To achieve this, the adapter uses [LISTEN/NOTIFY](https://www.postgresql.org/docs/current/sql-notify.html)
//! through PostgreSQL to communicate with other servers.
//!
//! The [`Driver`] abstraction allows the use of any PostgreSQL client.
//! One implementation is provided:
//! * [`SqlxDriver`](crate::drivers::sqlx::SqlxDriver) for the [`sqlx`] crate.
//!
//! You can also implement your own driver by implementing the [`Driver`] trait.
//!
//! <div class="warning">
//!     Socketioxide-postgres is not compatible with <code>@socketio/postgres-adapter</code>.
//!     They use completely different protocols and cannot be used together.
//!     Do not mix socket.io JS servers with socketioxide rust servers.
//! </div>
//!
//! ## How does it work?
//!
//! The [`PostgresAdapterCtr`] is a constructor for the [`SqlxAdapter`] which is an implementation of
//! the [`Adapter`](https://docs.rs/socketioxide/latest/socketioxide/adapter/trait.Adapter.html) trait.
//!
//! Then, for each namespace, an adapter is created and it takes a corresponding [`CoreLocalAdapter`].
//! The [`CoreLocalAdapter`] allows to manage the local rooms and local sockets. The default `LocalAdapter`
//! is simply a wrapper around this [`CoreLocalAdapter`].
//!
//! Once it is created the adapter is initialized with the [`CustomPostgresAdapter::init`] method.
//! It will subscribe to three PostgreSQL NOTIFY channels and emit heartbeats.
//! All messages are encoded with JSON.
//!
//! There are 7 types of requests:
//! * Broadcast a packet to all the matching sockets.
//! * Broadcast a packet to all the matching sockets and wait for a stream of acks.
//! * Disconnect matching sockets.
//! * Get all the rooms.
//! * Add matching sockets to rooms.
//! * Remove matching sockets from rooms.
//! * Fetch all the remote sockets matching the options.
//! * Heartbeat
//! * Initial heartbeat. When receiving an initial heartbeat all other servers reply a heartbeat immediately.
//!
//! For ack streams, the adapter will first send a `BroadcastAckCount` response to the server that sent the request,
//! and then send the acks as they are received (more details in [`CustomPostgresAdapter::broadcast_with_ack`] fn).
//!
//! On the other side, each time an action has to be performed on the local server, the adapter will
//! first broadcast a request to all the servers and then perform the action locally.

use drivers::Driver;
use futures_core::{Stream, stream::BoxStream};
use futures_util::{StreamExt, pin_mut};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::value::RawValue;
use socketioxide_core::{
    Sid, Uid,
    adapter::{
        BroadcastOptions, CoreAdapter, CoreLocalAdapter, DefinedAdapter, RemoteSocketData, Room,
        RoomParam, SocketEmitter, Spawnable,
        errors::{AdapterError, BroadcastError},
        remote_packet::{
            RequestIn, RequestOut, RequestTypeIn, RequestTypeOut, Response, ResponseType,
            ResponseTypeId,
        },
    },
    packet::Packet,
};
use std::{
    borrow::Cow,
    collections::HashMap,
    fmt, future,
    pin::Pin,
    sync::{Arc, Mutex, OnceLock},
    task::{Context, Poll},
    time::{Duration, Instant},
};
use tokio::{sync::mpsc, task::AbortHandle};

use crate::{
    drivers::Notification,
    stream::{AckStream, ChanStream},
};

pub mod drivers;
mod stream;

/// The configuration of the [`CustomPostgresAdapter`].
#[derive(Debug, Clone)]
pub struct PostgresAdapterConfig {
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

    /// The table name used to store socket.io attachments. Default is "socket_io_attachments".
    ///
    /// > The table name must be a sanitized string. Do not use special characters or spaces.
    pub table_name: Cow<'static, str>,

    /// The prefix used for the channels. Default is "socket.io".
    pub prefix: Cow<'static, str>,

    /// The threshold from which the payload size in bytes is considered large and should be passed through the
    /// attachment table. It should match the configured value on your PostgreSQL instance:
    /// <https://www.postgresql.org/docs/current/sql-notify.html>. By default it is 8KB (8000 bytes).
    pub payload_threshold: usize,

    /// The duration between cleanup queries on the attachment table.
    pub cleanup_interval: Duration,

    /// The maximum number of concurrent attachment fetches in-flight on the notification
    /// event pipeline. Default is 64.
    ///
    /// Incoming NOTIFY messages are processed through `.map().buffered(n).for_each()` so that
    /// [`recv_req`](CustomPostgresAdapter) is always called in wire order, while attachment DB
    /// round-trips overlap up to this bound. Raising it improves throughput under bursts of
    /// large payloads at the cost of more in-flight memory; lowering it tightens back-pressure
    /// on the LISTEN/NOTIFY pipeline. Because `buffered` preserves input order, a single slow
    /// attachment stalls every subsequent request until it resolves — so keep this comfortably
    /// above the typical burst size.
    pub ev_buffer_size: usize,
}

impl PostgresAdapterConfig {
    /// Create a new [`PostgresAdapterConfig`] with default values.
    pub fn new() -> Self {
        Self::default()
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

    /// The table name used to store socket.io attachments. Default is "socket_io_attachments".
    ///
    /// > The table name must be a sanitized string. Do not use special characters or spaces.
    pub fn with_table_name(mut self, table_name: impl Into<Cow<'static, str>>) -> Self {
        self.table_name = table_name.into();
        self
    }

    /// The prefix used for the channels. Default is "socket.io".
    pub fn with_prefix(mut self, prefix: impl Into<Cow<'static, str>>) -> Self {
        self.prefix = prefix.into();
        self
    }

    /// The threshold to the payload size in bytes. It should match the configured value on your PostgreSQL instance:
    /// <https://www.postgresql.org/docs/current/sql-notify.html>. By default it is 8KB (8000 bytes).
    pub fn with_payload_threshold(mut self, payload_threshold: usize) -> Self {
        self.payload_threshold = payload_threshold;
        self
    }

    /// The duration between cleanup queries on the attachment table. Default is 60 seconds.
    pub fn with_cleanup_interval(mut self, cleanup_interval: Duration) -> Self {
        self.cleanup_interval = cleanup_interval;
        self
    }

    /// The maximum number of concurrent attachment fetches in-flight on the notification
    /// event pipeline. Default is 64. See [`PostgresAdapterConfig::ev_buffer_size`] for tradeoffs.
    pub fn with_ev_buffer_size(mut self, ev_buffer_size: usize) -> Self {
        self.ev_buffer_size = ev_buffer_size;
        self
    }
}

impl Default for PostgresAdapterConfig {
    fn default() -> Self {
        Self {
            hb_timeout: Duration::from_secs(60),
            hb_interval: Duration::from_secs(10),
            request_timeout: Duration::from_secs(5),
            ack_response_buffer: 255,
            table_name: "socket_io_attachments".into(),
            prefix: "socket.io".into(),
            payload_threshold: 8_000,
            cleanup_interval: Duration::from_secs(60),
            ev_buffer_size: 64,
        }
    }
}

/// Represent any error that might happen when using this adapter.
#[derive(thiserror::Error)]
pub enum Error<D: Driver> {
    /// Postgres driver error
    #[error("driver error: {0}")]
    Driver(D::Error),
    /// Packet encoding/decoding error
    #[error("packet decoding error: {0}")]
    Serde(#[from] serde_json::Error),
    /// Response handler not found/full/closed for request
    #[error("response handler not found/full/closed for request: {req_id}")]
    ResponseHandlerNotFound {
        /// The request this response is for
        req_id: Sid,
    },
}

impl<R: Driver> fmt::Debug for Error<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl<R: Driver> From<Error<R>> for AdapterError {
    fn from(err: Error<R>) -> Self {
        AdapterError::from(Box::new(err) as Box<dyn std::error::Error + Send>)
    }
}

/// The adapter constructor. For each namespace you define, a new adapter instance is created
/// from this constructor.
#[derive(Debug, Clone)]
pub struct PostgresAdapterCtr<D: Driver> {
    driver: D,
    config: PostgresAdapterConfig,
}

#[cfg(feature = "sqlx")]
impl PostgresAdapterCtr<drivers::sqlx::SqlxDriver> {
    /// Create a new adapter constructor with the [`sqlx`](drivers::sqlx) driver
    /// and a default config.
    pub fn new_with_sqlx(pool: drivers::sqlx::sqlx_client::PgPool) -> Self {
        Self::new_with_sqlx_config(pool, PostgresAdapterConfig::default())
    }

    /// Create a new adapter constructor with the [`sqlx`](drivers::sqlx) driver
    /// and a custom config.
    pub fn new_with_sqlx_config(
        pool: drivers::sqlx::sqlx_client::PgPool,
        config: PostgresAdapterConfig,
    ) -> Self {
        let driver = drivers::sqlx::SqlxDriver::new(pool);
        Self { driver, config }
    }
}

#[cfg(feature = "tokio-postgres")]
impl PostgresAdapterCtr<drivers::tokio_postgres::TokioPostgresDriver> {
    /// Create a new adapter constructor with the [`tokio-postgres`](drivers::tokio_postgres) driver
    /// and a default config.
    pub async fn new_with_tokio_postgres<T>(
        pg_config: tokio_postgres::Config,
        tls: T,
    ) -> Result<Self, tokio_postgres::Error>
    where
        T: tokio_postgres::tls::MakeTlsConnect<tokio_postgres::Socket> + Send + Sync + 'static,
        <T as tokio_postgres::tls::MakeTlsConnect<tokio_postgres::Socket>>::Stream: Send,
    {
        Self::new_with_tokio_postgres_config(pg_config, tls, PostgresAdapterConfig::default()).await
    }

    /// Create a new adapter constructor with the [`sqlx`](drivers::sqlx) driver
    /// and a custom config.
    pub async fn new_with_tokio_postgres_config<T>(
        pg_config: tokio_postgres::Config,
        tls: T,
        config: PostgresAdapterConfig,
    ) -> Result<Self, tokio_postgres::Error>
    where
        T: tokio_postgres::tls::MakeTlsConnect<tokio_postgres::Socket> + Send + Sync + 'static,
        <T as tokio_postgres::tls::MakeTlsConnect<tokio_postgres::Socket>>::Stream: Send,
    {
        let driver = drivers::tokio_postgres::TokioPostgresDriver::new(pg_config, tls).await?;
        Ok(Self { driver, config })
    }
}

impl<D: Driver> PostgresAdapterCtr<D> {
    /// Create a new adapter constructor with a custom postgres driver and a config.
    ///
    /// You can implement your own driver by implementing the [`Driver`] trait with any postgres client.
    /// Check the [`drivers`] module for more information.
    pub fn new_with_driver(driver: D, config: PostgresAdapterConfig) -> Self {
        Self { driver, config }
    }
}

/// The postgres adapter with the [`sqlx`](drivers::sqlx) driver.
#[cfg(feature = "sqlx")]
pub type SqlxAdapter<E> = CustomPostgresAdapter<E, drivers::sqlx::SqlxDriver>;

/// The postgres adapter with the [`tokio_postgres`](drivers::tokio_postgres) driver.
#[cfg(feature = "tokio-postgres")]
pub type TokioPostgresAdapter<E> =
    CustomPostgresAdapter<E, drivers::tokio_postgres::TokioPostgresDriver>;

type ResponseHandlers = HashMap<Sid, mpsc::Sender<ResponsePayload>>;

/// The postgres adapter implementation.
/// It is generic over the [`Driver`] used to communicate with the postgres server.
/// And over the [`SocketEmitter`] used to communicate with the local server. This allows to
/// avoid cyclic dependencies between the adapter, `socketioxide-core` and `socketioxide` crates.
pub struct CustomPostgresAdapter<E, D: Driver> {
    /// The driver used by the adapter. This is used to communicate with the postgres server.
    /// All the postgres adapter instances share the same driver.
    driver: D,
    /// The configuration of the adapter.
    config: PostgresAdapterConfig,
    /// The local adapter, used to manage local rooms and socket stores.
    local: CoreLocalAdapter<E>,
    /// A map of nodes liveness, with the last time remote nodes were seen alive.
    nodes_liveness: Mutex<Vec<(Uid, std::time::Instant)>>,
    /// A map of response handlers used to await for responses from the remote servers.
    responses: Arc<Mutex<ResponseHandlers>>,
    /// A task that listens for events from the remote servers.
    ev_stream_task: OnceLock<AbortHandle>,
    hb_task: OnceLock<AbortHandle>,
}

impl<E, D: Driver> DefinedAdapter for CustomPostgresAdapter<E, D> {}
impl<E: SocketEmitter, D: Driver> CoreAdapter<E> for CustomPostgresAdapter<E, D> {
    type Error = Error<D>;
    type State = PostgresAdapterCtr<D>;
    type AckStream = AckStream<E::AckStream>;
    type InitRes = InitRes<D>;

    fn new(state: &Self::State, local: CoreLocalAdapter<E>) -> Self {
        Self {
            local,
            driver: state.driver.clone(),
            config: state.config.clone(),
            nodes_liveness: Mutex::new(Vec::new()),
            responses: Arc::new(Mutex::new(HashMap::new())),
            ev_stream_task: OnceLock::new(),
            hb_task: OnceLock::new(),
        }
    }

    fn init(self: Arc<Self>, on_success: impl FnOnce() + Send + 'static) -> Self::InitRes {
        let fut = async move {
            self.driver.init(&self.config.table_name).await?;

            let global_chan = self.get_global_chan();
            let node_chan = self.get_node_chan(self.local.server_id());
            let response_chan = self.get_response_chan(self.local.server_id());

            let channels = [
                global_chan.as_str(),
                node_chan.as_str(),
                response_chan.as_str(),
            ];

            let stream = self.driver.listen(&channels).await?;
            let ev_stream_task = tokio::spawn(self.clone().handle_ev_stream(stream)).abort_handle();
            assert!(
                self.ev_stream_task.set(ev_stream_task).is_ok(),
                "Adapter::init should be called only once"
            );
            let hb_task = tokio::spawn(self.clone().heartbeat_job()).abort_handle();
            assert!(
                self.hb_task.set(hb_task).is_ok(),
                "Adapter::init should be called only once"
            );

            // Send initial heartbeat when starting.
            self.emit_init_heartbeat().await.map_err(|e| match e {
                Error::Driver(e) => e,
                _ => unreachable!(),
            })?;

            on_success();
            Ok(())
        };
        InitRes(Box::pin(fut))
    }

    async fn close(&self) -> Result<(), Self::Error> {
        if let Some(hb_task) = self.hb_task.get() {
            hb_task.abort();
        }
        if let Some(ev_stream_task) = self.ev_stream_task.get() {
            ev_stream_task.abort();
        }

        self.driver.close().await.map_err(Error::Driver)
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
        let node_id = self.local.server_id();
        if !opts.is_local(node_id) {
            let req = RequestOut::new(node_id, RequestTypeOut::Broadcast(&packet), &opts);
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
        if opts.is_local(self.local.server_id()) {
            tracing::debug!(?opts, "broadcast with ack is local");
            let (local, _) = self.local.broadcast_with_ack(packet, opts, timeout);
            let stream = AckStream::new_local(local);
            return Ok(stream);
        }
        let req = RequestOut::new(
            self.local.server_id(),
            RequestTypeOut::BroadcastWithAck(&packet),
            &opts,
        );
        let req_id = req.id;

        let remote_serv_cnt = self.server_count().await?.saturating_sub(1);
        tracing::trace!(?remote_serv_cnt, "expecting acks from remote servers");

        let (tx, rx) = mpsc::channel(self.config.ack_response_buffer + remote_serv_cnt as usize);
        self.responses.lock().unwrap().insert(req_id, tx);

        self.send_req(req, None).await?;
        let (local, _) = self.local.broadcast_with_ack(packet, opts, timeout);

        // we wait for the configured ack timeout + the adapter request timeout
        let timeout = self
            .config
            .request_timeout
            .saturating_add(timeout.unwrap_or(self.local.ack_timeout()));

        let table_name = self.config.table_name.clone();
        let driver = self.driver.clone();

        // Resolve attachment payloads concurrently while preserving the order the notifications
        // arrived in the channel. `buffered` keeps wire order, which is required so that each
        // server's `BroadcastAckCount` is observed before its individual acks.
        let concurrency = std::cmp::max(self.config.ack_response_buffer, 1);
        let remote: BoxStream<'static, Box<RawValue>> = ChanStream::new(rx)
            .map(move |payload| resolve_resp_payload(payload, driver.clone(), table_name.clone()))
            .buffered(concurrency)
            .filter_map(future::ready)
            .boxed();

        Ok(AckStream::new(
            local,
            remote,
            timeout,
            remote_serv_cnt,
            req_id,
            self.responses.clone(),
        ))
    }

    async fn disconnect_socket(&self, opts: BroadcastOptions) -> Result<(), BroadcastError> {
        if !opts.is_local(self.local.server_id()) {
            let req = RequestOut::new(
                self.local.server_id(),
                RequestTypeOut::DisconnectSockets,
                &opts,
            );
            self.send_req(req, None).await.map_err(AdapterError::from)?;
        }
        self.local
            .disconnect_socket(opts)
            .map_err(BroadcastError::Socket)?;

        Ok(())
    }

    async fn rooms(&self, opts: BroadcastOptions) -> Result<Vec<Room>, Self::Error> {
        if opts.is_local(self.local.server_id()) {
            return Ok(self.local.rooms(opts).into_iter().collect());
        }
        let req = RequestOut::new(self.local.server_id(), RequestTypeOut::AllRooms, &opts);
        let req_id = req.id;

        // First get the remote stream because postgres might send
        // the responses before subscription is done.
        let stream = self
            .get_res::<()>(req_id, ResponseTypeId::AllRooms, opts.server_id)
            .await?;
        self.send_req(req, opts.server_id).await?;
        let local = self.local.rooms(opts);
        let rooms = stream
            .filter_map(|item| std::future::ready(item.into_rooms()))
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
        if !opts.is_local(self.local.server_id()) {
            let req = RequestOut::new(
                self.local.server_id(),
                RequestTypeOut::AddSockets(&rooms),
                &opts,
            );
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
        if !opts.is_local(self.local.server_id()) {
            let req = RequestOut::new(
                self.local.server_id(),
                RequestTypeOut::DelSockets(&rooms),
                &opts,
            );
            self.send_req(req, opts.server_id).await?;
        }
        self.local.del_sockets(opts, rooms);
        Ok(())
    }

    async fn fetch_sockets(
        &self,
        opts: BroadcastOptions,
    ) -> Result<Vec<RemoteSocketData>, Self::Error> {
        if opts.is_local(self.local.server_id()) {
            return Ok(self.local.fetch_sockets(opts));
        }
        let req = RequestOut::new(self.local.server_id(), RequestTypeOut::FetchSockets, &opts);
        // First get the remote stream because postgres might send
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

impl<E: SocketEmitter, D: Driver> CustomPostgresAdapter<E, D> {
    async fn heartbeat_job(self: Arc<Self>) -> Result<(), Error<D>> {
        let mut interval = tokio::time::interval(self.config.hb_interval);
        interval.tick().await; // first tick yields immediately
        loop {
            interval.tick().await;
            self.emit_heartbeat(None).await?;
        }
    }

    /// Drive the notification stream.
    ///
    /// Because `buffered` preserves input order, [`recv_req`](Self::recv_req) is always called
    /// in the order notifications were received on the NOTIFY channel — which is the same order
    /// the producing node issued them. This matters for causal sequences from a single producer,
    /// e.g. `AddSockets(room)` followed by `Broadcast(room)`: without ordering, a deferred
    /// attachment fetch for the first request could be overtaken by an inline second request,
    /// making the broadcast miss its target.
    ///
    /// The tradeoff of global ordering is head-of-line blocking: a single slow attachment
    /// fetch delays every subsequent request on this namespace, including requests from
    /// unrelated producer nodes.
    ///
    /// Response notifications (`handle_res_notif`) are handled synchronously inside the `map`
    /// closure and produce `None` so they do not participate in the buffered pipeline.
    async fn handle_ev_stream(self: Arc<Self>, stream: impl Stream<Item = D::Notification>) {
        let concurrency = std::cmp::max(self.config.ev_buffer_size, 1);
        let response_chan: Arc<str> = Arc::from(self.get_response_chan(self.local.server_id()));
        pin_mut!(stream);
        stream
            .map(|notif| {
                let this = self.clone();
                let response_chan = response_chan.clone();
                async move {
                    let result = if notif.channel() == &*response_chan {
                        this.handle_res_notif(notif).map(|_| None)
                    } else {
                        this.resolve_req_notif(notif).await
                    };

                    result
                        .inspect_err(|err| tracing::warn!(%err, "error handling notification"))
                        .ok()
                        .flatten()
                }
            })
            .buffered(concurrency)
            .filter_map(future::ready)
            .for_each(async |req| self.recv_req(req))
            .await;
    }

    /// Deserialize a response notification and forward it to the waiting ack-stream handler.
    /// Synchronous: no DB round-trip, attachment resolution happens on the consumer side
    /// (see `get_res`/`broadcast_with_ack`).
    fn handle_res_notif(&self, notif: D::Notification) -> Result<(), Error<D>> {
        match serde_json::from_str(notif.payload())? {
            p if p.is_loopback(self.local.server_id()) => {
                tracing::trace!("skipping loopback packets")
            }
            ResponsePacket {
                req_id, payload, ..
            } => {
                let tx = self
                    .responses
                    .lock()
                    .unwrap()
                    .get(&req_id)
                    .ok_or(Error::ResponseHandlerNotFound { req_id })?
                    .clone();

                tx.try_send(payload)
                    .map_err(|_| Error::ResponseHandlerNotFound { req_id })?;
            }
        };

        Ok(())
    }

    /// Parse a request notification into a [`RequestIn`]. Inline requests are decoded
    /// directly; attachment-deferred requests are fetched from the attachment table. Returns
    /// `None` for loopback packets or any recoverable error (decode, DB) — errors are logged
    /// and the packet is skipped so a single bad notification cannot derail the pipeline.
    async fn resolve_req_notif(
        self: &Arc<Self>,
        notif: D::Notification,
    ) -> Result<Option<RequestIn>, Error<D>> {
        let packet = serde_json::from_str::<RequestPacket<&RawValue>>(notif.payload())?;
        if packet.is_loopback(self.local.server_id()) {
            tracing::trace!("skipping loopback packets");
            return Ok(None);
        }
        match packet {
            RequestPacket::Request { payload, .. } => {
                Ok(Some(serde_json::from_str::<RequestIn>(payload.get())?))
            }
            RequestPacket::RequestWithAttachment { id, .. } => Ok(Some(
                resolve_attachment(&self.driver, &self.config.table_name, id).await?,
            )),
        }
    }

    fn recv_req(self: &Arc<Self>, req: RequestIn) {
        tracing::trace!(?req, "incoming request");
        match (req.r#type, req.opts) {
            (RequestTypeIn::Broadcast(p), Some(opts)) => self.recv_broadcast(opts, p),
            (RequestTypeIn::BroadcastWithAck(p), Some(opts)) => self
                .clone()
                .recv_broadcast_with_ack(req.node_id, req.id, p, opts),
            (RequestTypeIn::DisconnectSockets, Some(opts)) => self.recv_disconnect_sockets(opts),
            (RequestTypeIn::AllRooms, Some(opts)) => self.recv_rooms(req.node_id, req.id, opts),
            (RequestTypeIn::AddSockets(rooms), Some(opts)) => self.recv_add_sockets(opts, rooms),
            (RequestTypeIn::DelSockets(rooms), Some(opts)) => self.recv_del_sockets(opts, rooms),
            (RequestTypeIn::FetchSockets, Some(opts)) => {
                self.recv_fetch_sockets(req.node_id, req.id, opts)
            }
            req_type @ (RequestTypeIn::Heartbeat | RequestTypeIn::InitHeartbeat, _) => {
                self.recv_heartbeat(req_type.0, req.node_id)
            }
            _ => (),
        }
    }

    fn recv_broadcast(&self, opts: BroadcastOptions, packet: Packet) {
        tracing::trace!(?opts, "incoming broadcast");
        if let Err(e) = self.local.broadcast(packet, opts) {
            let ns = self.local.path();
            tracing::warn!(node_id = %self.local.server_id(), ?ns, "remote request broadcast handler: {:?}", e);
        }
    }

    fn recv_disconnect_sockets(&self, opts: BroadcastOptions) {
        if let Err(e) = self.local.disconnect_socket(opts) {
            let ns = self.local.path();
            tracing::warn!(
                node_id = %self.local.server_id(),
                %ns,
                "remote request disconnect sockets handler: {:?}",
                e
            );
        }
    }

    fn recv_broadcast_with_ack(
        self: Arc<Self>,
        origin: Uid,
        req_id: Sid,
        packet: Packet,
        opts: BroadcastOptions,
    ) {
        let (stream, count) = self.local.broadcast_with_ack(packet, opts, None);
        tokio::spawn(async move {
            let on_err = |err| {
                let ns = self.local.path();
                tracing::warn!(
                    node_id = %self.local.server_id(),
                    %ns,
                    "remote request broadcast with ack handler errors: {:?}",
                    err
                );
            };
            // First send the count of expected acks to the server that sent the request.
            // This is used to keep track of the number of expected acks.
            let res = Response {
                r#type: ResponseType::<()>::BroadcastAckCount(count),
                node_id: self.local.server_id(),
            };
            if let Err(err) = self.send_res(req_id, origin, res).await {
                on_err(err);
                return;
            }

            // Then send the acks as they are received.
            futures_util::pin_mut!(stream);
            while let Some(ack) = stream.next().await {
                let res = Response {
                    r#type: ResponseType::BroadcastAck(ack),
                    node_id: self.local.server_id(),
                };
                if let Err(err) = self.send_res(req_id, origin, res).await {
                    on_err(err);
                    return;
                }
            }
        });
    }

    fn recv_rooms(&self, origin: Uid, req_id: Sid, opts: BroadcastOptions) {
        let rooms = self.local.rooms(opts);
        let res = Response {
            r#type: ResponseType::<()>::AllRooms(rooms),
            node_id: self.local.server_id(),
        };
        let fut = self.send_res(req_id, origin, res);
        let ns = self.local.path().clone();
        let uid = self.local.server_id();
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
    fn recv_fetch_sockets(&self, origin: Uid, req_id: Sid, opts: BroadcastOptions) {
        let sockets = self.local.fetch_sockets(opts);
        let res = Response {
            node_id: self.local.server_id(),
            r#type: ResponseType::FetchSockets(sockets),
        };
        let fut = self.send_res(req_id, origin, res);
        let ns = self.local.path().clone();
        let uid = self.local.server_id();
        tokio::spawn(async move {
            if let Err(err) = fut.await {
                tracing::warn!(?uid, ?ns, "remote request fetch sockets handler: {:?}", err);
            }
        });
    }

    /// Receive a heartbeat from a remote node.
    /// It might be a FirstHeartbeat packet, in which case we are re-emitting a heartbeat to the remote node.
    fn recv_heartbeat(self: &Arc<Self>, req_type: RequestTypeIn, origin: Uid) {
        tracing::debug!(?req_type, "{:?} received", req_type);
        let mut node_liveness = self.nodes_liveness.lock().unwrap();
        // Even with a FirstHeartbeat packet we first consume the node liveness to
        // ensure that the node is not already in the list.
        for (id, liveness) in node_liveness.iter_mut() {
            if *id == origin {
                *liveness = Instant::now();
                return;
            }
        }

        node_liveness.push((origin, Instant::now()));

        if matches!(req_type, RequestTypeIn::InitHeartbeat) {
            tracing::debug!(
                ?origin,
                "initial heartbeat detected, saying hello to the new node"
            );

            let this = self.clone();
            tokio::spawn(async move {
                if let Err(err) = this.emit_heartbeat(Some(origin)).await {
                    tracing::warn!(
                        "could not re-emit heartbeat after new node detection: {:?}",
                        err
                    );
                }
            });
        }
    }

    /// Send a request to a specific target node or broadcast it to all nodes if no target is specified.
    ///
    /// The request body is serialized as JSON and wrapped in a [`RequestPacket`]. When the
    /// serialized body exceeds [`PostgresAdapterConfig::payload_threshold`], it is stored in the
    /// attachment table and only the row id travels over NOTIFY.
    async fn send_req(&self, req: RequestOut<'_>, target: Option<Uid>) -> Result<(), Error<D>> {
        tracing::trace!(?req, "sending request");
        let chan = match target {
            Some(target) => self.get_node_chan(target),
            None => self.get_global_chan(),
        };

        let node_id = self.local.server_id();
        let body = serde_json::to_string(&req)?;

        let payload = if body.len() > self.config.payload_threshold {
            let id = self
                .driver
                .push_attachment(&self.config.table_name, body.as_bytes())
                .await
                .map_err(Error::Driver)?;

            serde_json::to_string(&RequestPacket::<()>::RequestWithAttachment { node_id, id })?
        } else {
            let payload = RawValue::from_string(body)?;
            serde_json::to_string(&RequestPacket::Request { node_id, payload })?
        };

        self.driver
            .notify(&chan, &payload)
            .await
            .map_err(Error::Driver)?;
        Ok(())
    }

    /// Send a response to the node that sent the request.
    ///
    /// When the serialized response exceeds [`PostgresAdapterConfig::payload_threshold`], it is
    /// stored in the attachment table and only the row id travels over NOTIFY as a
    /// [`ResponsePayload::Attachment`].
    fn send_res<T: Serialize + fmt::Debug + Send + 'static>(
        &self,
        req_id: Sid,
        req_origin: Uid,
        payload: Response<T>,
    ) -> impl Future<Output = Result<(), Error<D>>> + 'static {
        tracing::trace!(
            ?payload,
            "sending response for {req_id} req to {req_origin}"
        );
        let driver = self.driver.clone();
        let chan = self.get_response_chan(req_origin);
        let table = self.config.table_name.clone();
        let threshold = self.config.payload_threshold;
        let node_id = self.local.server_id();

        async move {
            let body = serde_json::to_string(&payload)?;
            let payload = if body.len() > threshold {
                let id = driver
                    .push_attachment(&table, body.as_bytes())
                    .await
                    .map_err(Error::Driver)?;
                ResponsePayload::Attachment(id)
            } else {
                ResponsePayload::Data(RawValue::from_string(body)?)
            };

            let message = serde_json::to_string(&ResponsePacket {
                req_id,
                node_id,
                payload,
            })?;

            driver
                .notify(&chan, &message)
                .await
                .map_err(Error::Driver)?;
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
        let stream = ChanStream::new(rx);

        // Overlap attachment fetches across servers while preserving arrival order so that
        // `take(remote_serv_cnt)` still closes the stream after exactly one response per server.
        let concurrency = std::cmp::max(remote_serv_cnt, 1);
        let stream = stream
            .map(async move |payload| match payload {
                ResponsePayload::Data(data) => serde_json::from_str::<Response<T>>(data.get())
                    .inspect_err(|err| tracing::warn!("error decoding response: {err}"))
                    .ok(),
                ResponsePayload::Attachment(id) => {
                    resolve_attachment(&self.driver, &self.config.table_name, id)
                        .await
                        .inspect_err(|err| tracing::warn!("error fetching attachment: {err}"))
                        .ok()
                }
            })
            .buffered(concurrency)
            .filter_map(future::ready)
            .filter(move |item| future::ready(ResponseTypeId::from(&item.r#type) == response_type))
            .take(remote_serv_cnt)
            .take_until(tokio::time::sleep(self.config.request_timeout));

        Ok(stream)
    }

    /// Emit a heartbeat to the specified target node or broadcast to all nodes.
    async fn emit_heartbeat(&self, target: Option<Uid>) -> Result<(), Error<D>> {
        self.send_req(
            RequestOut::new_empty(self.local.server_id(), RequestTypeOut::Heartbeat),
            target,
        )
        .await
    }

    /// Emit an initial heartbeat to all nodes.
    async fn emit_init_heartbeat(&self) -> Result<(), Error<D>> {
        // Send initial heartbeat when starting.
        self.send_req(
            RequestOut::new_empty(self.local.server_id(), RequestTypeOut::InitHeartbeat),
            None,
        )
        .await
    }

    // == All channels are hashed to avoid thresspassing the 63 bytes limit on postgres channel ==
    // We cannot constraint the length of the channel name because it is generated dynamically.

    fn get_global_chan(&self) -> String {
        let chan = format!("{}#{}", self.config.prefix, self.local.path());
        hash_chan(&chan)
    }
    fn get_node_chan(&self, uid: Uid) -> String {
        let chan = format!("{}{}{}", self.config.prefix, self.local.path(), uid);
        hash_chan(&chan)
    }
    fn get_response_chan(&self, uid: Uid) -> String {
        let chan = format!(
            "response-{}{}{}",
            &self.config.prefix,
            self.local.path(),
            uid
        );
        hash_chan(&chan)
    }
}

fn hash_chan(chan: &str) -> String {
    let hash = xxhash_rust::xxh3::xxh3_64(chan.as_bytes());
    format!("ch_{:x}", hash)
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

async fn resolve_resp_payload<D: Driver>(
    payload: ResponsePayload,
    driver: D,
    table: Cow<'static, str>,
) -> Option<Box<RawValue>> {
    match payload {
        ResponsePayload::Data(data) => Some(data),
        ResponsePayload::Attachment(id) => resolve_attachment(&driver, &table, id)
            .await
            .inspect_err(|err| tracing::warn!(%err, id, "failed to resolve payload attachment"))
            .ok(),
    }
}

async fn resolve_attachment<T: DeserializeOwned, D: Driver>(
    driver: &D,
    table_name: &str,
    id: i64,
) -> Result<T, Error<D>> {
    let bytes = driver
        .get_attachment(table_name, id)
        .await
        .map_err(Error::Driver)?;
    Ok(serde_json::from_slice(&bytes)?)
}

/// Wire-level wrapper for request NOTIFY payloads.
///
/// A request may either be inline (serialized request JSON) or deferred to the
/// attachment table. The `node_id` in the deferred variant lets the receiver
/// filter out loopback notifications before hitting the database.
#[derive(Debug, Serialize, Deserialize)]
enum RequestPacket<T> {
    Request { node_id: Uid, payload: T },
    RequestWithAttachment { node_id: Uid, id: i64 },
}
impl<T> RequestPacket<T> {
    fn is_loopback(&self, node_id: Uid) -> bool {
        match self {
            RequestPacket::Request { node_id: id, .. } => *id == node_id,
            RequestPacket::RequestWithAttachment { node_id: id, .. } => *id == node_id,
        }
    }
}

#[derive(Deserialize, Serialize)]
struct ResponsePacket {
    req_id: Sid,
    node_id: Uid,
    payload: ResponsePayload,
}
impl ResponsePacket {
    fn is_loopback(&self, node_id: Uid) -> bool {
        self.node_id == node_id
    }
}
#[derive(Debug, Deserialize, Serialize)]
pub(crate) enum ResponsePayload {
    Data(Box<RawValue>),
    Attachment(i64),
}
