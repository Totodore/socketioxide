use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

use engineio_server::socket::Socket as EIoSocket;
use engineio_server::{engine::EngineIo, layer::EngineIoHandler};
use serde::Serialize;
use serde_json::{json, Value};
use tracing::debug;
use tracing::error;

use crate::handshake::Handshake;
use crate::{
    config::SocketIoConfig,
    errors::Error,
    ns::{EventCallback, Namespace},
    packet::{Packet, PacketData},
};

pub struct Client {
    pub(crate) config: SocketIoConfig,
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
            config,
            engine,
            ns: ns_handlers
                .into_iter()
                .map(|(path, callback)| (path.clone(), Namespace::new(path, callback)))
                .collect(),
        };
        client
    }

    pub async fn emit(&self, sid: i64, packet: Packet<impl Serialize>) -> Result<(), Error> {
        // debug!("Emitting packet: {:?}", packet);
        let socket = self.engine.upgrade().unwrap().get_socket(sid).unwrap();
        socket.emit(packet.try_into()?).await.unwrap();
        Ok(())
    }

    /// Apply an incoming binary payload to a partial binary packet waiting to be filled with all the payloads
    /// Returns true if the packet is complete and should be processed
    fn apply_payload_on_packet(&self, data: Vec<u8>, socket: &EIoSocket<Self>) -> bool {
        debug!("[sid={}] applying payload on packet", socket.sid);
        if let Some(ref mut packet) = *socket.data.partial_bin_packet.lock().unwrap() {
            match packet.inner {
                PacketData::BinaryEvent(_, ref mut bin, _)
                | PacketData::BinaryAck(ref mut bin, _) => {
                    bin.add_payload(data);
                    return bin.is_complete();
                }
                _ => unreachable!("partial_bin_packet should only be set for binary packets"),
            }
        } else {
            debug!("[sid={}] socket received unexpected bin data", socket.sid);
            return false;
        };
    }
}

#[derive(Default)]
pub struct SocketData {
    /// Partial binary packet that is being received
    /// Stored here until all the binary payloads are received
    pub partial_bin_packet: Mutex<Option<Packet<Value>>>,
}

#[engineio_server::async_trait]
impl EngineIoHandler for Client {
    type Data = SocketData;

    fn on_connect(self: Arc<Self>, socket: &EIoSocket<Self>) {
        println!("socket connect {}", socket.sid);
        // self.state = SocketState::AwaitingConnect;
    }
    fn on_disconnect(self: Arc<Self>, socket: &EIoSocket<Self>) {
        println!("socket disconnect {}", socket.sid);
    }

    async fn on_message(self: Arc<Self>, msg: String, socket: &EIoSocket<Self>) {
        debug!("Received message: {:?}", msg);
        let packet = Packet::<Value>::try_from(msg);
        debug!("Packet: {:?}", packet);
        match packet {
            Ok(Packet {
                inner: PacketData::Connect(auth),
                ns: ns_path,
            }) => {
                debug!("auth: {:?}", auth);
                let handshake = Handshake {
                    url: "".to_string(),
                    issued: 0,
                    auth: auth.unwrap_or(json!({})),
                };
                if let Some(ns) = self.ns.get(&ns_path) {
                    ns.connect(socket.sid, self.clone(), handshake);
                    self.emit(socket.sid, Packet::connect(ns_path, socket.sid))
                        .await
                        .unwrap();
                } else {
                    self.emit(socket.sid, Packet::<()>::invalid_namespace(ns_path))
                        .await
                        .unwrap();
                }
            }
            Ok(Packet {
                inner: PacketData::BinaryEvent(e, p, a),
                ns,
            }) => {
                socket
                    .data
                    .partial_bin_packet
                    .lock()
                    .unwrap()
                    .replace(Packet { inner: PacketData::BinaryEvent(e, p, a), ns });
            },
            Ok(Packet {
                inner: PacketData::BinaryAck(p, a),
                ns,
            }) => {
                socket
                    .data
                    .partial_bin_packet
                    .lock()
                    .unwrap()
                    .replace(Packet { inner: PacketData::BinaryAck(p, a), ns });
            }
            Ok(Packet {
                inner: packet_data,
                ns,
            }) => {
                if let Some(ns) = self.ns.get(&ns) {
                    if let Err(e) = ns.recv(socket.sid, packet_data) {
                        error!("[sid={}] {e}", socket.sid);
                    }
                }
            }
            Err(e) => {
                debug!("socket serialization error: {}", e);
                socket.emit_close().await;
            }
        };
    }

    async fn on_binary(self: Arc<Self>, data: Vec<u8>, socket: &EIoSocket<Self>) {
        if self.apply_payload_on_packet(data, socket) {
            if let Some(packet) = socket.data.partial_bin_packet.lock().unwrap().take() {
                if let Some(ns) = self.ns.get(&packet.ns) {
                    if let Err(e) = ns.recv(socket.sid, packet.inner) {
                        error!("[sid={}] {e}", socket.sid);
                    }
                }
            }
        }
    }
}
