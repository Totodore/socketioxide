use std::{
    borrow::Cow,
    fmt,
    pin::Pin,
    sync::{Arc, OnceLock},
    task,
    time::Duration,
};

use bytes::Bytes;
use drivers::{Driver, MessageStream};
use futures_core::{FusedStream, Stream};
use futures_util::StreamExt;
use pin_project_lite::pin_project;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use socketioxide_core::{
    adapter::{
        AckStreamItem, BroadcastFlags, BroadcastOptions, CoreAdapter, CoreLocalAdapter, Room,
        RoomParam, SocketEmitter,
    },
    errors::SocketError,
    packet::Packet,
    parser::Parse,
    Sid,
};

pub mod drivers;

#[derive(Debug, Clone)]
pub struct RedisAdapterConfig {
    request_timeout: Duration,
    specific_response_chan: bool,
    prefix: Cow<'static, str>,
}
#[derive(Clone)]
pub struct RedisAdapterState<R> {
    driver: Arc<R>,
    config: RedisAdapterConfig,
}

pub struct RedisAdapter<E, R> {
    /// The driver used by the adapter. This is used to communicate with the redis server.
    /// All the adapters share the same driver.
    driver: Arc<R>,
    config: RedisAdapterConfig,
    /// A unique identifier for the adapter to identify itself in the redis server.
    uid: Sid,
    /// The local adapter, used to manage local rooms and socket stores.
    local: CoreLocalAdapter<E>,
}

#[derive(Serialize, Deserialize)]
enum RequestType {
    Broadcast,
    BroadcastWithAck,
}
#[derive(Serialize, Deserialize)]
struct Request {
    uid: Sid,
    req_id: Sid,
    r#type: RequestType,
    opts: BroadcastOptions,
    packet: Packet,
}
#[derive(Debug)]
struct BoxedError(Box<dyn std::error::Error + Send + Sync>);
impl fmt::Display for BoxedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for BoxedError {}
impl Serialize for BoxedError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_string().serialize(serializer)
    }
}

pin_project! {
    pub struct AckStream<S> {
        #[pin]
        local: S
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
    fn get_req_chan(&self) -> String {
        format!("{}-request#{}", self.config.prefix, self.local.path())
    }
    fn get_ack_chan(&self, req_id: Sid) -> String {
        let path = self.local.path();
        let prefix = &self.config.prefix;
        format!("{}-ack#{}#{}#{}", prefix, path, self.uid, req_id)
    }
}
impl<E: SocketEmitter, R: Driver> RedisAdapter<E, R> {
    fn handle_broadcast_req(&self, req: Request) {
        //TODO: err
        self.local.broadcast(req.packet, req.opts).unwrap();
    }
    fn handle_broadcast_with_ack_req(self: Arc<Self>, req: Request) {
        let stream = self.local.broadcast_with_ack(req.packet, req.opts, None);
        tokio::spawn(async move {
            futures_util::pin_mut!(stream);
            while let Some(ack) = stream.next().await {
                let ack = rmp_serde::to_vec(&ack).unwrap();
                let chan = self.get_ack_chan(req.req_id);
                self.driver.publish(chan, ack).await.unwrap();
            }
        });
    }
}

impl<E: SocketEmitter, R: Driver> CoreAdapter<E> for RedisAdapter<E, R> {
    type Error = R::Error;
    type State = RedisAdapterState<R>;
    type AckStream = AckStream<E::AckStream>;

    fn new(state: &Self::State, local: CoreLocalAdapter<E>) -> Self {
        Self {
            local,
            driver: state.driver.clone(),
            config: state.config.clone(),
            uid: Sid::new(),
        }
    }

    async fn init(self: Arc<Self>) -> Result<(), Self::Error> {
        use futures_util::stream::StreamExt;
        let chan = format!("{}-request#{}", self.config.prefix, self.local.path());
        let mut stream = self.driver.subscribe(chan).await?;
        tokio::spawn(async move {
            while let Some(item) = stream.next().await {
                let req: Request = rmp_serde::from_slice(&item).unwrap();
                match req.r#type {
                    RequestType::Broadcast => self.handle_broadcast_req(req),
                    RequestType::BroadcastWithAck => {
                        self.clone().handle_broadcast_with_ack_req(req)
                    }
                };
            }
        });
        Ok(())
    }

    async fn close(&self) -> Result<(), Self::Error> {
        self.driver.unsubscribe(&self.local.path()).await?;
        Ok(())
    }

    async fn server_count(&self) -> Result<u16, Self::Error> {
        let count = self.driver.num_serv(&self.local.path()).await?;
        Ok(count)
    }

    async fn broadcast(
        &self,
        packet: Packet,
        opts: BroadcastOptions,
    ) -> Result<(), Vec<SocketError>> {
        if !opts.has_flag(BroadcastFlags::Local) {
            let req = Request {
                r#type: RequestType::Broadcast,
                uid: self.uid,
                req_id: Sid::new(),
                opts: opts.clone(),
                packet: packet.clone(),
            };
            let req = rmp_serde::to_vec(&req).unwrap();
            let chan = self.get_req_chan();
            self.driver.publish(chan, req).await.unwrap();
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
            return Ok(AckStream { local });
        }
        let req_id = Sid::new();
        let req = Request {
            r#type: RequestType::Broadcast,
            uid: self.uid,
            req_id,
            opts: opts.clone(),
            packet: packet.clone(),
        };
        let req = rmp_serde::to_vec(&req).unwrap();

        let chan = self.get_req_chan();
        self.driver.publish(chan, req).await.unwrap();

        let chan = self.get_ack_chan(req_id);
        let stream = self.driver.subscribe(chan).await.unwrap();
        //TODO: combine streams
        let local = self.local.broadcast_with_ack(packet, opts, timeout);
        Ok(AckStream { local })
    }

    fn get_local(&self) -> &CoreLocalAdapter<E> {
        &self.local
    }
}
