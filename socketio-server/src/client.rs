use std::collections::HashMap;
use std::sync::{Arc, Weak};

use engineio_server::socket::Socket as EIoSocket;
use engineio_server::{engine::EngineIo, layer::EngineIoHandler};
use serde::Serialize;
use tracing::debug;

use crate::{
    config::SocketIoConfig,
    errors::Error,
    ns::{EventCallback, Namespace},
    packet::{Packet, PacketData},
};

#[derive(Debug, Clone)]
enum ClientState {
    AwaitingConnect,
    Connected,
    Closed,
}

pub struct Client {
    state: ClientState,
    config: SocketIoConfig,
    ns: HashMap<String, Namespace>,
    engine: Weak<EngineIo<Self>>,
}

impl Client {
    pub fn new(
        config: SocketIoConfig,
        engine: Weak<EngineIo<Self>>,
        ns_handlers: HashMap<String, EventCallback>,
    ) -> Self {
        let client = Self {
            state: ClientState::Closed,
            config,
            engine,
            ns: ns_handlers
                .into_iter()
                .map(|(path, callback)| (path.clone(), Namespace::new(path, callback)))
                .collect(),
        };
        client
    }

    pub async fn emit<T>(&self, sid: i64, packet: Packet<T>) -> Result<(), Error>
    where
        T: Serialize,
    {
        // debug!("Emitting packet: {:?}", packet);
        let socket = self.engine.upgrade().unwrap().get_socket(sid).unwrap();
        socket.emit(packet.try_into()?).await.unwrap();
        Ok(())
    }
}

#[engineio_server::async_trait]
impl EngineIoHandler for Client {
    fn on_connect(self: Arc<Self>, socket: &EIoSocket<Self>) {
        println!("socket connect {}", socket.sid);
        // self.state = SocketState::AwaitingConnect;
    }
    fn on_disconnect(self: Arc<Self>, socket: &EIoSocket<Self>) {
        println!("socket disconnect {}", socket.sid);
    }

    async fn on_message(self: Arc<Self>, msg: String, socket: &EIoSocket<Self>) {
        debug!("Received message: {:?}", msg);
        match Packet::<serde_json::Value>::try_from(msg).unwrap() {
            Packet {
                inner: PacketData::Connect(None),
                ns,
            } => {
                self.ns.get(&ns).unwrap().connect(socket.sid, self.clone());
                self.emit(socket.sid, Packet::<()>::connect(ns, socket.sid))
                    .await
                    .unwrap();
            }
            Packet {
                inner: PacketData::Event(msg, d),
                ns,
            } => {
                if let Some(ns) = self.ns.get(&ns) {
                    ns.recv_event(socket.sid, msg, d);
                }
            }
            _ => {}
        };
    }

    async fn on_binary(self: Arc<Self>, data: Vec<u8>, socket: &EIoSocket<Self>) {
        println!("Ping pong binary message {:?}", data);
    }
}
