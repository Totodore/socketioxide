use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    future::Future,
    sync::{Arc, OnceLock, RwLock},
    time::Duration,
};

use drivers::{Driver, MessageStream};
use socketioxide_core::{
    adapter::{BroadcastOptions, CoreAdapter, Room, RoomParam, SocketEmitter},
    errors::{DisconnectError, SocketError},
    packet::Packet,
    Sid,
};

mod drivers;

#[derive(Debug, Clone)]
struct RedisAdapterConfig {
    request_timeout: Duration,
    specific_response_chan: bool,
    prefix: Cow<'static, str>,
}
#[derive(Clone)]
struct RedisAdapterState<R> {
    driver: Arc<R>,
    config: RedisAdapterConfig,
}

struct RedisAdapter<E, R> {
    sockets: E,
    driver: Arc<R>,
    config: RedisAdapterConfig,
    rooms: RwLock<HashMap<Room, HashSet<Sid>>>,
    rx: OnceLock<MessageStream>,
    uid: Sid,
}

impl<E: SocketEmitter, R> RedisAdapter<E, R> {
    fn get_req_channel(&self, room: &str) -> String {
        format!(
            "{}-request#{}#{}#",
            self.config.prefix,
            self.sockets.path(),
            room
        )
    }
    fn get_res_channel(&self, room: &str) -> String {
        let path = self.sockets.path();
        let prefix = &self.config.prefix;
        if self.config.specific_response_chan {
            format!("{}-response#{}#{}#{}#", prefix, path, room, self.uid)
        } else {
            format!("{}-response#{}#{}#", prefix, path, room)
        }
    }
}

impl<E: SocketEmitter, R: Driver> CoreAdapter<E> for RedisAdapter<E, R> {
    type Error = R::Error;
    type State = RedisAdapterState<R>;

    fn new(state: &Self::State, sockets: E) -> Self {
        Self {
            sockets,
            driver: state.driver.clone(),
            config: state.config.clone(),
            rooms: RwLock::new(HashMap::new()),
            rx: OnceLock::new(),
            uid: Sid::new(),
        }
    }

    async fn init(&self) -> Result<(), Self::Error> {
        let rx = self.driver.subscribe(self.sockets.path()).await?;
        self.rx
            .set(rx)
            .expect("init fn on adapter should be called only once");
        Ok(())
    }

    async fn close(&self) -> Result<(), Self::Error> {
        self.driver.unsubscribe(&self.sockets.path()).await?;
        Ok(())
    }

    async fn server_count(&self) -> Result<u16, Self::Error> {
        let count = self.driver.num_serv(&self.sockets.path()).await?;
        Ok(count)
    }

    async fn add_all(
        &self,
        sid: socketioxide_core::Sid,
        rooms: impl RoomParam,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    fn del(
        &self,
        sid: socketioxide_core::Sid,
        rooms: impl RoomParam,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        todo!()
    }

    fn del_all(
        &self,
        sid: socketioxide_core::Sid,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        todo!()
    }

    fn broadcast(
        &self,
        packet: Packet,
        opts: BroadcastOptions,
    ) -> impl Future<Output = Result<(), Vec<SocketError>>> + Send {
        todo!()
    }

    fn broadcast_with_ack(
        &self,
        packet: Packet,
        opts: BroadcastOptions,
        timeout: Option<std::time::Duration>,
    ) -> impl Future<Output = Result<E::AckStream, Self::Error>> + Send {
        todo!()
    }

    fn sockets(
        &self,
        opts: BroadcastOptions,
    ) -> impl Future<Output = Result<Vec<Sid>, Self::Error>> + Send {
        todo!()
    }

    fn socket_rooms(
        &self,
        sid: socketioxide_core::Sid,
    ) -> impl Future<Output = Result<Vec<Room>, Self::Error>> + Send {
        todo!()
    }

    fn add_sockets(
        &self,
        opts: BroadcastOptions,
        rooms: impl RoomParam,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        todo!()
    }

    fn del_sockets(
        &self,
        opts: BroadcastOptions,
        rooms: impl RoomParam,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        todo!()
    }

    fn disconnect_socket(
        &self,
        opts: BroadcastOptions,
    ) -> impl Future<Output = Result<(), Vec<DisconnectError>>> + Send {
        todo!()
    }

    fn rooms(&self) -> impl Future<Output = Result<Vec<Room>, Self::Error>> + Send {
        todo!()
    }
}
