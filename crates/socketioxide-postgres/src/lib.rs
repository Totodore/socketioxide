#![cfg_attr(docsrs, feature(doc_auto_cfg))]
#![warn(
    clippy::all,
    clippy::todo,
    clippy::empty_enums,
    clippy::mem_forget,
    clippy::unused_self,
    clippy::filter_map_next,
    clippy::needless_continue,
    clippy::needless_borrow,
    clippy::match_wildcard_for_single_variants,
    clippy::if_let_mutex,
    clippy::await_holding_lock,
    clippy::indexing_slicing,
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
//!

use drivers::Driver;
use futures_core::Stream;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
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
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::{Duration, Instant},
};

use crate::{drivers::Notification, stream::AckStream};

mod drivers;
mod stream;

/// The configuration of the [`MongoDbAdapter`].
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
    pub table_name: Cow<'static, str>,
    /// The prefix used for the channels. Default is "socket.io".
    pub prefix: Cow<'static, str>,
    /// The treshold to the payload size in bytes. It should match the configured value on your PostgreSQL instance:
    /// <https://www.postgresql.org/docs/current/sql-notify.html>
    pub payload_treshold: usize,
    /// The duration between cleanup queries on the
    pub cleanup_intervals: Duration,
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

/// An event we should answer to
#[derive(Debug, Deserialize)]
struct Event {}

pub struct PostgresAdapterCtr<D: Driver> {
    driver: D,
    config: PostgresAdapterConfig,
}

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
    /// A unique identifier for the adapter to identify itself in the postgres server.
    uid: Uid,
    /// The local adapter, used to manage local rooms and socket stores.
    local: CoreLocalAdapter<E>,
    /// A map of nodes liveness, with the last time remote nodes were seen alive.
    nodes_liveness: Mutex<Vec<(Uid, std::time::Instant)>>,
    /// A map of response handlers used to await for responses from the remote servers.
    responses: Arc<Mutex<HashMap<Sid, D::NotifStream>>>,
}

impl<E, D: Driver> DefinedAdapter for CustomPostgresAdapter<E, D> {}
impl<E: SocketEmitter, D: Driver> CoreAdapter<E> for CustomPostgresAdapter<E, D> {
    type Error = Error<D>;
    type State = PostgresAdapterCtr<D>;
    type AckStream = AckStream<E::AckStream, D::Notification>;
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
            let stream = self.driver.listen("event").await?;
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
        let remote = self.driver.listen("").await?;

        self.send_req(req, None).await?;
        let (local, _) = self.local.broadcast_with_ack(packet, opts, timeout);

        Ok(AckStream::new(
            local,
            remote,
            self.config.request_timeout,
            remote_serv_cnt,
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

impl<E: SocketEmitter, D: Driver> CustomPostgresAdapter<E, D> {
    async fn heartbeat_job(self: Arc<Self>) -> Result<(), Error<D>> {
        let mut interval = tokio::time::interval(self.config.hb_interval);
        interval.tick().await; // first tick yields immediately
        loop {
            interval.tick().await;
            self.emit_heartbeat(None).await?;
        }
    }

    async fn handle_ev_stream(self: Arc<Self>, stream: impl Stream<Item = RequestIn>) {
        futures_util::pin_mut!(stream);
        while let Some(req) = stream.next().await {
            self.recv_req(req);
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
            tracing::warn!(?self.uid, ?ns, "remote request broadcast handler: {:?}", e);
        }
    }

    fn recv_disconnect_sockets(&self, opts: BroadcastOptions) {
        if let Err(e) = self.local.disconnect_socket(opts) {
            let ns = self.local.path();
            tracing::warn!(
                ?self.uid,
                ?ns,
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
            if let Err(err) = self.send_res(req_id, origin, res).await {
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
            node_id: self.uid,
        };
        let fut = self.send_res(req_id, origin, res);
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
    fn recv_fetch_sockets(&self, origin: Uid, req_id: Sid, opts: BroadcastOptions) {
        let sockets = self.local.fetch_sockets(opts);
        let res = Response {
            node_id: self.uid,
            r#type: ResponseType::FetchSockets(sockets),
        };
        let fut = self.send_res(req_id, origin, res);
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
    async fn send_req(&self, req: RequestOut<'_>, target: Option<Uid>) -> Result<(), Error<D>> {
        tracing::trace!(?req, "sending request");
        // let head = ItemHeader::Req { target };
        // let req = self.new_packet(head, &req)?;
        self.driver
            .notify("yolo", &req)
            .await
            .map_err(Error::Driver)?;
        Ok(())
    }

    /// Send a response to the node that sent the request.
    fn send_res<T: Serialize + fmt::Debug + 'static>(
        &self,
        req_id: Sid,
        req_origin: Uid,
        res: Response<T>,
    ) -> impl Future<Output = Result<(), Error<D>>> + 'static {
        tracing::trace!(?res, "sending response for {req_id} req to {req_origin}");
        let driver = self.driver.clone();
        //TODO: is this the right way?
        async move {
            driver
                .notify("response", &res)
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

        let stream = self.driver.listen("test").await.unwrap();
        self.responses.lock().unwrap().insert(req_id, stream);

        let stream = stream
            .filter_map(|notif| {
                let data = match serde_json::from_str::<Response<T>>(notif.payload()) {
                    Ok(data) => Some(data),
                    Err(e) => {
                        tracing::warn!(channel = %notif.channel(), "error decoding response: {e}");
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
        self.send_req(
            RequestOut::new_empty(self.uid, RequestTypeOut::Heartbeat),
            target,
        )
        .await
    }

    /// Emit an initial heartbeat to all nodes.
    async fn emit_init_heartbeat(&self) -> Result<(), Error<D>> {
        // Send initial heartbeat when starting.
        self.send_req(
            RequestOut::new_empty(self.uid, RequestTypeOut::InitHeartbeat),
            None,
        )
        .await
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
