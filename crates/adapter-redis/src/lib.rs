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

//! A redis adapter implementation for the socketioxide crate.

use std::{borrow::Cow, fmt, future::Future, pin::Pin, sync::Arc, task, time::Duration};

use drivers::{Driver, MessageStream};
use futures_core::{FusedStream, Stream};
use futures_util::StreamExt;
use pin_project_lite::pin_project;
use serde::{Deserialize, Serialize};
use socketioxide_core::{
    adapter::{
        AckStreamItem, BroadcastFlags, BroadcastOptions, CoreAdapter, CoreLocalAdapter, Room,
        SocketEmitter,
    },
    errors::{DisconnectError, SocketError},
    packet::Packet,
    Sid, Value,
};

/// Drivers are an abstraction over the pub/sub backend used by the adapter.
/// You can use the provided implementation or implement your own.
pub mod drivers;

/// The adapter config
#[derive(Debug, Clone)]
pub struct RedisAdapterConfig {
    request_timeout: Duration,
    prefix: Cow<'static, str>,
}

impl Default for RedisAdapterConfig {
    fn default() -> Self {
        Self {
            request_timeout: Duration::from_secs(5),
            prefix: Cow::Borrowed("socket.io"),
        }
    }
}

/// The adapter state
#[derive(Clone)]
pub struct RedisAdapterState<R> {
    driver: Arc<R>,
    config: RedisAdapterConfig,
}

/// The redis adapter
pub struct RedisAdapter<E, R> {
    /// The driver used by the adapter. This is used to communicate with the redis server.
    /// All the redis adapter instances share the same driver.
    driver: Arc<R>,
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

#[derive(Serialize, Deserialize, Debug)]
enum RequestType {
    Broadcast(Packet),
    BroadcastWithAck(Packet),
    DisconnectSocket,
    AllRooms,
}

#[derive(Serialize, Deserialize)]
enum ResponseType<E: SocketEmitter> {
    BroadcastWithAck((Sid, Result<Value, E::AckError>)),
    AllRooms(Vec<Room>),
}
impl<E: SocketEmitter> fmt::Debug for ResponseType<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResponseType::BroadcastWithAck((sid, res)) => f
                .debug_tuple("BroadcastWithAck")
                .field(sid)
                .field(res)
                .finish(),
            ResponseType::AllRooms(rooms) => f.debug_tuple("AllRooms").field(rooms).finish(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct Request {
    uid: Sid,
    req_id: Sid,
    r#type: RequestType,
    opts: BroadcastOptions,
}

#[derive(Serialize, Deserialize)]
#[serde(bound = "E: SocketEmitter")]
struct Response<E: SocketEmitter> {
    uid: Sid,
    req_id: Sid,
    r#type: ResponseType<E>,
}
impl<E: SocketEmitter> fmt::Debug for Response<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Response")
            .field("uid", &self.uid)
            .field("req_id", &self.req_id)
            .field("r#type", &self.r#type)
            .finish()
    }
}

impl<D: Driver> RedisAdapterState<D> {
    /// Create a new redis adapter state.
    pub fn new(driver: Arc<D>, config: RedisAdapterConfig) -> Self {
        Self { driver, config }
    }
}

pin_project! {
    /// A stream of acknowledgement messages received from the local and remote servers.
    pub struct AckStream<S> {
        #[pin]
        local: S,
        #[pin]
        remote: MessageStream
    }
}

impl<Err, S: Stream<Item = AckStreamItem<Err>>> Stream for AckStream<S> {
    type Item = AckStreamItem<Err>;

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
    ) -> task::Poll<Option<Self::Item>> {
        self.project().local.poll_next(cx)
    }
}

impl<Err, S: Stream<Item = AckStreamItem<Err>> + FusedStream> FusedStream for AckStream<S> {
    fn is_terminated(&self) -> bool {
        self.local.is_terminated()
    }
}

impl<E: SocketEmitter, R> RedisAdapter<E, R> {
    fn get_res_chan(&self, req_id: Sid) -> String {
        let path = self.local.path();
        let prefix = &self.config.prefix;
        format!("{}-response#{}#{}#{}#", prefix, path, self.uid, req_id)
    }
}

