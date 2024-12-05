use std::{
    borrow::Cow,
    sync::{Arc, OnceLock},
    time::Duration,
};

use drivers::{Driver, MessageStream};
use serde::{Deserialize, Serialize};
use socketioxide_core::{
    adapter::{
        BroadcastFlags, BroadcastOptions, CoreAdapter, CoreLocalAdapter, Room, RoomParam,
        SocketEmitter,
    },
    errors::SocketError,
    packet::Packet,
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
struct RedisAdapterState<R> {
    driver: Arc<R>,
    config: RedisAdapterConfig,
}

pub struct RedisAdapter<E, R> {
    driver: Arc<R>,
    config: RedisAdapterConfig,
    rx: OnceLock<MessageStream>,
    uid: Sid,
    local: CoreLocalAdapter<E>,
}

#[derive(Serialize, Deserialize)]
struct SendOptions {
    rooms: Vec<Room>,
    except: Vec<Sid>,
    flags: u8,
}

#[derive(Serialize, Deserialize)]
enum RequestType {
    Sockets = 0,
    AllRooms = 1,
    RemoteJoin = 2,
    RemoteLeave = 3,
    RemoteDisconnect = 4,
    RemoveFetch = 5,
    ServerSideEmit = 6,
    Broadcast,
    BroadcastClientCount,
    BroadcastAck,
}
#[derive(Serialize, Deserialize)]
struct Request {
    uid: Sid,
    req_id: Sid,
    r#type: RequestType,
}

impl<E: SocketEmitter, R> RedisAdapter<E, R> {
    fn get_req_channel(&self, room: &str) -> String {
        format!(
            "{}-request#{}#{}#",
            self.config.prefix,
            self.local.path(),
            room
        )
    }
    fn get_res_channel(&self, room: &str) -> String {
        let path = self.local.path();
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

    fn new(state: &Self::State, local: CoreLocalAdapter<E>) -> Self {
        Self {
            local,
            driver: state.driver.clone(),
            config: state.config.clone(),
            rx: OnceLock::new(),
            uid: Sid::new(),
        }
    }

    async fn init(&self) -> Result<(), Self::Error> {
        let rx = self.driver.subscribe(self.local.path().clone()).await?;
        self.rx
            .set(rx)
            .expect("init fn on adapter should be called only once");
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
            let opts = SendOptions {
                rooms: opts.rooms,
                except: opts.except,
                flags: opts.flags.bits(),
            };
        }

        self.local.broadcast(packet, opts);
    }

    fn get_local(&self) -> &CoreLocalAdapter<E> {
        &self.local
    }
}
