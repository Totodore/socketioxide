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
    nonstandard_style
)]

use std::{
    borrow::Cow,
    collections::HashMap,
    fmt, future,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::{Duration, Instant},
};

use futures_core::{future::Future, Stream};
use futures_util::StreamExt;
use serde::{de::DeserializeOwned, Serialize};
use socketioxide_core::{
    adapter::{
        BroadcastOptions, CoreAdapter, CoreLocalAdapter, DefinedAdapter, RemoteSocketData, Room,
        RoomParam, SocketEmitter, Spawnable,
    },
    errors::{AdapterError, BroadcastError},
    packet::Packet,
    Sid, Uid,
};
use stream::{AckStream, ChanStream, DropStream};
use tokio::sync::mpsc;

use drivers::{Driver, Item, ItemHeader};
use request::{RequestIn, RequestOut, RequestTypeIn, RequestTypeOut, Response, ResponseType};

pub mod drivers;

mod request;
mod stream;

#[derive(Debug, Clone)]
pub struct MongoDbAdapterConfig {
    hb_timeout: Duration,
    hb_interval: Duration,
    request_timeout: Duration,
    /// The channel size used to receive ack responses. Default is 255.
    ///
    /// If you have a lot of servers/sockets and that you may miss acknowledgement because they arrive faster
    /// than you poll them with the returned stream, you might want to increase this value.
    pub ack_response_buffer: usize,
    /// The collection name used to store socket.io data. Default is "socket.io-adapter".
    pub collection: Cow<'static, str>,
}

impl MongoDbAdapterConfig {
    pub fn new() -> Self {
        Self {
            hb_timeout: Duration::from_secs(60),
            hb_interval: Duration::from_secs(10),
            request_timeout: Duration::from_secs(5),
            ack_response_buffer: 255,
            collection: Cow::Borrowed("socket.io-adapter"),
        }
    }
    pub fn with_hb_timeout(mut self, hb_timeout: Duration) -> Self {
        self.hb_timeout = hb_timeout;
        self
    }
    pub fn with_hb_interval(mut self, hb_interval: Duration) -> Self {
        self.hb_interval = hb_interval;
        self
    }
    pub fn with_request_timeout(mut self, request_timeout: Duration) -> Self {
        self.request_timeout = request_timeout;
        self
    }
    pub fn with_ack_response_buffer(mut self, ack_response_buffer: usize) -> Self {
        self.ack_response_buffer = ack_response_buffer;
        self
    }
}

impl Default for MongoDbAdapterConfig {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct MongoDbAdapterCtr<D> {
    driver: D,
    config: MongoDbAdapterConfig,
}

#[cfg(feature = "mongodb")]
impl MongoDbAdapterCtr<drivers::mongodb::MongoDbDriver> {
    pub fn new_with_mongodb(db: mongodb::Database) -> Self {
        let config = MongoDbAdapterConfig::default();
        Self {
            driver: drivers::mongodb::MongoDbDriver::new(db, &config.collection),
            config,
        }
    }
    pub fn new_with_mongodb_config(db: mongodb::Database, config: MongoDbAdapterConfig) -> Self {
        Self {
            driver: drivers::mongodb::MongoDbDriver::new(db, &config.collection),
            config,
        }
    }
}
impl<D: Driver> MongoDbAdapterCtr<D> {
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
    // /// Packet encoding error
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
    /// A map of nodes liveness, with the last time the node was seen alive.
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
            let stream = self.driver.watch(self.uid).await?;
            tokio::spawn(self.clone().handle_ev_stream(stream));
            tokio::spawn(self.clone().heartbeat_job());