impl<E: SocketEmitter, R: Driver> RedisAdapter<E, R> {
    /// Handle a generic request received from the request channel.
    fn handle_req(self: &Arc<Self>, item: Vec<u8>) {
        let req: Request = rmp_serde::from_slice(&item).unwrap();
        let sid = req.req_id;
        match req.r#type {
            RequestType::Broadcast(p) => self.handle_broadcast_req(req.opts, p),
            RequestType::BroadcastWithAck(p) => {
                self.clone().handle_broadcast_with_ack_req(sid, req.opts, p)
            }
            RequestType::DisconnectSocket => self.handle_disconnect_sockets_req(req),
            RequestType::AllRooms => self.handle_fetch_rooms_req(req),
        };
    }

    #[tracing::instrument(skip(self))]
    fn handle_broadcast_req(&self, opts: BroadcastOptions, packet: Packet) {
        if let Err(e) = self.local.broadcast(packet, opts) {
            let ns = self.local.path();
            let uid = self.uid;
            tracing::warn!(
                ?uid,
                ?ns,
                "remote request broadcast handler errors: {:?}",
                e
            );
        }
    }

    fn handle_disconnect_sockets_req(&self, req: Request) {
        self.local.disconnect_socket(req.opts).unwrap();
    }

    fn handle_broadcast_with_ack_req(self: Arc<Self>, id: Sid, opts: BroadcastOptions, p: Packet) {
        let stream = self.local.broadcast_with_ack(p, opts, None);
        tokio::spawn(async move {
            futures_util::pin_mut!(stream);
            while let Some(ack) = stream.next().await {
                let res = Response::<E> {
                    req_id: id,
                    r#type: ResponseType::BroadcastWithAck(ack),
                    uid: self.uid,
                };
                self.send_res(res).await.unwrap();
            }
        });
    }

    fn handle_fetch_rooms_req(&self, req: Request) {
        let rooms = self.local.rooms();
        let res = Response::<E> {
            req_id: req.req_id,
            r#type: ResponseType::AllRooms(rooms),
            uid: self.uid,
        };
        tokio::spawn(self.send_res(res));
    }

    async fn send_req(&self, req: Request) -> Result<(), R::Error> {
        tracing::trace!(?req, "sending request");
        let req = rmp_serde::to_vec(&req).unwrap();
        self.driver.publish(&self.req_chan, req).await?;
        Ok(())
    }

    fn send_res(
        &self,
        res: Response<E>,
    ) -> impl Future<Output = Result<(), R::Error>> + Send + 'static {
        tracing::trace!(?res, "sending response");
        let req = rmp_serde::to_vec(&res).unwrap();
        let driver = self.driver.clone();
        let chan = self.get_res_chan(res.req_id);
        async move {
            driver.publish(&chan, req).await?;
            Ok(())
        }
    }
}

impl<E: SocketEmitter, R: Driver> CoreAdapter<E> for RedisAdapter<E, R> {
    type Error = R::Error;
    type State = RedisAdapterState<R>;
    type AckStream = AckStream<E::AckStream>;

    fn new(state: &Self::State, local: CoreLocalAdapter<E>) -> Self {
        let req_chan = format!("{}-request#{}#", state.config.prefix, local.path());
        Self {
            local,
            driver: state.driver.clone(),
            config: state.config.clone(),
            uid: Sid::new(),
            req_chan,
        }
    }

    async fn init(self: Arc<Self>) -> Result<(), Self::Error> {
        use futures_util::stream::StreamExt;
        let mut stream = self.driver.subscribe(self.req_chan.clone()).await?;
        tracing::trace!(?self.req_chan, "subscribing to request channel");
        tokio::spawn(async move {
            while let Some(item) = stream.next().await {
                self.handle_req(item);
            }
        });
        Ok(())
    }

    async fn close(&self) -> Result<(), Self::Error> {
        self.driver.unsubscribe(&self.local.path()).await?;
        Ok(())
    }

    /// Get the number of servers by getting the number of subscribers to the request channel.
    async fn server_count(&self) -> Result<u16, Self::Error> {
        let count = self.driver.num_serv(&self.req_chan).await?;
        Ok(count)
    }

    /// Broadcast a packet to all the servers through the request channel.
    ///
    /// Currently, the errors are only returned for the local node.
    #[tracing::instrument(skip(self))]
    async fn broadcast(
        &self,
        packet: Packet,
        opts: BroadcastOptions,
    ) -> Result<(), Vec<SocketError>> {
        if !opts.has_flag(BroadcastFlags::Local) {
            let req = Request {
                r#type: RequestType::Broadcast(packet.clone()),
                uid: self.uid,
                req_id: Sid::new(),
                opts: opts.clone(),
            };
            self.send_req(req).await.unwrap();
        }

        self.local.broadcast(packet, opts)?;
        Ok(())
    }

    async fn broadcast_with_ack(
        &self,
        packet: Packet,
        opts: BroadcastOptions,
        timeout: Option<Duration>,
    ) -> Result<Self::AckStream, Self::Error> {
        if opts.has_flag(BroadcastFlags::Local) {
            let local = self.local.broadcast_with_ack(packet, opts, timeout);
            return Ok(AckStream {
                local,
                remote: MessageStream::new_empty(),
            });
        }
        let req_id = Sid::new();
        let req = Request {
            r#type: RequestType::Broadcast(packet.clone()),
            uid: self.uid,
            req_id,
            opts: opts.clone(),
        };
        self.send_req(req).await.unwrap();

        let chan = self.get_res_chan(req_id);
        let remote = self.driver.subscribe(chan).await.unwrap();
        let local = self.local.broadcast_with_ack(packet, opts, timeout);
        Ok(AckStream { local, remote })
    }

    async fn disconnect_socket(&self, opts: BroadcastOptions) -> Result<(), Vec<DisconnectError>> {
        if !opts.has_flag(BroadcastFlags::Local) {
            let req = Request {
                r#type: RequestType::DisconnectSocket,
                uid: self.uid,
                req_id: Sid::new(),
                opts: opts.clone(),
            };
            self.send_req(req).await.unwrap();
        }
        self.local.disconnect_socket(opts)?;

        Ok(())
    }

    async fn rooms(&self) -> Result<Vec<Room>, Self::Error> {
        let req = Request {
            r#type: RequestType::AllRooms,
            uid: self.uid,
            req_id: Sid::new(),
            opts: BroadcastOptions::default(),
        };
        self.send_req(req).await?;
        let local = self.local.rooms();
        Ok(local)
    }

    fn get_local(&self) -> &CoreLocalAdapter<E> {
        &self.local
    }
}
