use std::{borrow::Cow, future::Future, pin::Pin, sync::Arc, task, time::Duration};

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
    /// All the redis adapter instances share the same driver.
    driver: Arc<R>,
    /// The configuration of the adapter.
    config: RedisAdapterConfig,
    /// A unique identifier for the adapter to identify itself in the redis server.
    uid: Sid,
    /// The local adapter, used to manage local rooms and socket stores.
    local: CoreLocalAdapter<E>,
}

#[derive(Serialize, Deserialize)]
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

#[derive(Serialize, Deserialize)]
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

pin_project! {
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
    fn get_chan(&self) -> String {
        format!("{}#{}", self.config.prefix, self.local.path())
    }

    fn get_req_chan(&self) -> String {
        format!("{}-request#{}#", self.config.prefix, self.local.path())
    }

    fn get_res_chan(&self, req_id: Sid) -> String {
        let path = self.local.path();
        let prefix = &self.config.prefix;
        if self.config.specific_response_chan {
            format!("{}-response#{}#{}#{}", prefix, path, self.uid, req_id)
        } else {
            format!("{}-response#{}#", prefix, path)
        }
    }
}

impl<E: SocketEmitter, R: Driver> RedisAdapter<E, R> {
    fn handle_broadcast_req(&self, opts: BroadcastOptions, packet: Packet) {
        //TODO: err
        self.local.broadcast(packet, opts).unwrap();
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
        let req = rmp_serde::to_vec(&req).unwrap();
        let chan = self.get_req_chan();
        self.driver.publish(chan, req).await?;
        Ok(())
    }

    fn send_res(
        &self,
        res: Response<E>,
    ) -> impl Future<Output = Result<(), R::Error>> + Send + 'static {
        let chan = self.get_res_chan(res.req_id);
        let req = rmp_serde::to_vec(&res).unwrap();
        let driver = self.driver.clone();
        async move {
            driver.publish(chan, req).await?;
            Ok(())
        }
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

        let chan = self.get_ack_chan(req_id);
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
