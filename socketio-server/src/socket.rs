use engineio_server::layer::EngineIoHandler;
use engineio_server::socket::Socket as EIoSocket;
use tracing::debug;

use crate::{packet::{Packet, PacketData}, errors::Error};

#[derive(Debug, Clone)]
enum SocketState {
    AwaitingConnect,
    Connected,
    Closed,
}
#[derive(Debug, Clone)]
pub struct Socket {
    state: SocketState,
}

impl Socket {
    pub fn new() -> Self {
        Self {
            state: SocketState::Closed,
        }
    }

    async fn emit(&self, socket: &EIoSocket<Self>, packet: Packet<()>) -> Result<(), Error> {
        debug!("Emitting packet: {:?}", packet);
        socket.emit(packet.try_into()?).await.unwrap();
        Ok(())
    }
}

#[engineio_server::async_trait]
impl EngineIoHandler for Socket {
    fn on_connect(&self, socket: &EIoSocket<Self>) {
        println!("socket connect {}", socket.sid);
        // self.state = SocketState::AwaitingConnect;
    }
    fn on_disconnect(&self, socket: &EIoSocket<Self>) {
        println!("socket disconnect {}", socket.sid);
    }

    async fn on_message(&self, msg: String, socket: &EIoSocket<Self>) {
        debug!("Received message: {:?}", msg);
        match Packet::<()>::try_from(msg).unwrap() {
            Packet {
                inner: PacketData::Connect(None),
                ns,
            } => {
                self.emit(socket, Packet::connect(ns, socket.sid)).await.unwrap();
            }
            _ => {}
        };
    }

    async fn on_binary(&self, data: Vec<u8>, socket: &EIoSocket<Self>) {
        println!("Ping pong binary message {:?}", data);
    }
}
