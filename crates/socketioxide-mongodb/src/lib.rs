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

use std::{
    collections::HashMap,
    fmt,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::Duration,
};

use drivers::Driver;
use futures_core::{future::Future, Stream};
use request::{RequestOut, RequestTypeOut, Response};
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
use stream::{AckStream, DropStream};
use tokio::sync::mpsc;
mod drivers;
mod request;
mod stream;

#[derive(Debug, Clone)]
pub struct MongoDbAdapterConfig {
    hb_timeout: Duration,
    hb_interval: Duration,
    request_timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct MongoDbAdapterCtr<D> {
    driver: D,
    config: MongoDbAdapterConfig,
}

#[derive(Debug, Clone)]
pub struct Error<E>(E);
impl<E: fmt::Display> fmt::Display for Error<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl<E: std::error::Error> std::error::Error for Error<E> {}
impl<E: std::error::Error + Send + 'static> From<Error<E>> for AdapterError {
    fn from(e: Error<E>) -> AdapterError {
        AdapterError(Box::new(e))
    }
}
impl<E> From<E> for Error<E> {
    fn from(e: E) -> Self {
        Error(e)
    }
}
pub(crate) type ResponseHandlers = HashMap<Sid, mpsc::Sender<Vec<u8>>>;

/// The mongodb adapter implementation.
/// It is generic over the [`Driver`] used to communicate with the mongodb server.
/// And over the [`SocketEmitter`] used to communicate with the local server. This allows to
/// avoid cyclic dependencies between the adapter, `socketioxide-core` and `socketioxide` crates.
pub struct CustomMongoDbAdapter<E, D> {
    /// The driver used by the adapter. This is used to communicate with the redis server.
    /// All the redis adapter instances share the same driver.
    driver: D,
    /// The configuration of the adapter.
    config: MongoDbAdapterConfig,
    /// A unique identifier for the adapter to identify itself in the redis server.
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
    type Error = Error<D::Error>;
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
            check_ns(self.local.path())?;
            let stream = self.driver.watch().await.map_err(InitError::Driver)?;
            tokio::spawn(self.clone().handle_ev_stream(stream));
            tokio::spawn(self.clone().heartbeat_job());
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
        let now = std::time::Instant::now();
        let mut nodes_liveness = self.nodes_liveness.lock().unwrap();
        nodes_liveness.retain(|(_, v)| v > &now);
        Ok(nodes_liveness.len() as u16)
    }

    /// Broadcast a packet to all the servers to send them through their sockets.
    async fn broadcast(
        &self,
        packet: Packet,
        opts: BroadcastOptions,
    ) -> Result<(), BroadcastError> {
        if !opts.is_local(self.uid) {
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
        if opts.is_local(self.uid) {
            tracing::debug!(?opts, "broadcast with ack is local");
            let (local, _) = self.local.broadcast_with_ack(packet, opts, timeout);
            let stream = AckStream::new_local(local);
            return Ok(stream);
        }
        // let req = RequestOut::new(self.uid, RequestTypeOut::BroadcastWithAck(&packet), &opts);
        // let req_id = req.id;

        // let remote_serv_cnt = self.server_count().await?.saturating_sub(1);

        // let (tx, rx) = mpsc::channel(self.config.ack_response_buffer + remote_serv_cnt as usize);
        // self.responses.lock().unwrap().insert(req_id, tx);
        // let remote = MessageStream::new(rx);
        // self.send_req(req, opts.server_id).await?;
        // let (local, _) = self.local.broadcast_with_ack(packet, opts, timeout);

        // Ok(AckStream::new(
        //     local,
        //     remote,
        //     self.config.request_timeout,
        //     remote_serv_cnt,
        //     req_id,
        //     self.responses.clone(),
        // ))
    }

    async fn disconnect_socket(&self, opts: BroadcastOptions) -> Result<(), BroadcastError> {
        if !opts.is_local(self.uid) {
            let req = RequestOut::new(self.uid, RequestTypeOut::DisconnectSockets, &opts);
            self.send_req(req).await.map_err(AdapterError::from)?;
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
        // let req = RequestOut::new(self.uid, RequestTypeOut::AllRooms, &opts);
        // let req_id = req.id;

        // // First get the remote stream because redis might send
        // // the responses before subscription is done.
        // let stream = self
        //     .get_res::<()>(req_id, PACKET_IDX, opts.server_id)
        //     .await?;
        // self.send_req(req, opts.server_id).await?;
        // let local = self.local.rooms(opts);
        // let rooms = stream
        //     .filter_map(|item| future::ready(item.into_rooms()))
        //     .fold(local, |mut acc, item| async move {
        //         acc.extend(item);
        //         acc
        //     })
        //     .await;
        // Ok(Vec::from_iter(rooms))
    }

    async fn add_sockets(
        &self,
        opts: BroadcastOptions,
        rooms: impl RoomParam,
    ) -> Result<(), Self::Error> {
        let rooms: Vec<Room> = rooms.into_room_iter().collect();
        if !opts.is_local(self.uid) {
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
        if !opts.is_local(self.uid) {
            let req = RequestOut::new(self.uid, RequestTypeOut::DelSockets(&rooms), &opts);
            self.send_req(req).await?;
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
        // const PACKET_IDX: u8 = 3;
        // let req = RequestOut::new(self.uid, RequestTypeOut::FetchSockets, &opts);
        // let req_id = req.id;
        // // First get the remote stream because redis might send
        // // the responses before subscription is done.
        // let remote = self
        //     .get_res::<RemoteSocketData>(req_id, PACKET_IDX, opts.server_id)
        //     .await?;

        // self.send_req(req, opts.server_id).await?;
        // let local = self.local.fetch_sockets(opts);
        // let sockets = remote
        //     .filter_map(|item| future::ready(item.into_fetch_sockets()))
        //     .fold(local, |mut acc, item| async move {
        //         acc.extend(item);
        //         acc
        //     })
        //     .await;
        // Ok(sockets)
    }

    fn get_local(&self) -> &CoreLocalAdapter<E> {
        &self.local
    }
}

impl<E: SocketEmitter, D: Driver> CustomMongoDbAdapter<E, D> {
    async fn heartbeat_job(self: Arc<Self>) -> Result<(), Error<D::Error>> {
        let mut interval = tokio::time::interval(self.config.hb_interval);
        const OPTS: BroadcastOptions = BroadcastOptions::new_empty();
        loop {
            interval.tick().await;
            self.send_req(RequestOut::new(self.uid, RequestTypeOut::Heartbeat, &OPTS))
                .await?;
        }
    }

    async fn handle_ev_stream(
        self: Arc<Self>,
        stream: impl Stream<Item = Result<Vec<u8>, D::Error>>,
    ) {
        use futures_util::StreamExt;
        stream
            .for_each(|item| async move {
                match item {
                    Ok(data) => {}
                    Err(err) => {}
                }
            })
            .await;
    }

    async fn send_req(&self, req: RequestOut<'_>) -> Result<(), Error<D::Error>> {
        tracing::trace!(?req, "sending request");
        let req = rmp_serde::to_vec(&req).unwrap();
        self.driver.emit(&req).await?;
        Ok(())
    }

    fn send_res<T: Serialize + fmt::Debug>(
        &self,
        req_id: Sid,
        res: Response<T>,
    ) -> impl Future<Output = Result<(), Error<D::Error>>> + Send + 'static {
        tracing::trace!(?res, "sending response for {req_id} req");
        // We send the req_id separated from the response object.
        // This allows to partially decode the response and route by the req_id
        // before fully deserializing it.
        let res = rmp_serde::to_vec(&(req_id, res));
        let driver = self.driver.clone();
        async move {
            driver.emit(&res?).await?;
            Ok(())
        }
    }

    /// Await for all the responses from the remote servers.
    async fn get_res<T: DeserializeOwned + fmt::Debug>(
        &self,
        req_id: Sid,
        response_idx: u8,
        target_uid: Option<Uid>,
    ) -> Result<impl Stream<Item = Response<T>>, Error<D::Error>> {
        // Check for specific target node
        let remote_serv_cnt = if target_uid.is_none() {
            self.server_count().await?.saturating_sub(1) as usize
        } else {
            1
        };
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
            .take_until(tokio::time::sleep(self.config.request_timeout));
        let stream = DropStream::new(stream, self.responses.clone(), req_id);
        Ok(stream)
    }
}

/// Error that can happen when initializing the adapter.
#[derive(thiserror::Error)]
pub enum InitError<D: Driver> {
    /// Driver error.
    #[error("driver error: {0}")]
    Driver(D::Error),
    /// Malformed namespace path.
    #[error("malformed namespace path, it must not contain '#'")]
    MalformedNamespace,
}
impl<D: Driver> fmt::Debug for InitError<D> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Driver(err) => fmt::Debug::fmt(err, f),
            Self::MalformedNamespace => write!(f, "Malformed namespace path"),
        }
    }
}
/// The result of the init future.
#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct InitRes<D: Driver>(futures_core::future::BoxFuture<'static, Result<(), InitError<D>>>);

impl<D: Driver> Future for InitRes<D> {
    type Output = Result<(), InitError<D>>;

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

/// Checks if the namespace path is valid
fn check_ns<D: Driver>(path: &str) -> Result<(), InitError<D>> {
    if path.is_empty() || path.contains('#') {
        Err(InitError::MalformedNamespace)
    } else {
        Ok(())
    }
}
