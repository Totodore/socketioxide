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

use std::{
    borrow::Cow,
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{self, Poll},
    time::Duration,
};

use drivers::{Driver, MessageStream};
use futures_core::{FusedStream, Stream};
use futures_util::StreamExt;
use pin_project_lite::pin_project;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
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

#[derive(Serialize, Deserialize, Debug)]
enum ResponseType<AckErr> {
    BroadcastAck((Sid, Result<Value, AckErr>)),
    BroadcastAckCount(u32),
    AllRooms(Vec<Room>),
}
#[derive(Serialize, Deserialize, Debug)]
struct Request {
    uid: Sid,
    req_id: Sid,
    r#type: RequestType,
    opts: BroadcastOptions,
}

#[derive(Serialize, Deserialize, Debug)]
struct Response<AckErr> {
    uid: Sid,
    req_id: Sid,
    r#type: ResponseType<AckErr>,
}

impl<D: Driver> RedisAdapterState<D> {
    /// Create a new redis adapter state.
    pub fn new(driver: Arc<D>, config: RedisAdapterConfig) -> Self {
        Self { driver, config }
    }
}

pin_project! {
    /// A stream of acknowledgement messages received from the local and remote servers.
    /// The ack_cnt is used to keep track of the number of expected vs received acks.
    pub struct AckStream<S> {
        #[pin]
        local: S,
        #[pin]
        remote: MessageStream,
        ack_cnt: u32,
        serv_cnt: u16,
    }
}

impl<S> AckStream<S> {
    fn new(local: S, remote: MessageStream, serv_cnt: u16) -> Self {
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
    fn poll_remote<Err: DeserializeOwned>(
        self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
    ) -> Poll<Option<AckStreamItem<Err>>> {
        let projection = self.project();
        match projection.remote.poll_next(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Ready(Some(item)) => {
                let res: Response<Err> = rmp_serde::from_slice(&item).unwrap();
                match res.r#type {
                    ResponseType::BroadcastAckCount(count) => {
                        *projection.ack_cnt += count;
                        *projection.serv_cnt -= 1;
                        // We wake the task here to be sure that the stream is polled again.
                        // However it seems it is not the best thing to do:
                        // https://users.rust-lang.org/t/help-on-streams-using-wake-by-ref-before-pending-to-yield-for-other-work/85696/2
                        cx.waker().wake_by_ref();
                        Poll::Pending
                    }
                    ResponseType::BroadcastAck((sid, res)) => {
                        *projection.ack_cnt -= 1;
                        Poll::Ready(Some((sid, res)))
                    }
                    _ => Poll::Pending,
                }
            }
        }
    }
}
impl<Err: DeserializeOwned, S: Stream<Item = AckStreamItem<Err>>> Stream for AckStream<S> {
    type Item = AckStreamItem<Err>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> Poll<Option<Self::Item>> {
        match self.as_mut().project().local.poll_next(cx) {
            Poll::Pending | Poll::Ready(None) => self.poll_remote(cx),
            Poll::Ready(Some(item)) => Poll::Ready(Some(item)),
        }
    }
}

impl<Err: DeserializeOwned, S: Stream<Item = AckStreamItem<Err>> + FusedStream> FusedStream
    for AckStream<S>
{
    /// The stream is terminated if:
    /// * The local stream is terminated.
    /// * All the servers have sent the expected ack count.
    /// * We have received all the expected acks.
    fn is_terminated(&self) -> bool {
        self.local.is_terminated() && self.ack_cnt == 0 && self.serv_cnt == 0
    }
}

impl<E: SocketEmitter, R> RedisAdapter<E, R> {
    /// Build a response channel for a request.
    ///
    /// The uid is used to identify the server that sent the request.
    /// The req_id is used to identify the request.
    fn get_res_chan(&self, uid: Sid, req_id: Sid) -> String {
        let path = self.local.path();
        let prefix = &self.config.prefix;
        format!("{}-response#{}#{}#{}#", prefix, path, uid, req_id)
    }
}