            // Send initial heartbeat when starting.
            const HB_OPTS: BroadcastOptions = BroadcastOptions::new_empty();
            self.send_req(
                RequestOut::new(self.uid, RequestTypeOut::Heartbeat, &HB_OPTS),
                None,
            )
            .await
            .map_err(|e| match e {
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

    /// Get the number of servers by getting the number of subscribers to the request channel.
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
        const PACKET_IDX: u8 = 2;
        if opts.is_local(self.uid) {
            return Ok(self.local.rooms(opts).into_iter().collect());
        }
        let req = RequestOut::new(self.uid, RequestTypeOut::AllRooms, &opts);
        let req_id = req.id;

        // First get the remote stream because mongodb might send
        // the responses before subscription is done.
        let stream = self
            .get_res::<()>(req_id, PACKET_IDX, opts.server_id)
            .await?;
        self.send_req(req, None).await?;
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
            self.send_req(req, None).await?;
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
            self.send_req(req, None).await?;
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
        const PACKET_IDX: u8 = 3;
        let req = RequestOut::new(self.uid, RequestTypeOut::FetchSockets, &opts);
        // First get the remote stream because mongodb might send
        // the responses before subscription is done.
        let remote = self
            .get_res::<RemoteSocketData>(req.id, PACKET_IDX, opts.server_id)
            .await?;

        self.send_req(req, None).await?;
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
        const OPTS: BroadcastOptions = BroadcastOptions::new_empty();

        let mut interval = tokio::time::interval(self.config.hb_interval);
        interval.tick().await; // first tick yields immediately
        loop {
            interval.tick().await;
            self.send_req(
                RequestOut::new(self.uid, RequestTypeOut::Heartbeat, &OPTS),
                None,
            )
            .await?;
        }
    }

    async fn handle_ev_stream(
        self: Arc<Self>,
        mut stream: impl Stream<Item = Result<Item, D::Error>> + Unpin,
    ) {
        while let Some(item) = stream.next().await {
            match item {
                Ok((ItemHeader::Req { target, .. }, data))
                    if target.map_or(true, |id| id == self.uid) =>
                {
                    tracing::debug!(?target, "request header");
                    if let Err(e) = self.recv_req(data).await {
                        tracing::warn!("error receiving request from driver: {e}");
                    }
                }
                Ok((ItemHeader::Req { target, .. }, _)) => {
                    tracing::debug!(
                        ?target,
                        "receiving request which is not for us, skipping..."
                    );
                }
                Ok((header @ ItemHeader::Res { request, .. }, data)) => {
                    tracing::trace!(?request, "received response");
                    let handlers = self.responses.lock().unwrap();
                    if let Some(tx) = handlers.get(&request) {
                        if let Err(e) = tx.try_send((header, data)) {
                            tracing::warn!("error sending response to handler: {e}");
                        }
                    } else {
                        tracing::warn!(?header, ?handlers, "could not find req handler");
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
            RequestTypeIn::Heartbeat => self.recv_heartbeat(req),
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
            if let Err(err) = self.send_res(req.id, res).await {
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
                if let Err(err) = self.send_res(req.id, res).await {
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
        let fut = self.send_res(req.id, res);
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
        let fut = self.send_res(req.id, res);
        let ns = self.local.path().clone();
        let uid = self.uid;
        tokio::spawn(async move {
            if let Err(err) = fut.await {
                tracing::warn!(?uid, ?ns, "remote request fetch sockets handler: {:?}", err);
            }
        });
    }
    fn recv_heartbeat(&self, req: RequestIn) {
        tracing::debug!(?req.node_id, "heartbeat received");
        let mut node_liveness = self.nodes_liveness.lock().unwrap();
        for (id, liveness) in node_liveness.iter_mut() {
            if *id == req.node_id {
                *liveness = Instant::now();
                return;
            }
        }

        node_liveness.push((req.node_id, Instant::now()));
    }

    async fn send_req(&self, req: RequestOut<'_>, target: Option<Uid>) -> Result<(), Error<D>> {
        tracing::trace!(?req, "sending request");
        let head = ItemHeader::Req {
            target,
            origin: self.uid,
        };
        let req = (head, rmp_serde::to_vec(&req)?);
        self.driver.emit(&req).await.map_err(Error::from_driver)?;
        Ok(())
    }

    fn send_res<T: Serialize + fmt::Debug>(
        &self,
        req_id: Sid,
        res: Response<T>,
    ) -> impl Future<Output = Result<(), Error<D>>> + Send + 'static {
        tracing::trace!(?res, "sending response for {req_id} req");
        let driver = self.driver.clone();
        let head = ItemHeader::Res {
            request: req_id,
            origin: self.uid,
        };
        let res = rmp_serde::to_vec(&res).map(|res| (head, res));
        async move {
            driver.emit(&res?).await.map_err(Error::from_driver)?;
            Ok(())
        }
    }

    /// Await for all the responses from the remote servers.
    async fn get_res<T: DeserializeOwned + fmt::Debug>(
        &self,
        req_id: Sid,
        response_idx: u8,
        target_uid: Option<Uid>,
    ) -> Result<impl Stream<Item = Response<T>>, Error<D>> {
        // Check for specific target node
        let remote_serv_cnt = if target_uid.is_none() {
            self.server_count().await?.saturating_sub(1) as usize
        } else {
            1
        };
        let (tx, rx) = mpsc::channel(std::cmp::max(remote_serv_cnt, 1));
        self.responses.lock().unwrap().insert(req_id, tx);
        let stream = ChanStream::new(rx)
            .filter_map(|(_header, item)| {
                let data = match rmp_serde::from_slice::<Response<T>>(&item) {
                    Ok(data) => Some(data),
                    Err(e) => {
                        tracing::warn!(header = ?_header, "error decoding response: {e}");
                        None
                    }
                };
                future::ready(data)
            })
            .filter(move |item| future::ready(item.r#type.to_u8() == response_idx))
            .take(remote_serv_cnt)
            .take_until(tokio::time::sleep(self.config.request_timeout));
        let stream = DropStream::new(stream, self.responses.clone(), req_id);
        Ok(stream)
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