impl<E: SocketEmitter, R: Driver> RedisAdapter<E, R> {
    /// Handle a generic request received from the request channel.
    fn recv_req(self: &Arc<Self>, item: Vec<u8>) {
        let req: Request = rmp_serde::from_slice(&item).unwrap();
        if req.uid == self.uid {
            return;
        }

        tracing::trace!(?req, "handling request");

        match req.r#type {
            RequestType::Broadcast(p) => self.recv_broadcast(req.opts, p),
            RequestType::BroadcastWithAck(p) => self
                .clone()
                .recv_broadcast_with_ack(req.uid, req.req_id, req.opts, p),
            RequestType::DisconnectSocket => self.recv_disconnect_sockets(req),
            RequestType::AllRooms => self.recv_fetch_rooms(req),
        };
    }

    fn recv_broadcast(&self, opts: BroadcastOptions, packet: Packet) {
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

    fn recv_disconnect_sockets(&self, req: Request) {
        self.local.disconnect_socket(req.opts).unwrap();
    }

    fn recv_broadcast_with_ack(
        self: Arc<Self>,
        uid: Sid,
        req_id: Sid,
        opts: BroadcastOptions,
        p: Packet,
    ) {
        let (stream, count) = self.local.broadcast_with_ack(p, opts, None);
        tokio::spawn(async move {
            // First send the count of expected acks to the server that sent the request.
            // This is used to keep track of the number of expected acks.
            let res = Response {
                req_id,
                r#type: ResponseType::BroadcastAckCount(count),
                uid: self.uid,
            };
            self.send_res(uid, res).await.unwrap();

            // Then send the acks as they are received.
            futures_util::pin_mut!(stream);
            while let Some(ack) = stream.next().await {
                let res = Response {
                    req_id,
                    r#type: ResponseType::BroadcastAck(ack),
                    uid: self.uid,
                };
                self.send_res(uid, res).await.unwrap();
            }
        });
    }

    fn recv_fetch_rooms(&self, req: Request) {
        let rooms = self.local.rooms();
        let res = Response {
            req_id: req.req_id,
            r#type: ResponseType::AllRooms(rooms),
            uid: self.uid,
        };
        tokio::spawn(self.send_res(req.uid, res));
    }

    async fn send_req(&self, req: Request) -> Result<(), R::Error> {
        tracing::trace!(?req, "sending request");
        let req = rmp_serde::to_vec(&req).unwrap();
        self.driver.publish(&self.req_chan, req).await?;
        Ok(())
    }

    fn send_res(
        &self,
        uid: Sid,
        res: Response<E::AckError>,
    ) -> impl Future<Output = Result<(), R::Error>> + Send + 'static {
        tracing::trace!(?res, "sending response");
        let chan = self.get_res_chan(uid, res.req_id);
        let res = rmp_serde::to_vec(&res).unwrap();
        let driver = self.driver.clone();
        async move {
            driver.publish(&chan, res).await?;
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
                self.recv_req(item);
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

    /// Broadcast a packet to all the servers to send them through their sockets.
    ///
    /// Currently, the errors are only returned for the local server.
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
            return Ok(AckStream::new(local, MessageStream::new_empty(), 0));
        }
        let req_id = Sid::new();
        let req = Request {
            r#type: RequestType::BroadcastWithAck(packet.clone()),
            uid: self.uid,
            req_id,
            opts: opts.clone(),
        };
        self.send_req(req).await.unwrap();

        let remote_serv_cnt = self.server_count().await? - 1;
        let chan = self.get_res_chan(self.uid, req_id);
        let remote = self.driver.subscribe(chan).await.unwrap();
        let (local, _) = self.local.broadcast_with_ack(packet, opts, timeout);
        Ok(AckStream::new(local, remote, remote_serv_cnt))
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

#[cfg(test)]
mod tests {
    use futures_core::{FusedStream, Stream};
    use futures_util::StreamExt;
    use rmp_serde::to_vec;
    use socketioxide_core::{Sid, Str, Value};

    use crate::{drivers::MessageStream, AckStream, Response, ResponseType};

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
        let mut stream = AckStream::new(EmptyStream, remote, 2);
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
        for _ in 0..4 {
            assert!(stream.next().await.is_some());
        }
        assert!(stream.is_terminated());
    }
}
