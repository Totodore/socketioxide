use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

use engineioxide::socket::Socket as EIoSocket;
use engineioxide::{engine::EngineIo, layer::EngineIoHandler};
use serde_json::Value;
use tracing::debug;
use tracing::error;

use crate::adapter::Adapter;
use crate::handshake::Handshake;
use crate::{
    config::SocketIoConfig,
    errors::Error,
    ns::{EventCallback, Namespace},
    packet::{Packet, PacketData},
};

#[derive(Debug)]
pub struct Client<A: Adapter> {
    pub(crate) config: SocketIoConfig,
    ns: HashMap<String, Arc<Namespace<A>>>,
    engine: Weak<EngineIo<Self>>,
}

impl<A: Adapter> Client<A> {
    pub fn new(
        config: SocketIoConfig,
        engine: Weak<EngineIo<Self>>,
        ns_handlers: HashMap<String, EventCallback<A>>,
    ) -> Self {
        Self {
            config,
            engine,
            ns: ns_handlers
                .into_iter()
                .map(|(path, callback)| (path.clone(), Namespace::new(path, callback)))
                .collect(),
        }
    }

    pub fn emit(&self, sid: i64, packet: Packet, bin: Vec<Vec<u8>>) -> Result<(), Error> {
        let socket = self
            .engine
            .upgrade()
            .ok_or(Error::EngineGone)?
            .get_socket(sid)
            .ok_or(Error::SocketGone(sid))?;
        socket.emit(packet.try_into()?)?;

        for payload in bin {
            socket.emit_binary(payload)?;
        }
        Ok(())
    }

    /// Apply an incoming binary payload to a partial binary packet waiting to be filled with all the payloads
    ///
    /// Returns true if the packet is complete and should be processed
    fn apply_payload_on_packet(&self, data: Vec<u8>, socket: &EIoSocket<Self>) -> bool {
        debug!("[sid={}] applying payload on packet", socket.sid);
        if let Some(ref mut packet) = *socket.data.partial_bin_packet.lock().unwrap() {
            match packet.inner {
                PacketData::BinaryEvent(_, ref mut bin, _)
                | PacketData::BinaryAck(ref mut bin, _) => {
                    bin.add_payload(data);
                    bin.is_complete()
                }
                _ => unreachable!("partial_bin_packet should only be set for binary packets"),
            }
        } else {
            debug!("[sid={}] socket received unexpected bin data", socket.sid);
            false
        }
    }

    /// Called when a socket connects to a new namespace
    async fn sock_connect(
        self: Arc<Self>,
        auth: Value,
        ns_path: String,
        socket: &EIoSocket<Self>,
    ) -> Result<(), Error> {
        debug!("auth: {:?}", auth);
        let handshake = Handshake::new(auth, socket.req_data.clone());
        let sid = socket.sid;
        if let Some(ns) = self.get_ns(&ns_path) {
            ns.connect(sid, self.clone(), handshake);
            self.emit(sid, Packet::connect(ns_path, sid), vec![])
        } else {
            self.emit(sid, Packet::invalid_namespace(ns_path), vec![])
        }
    }

    /// Cache-in the socket data until all the binary payloads are received
    fn sock_recv_bin_packet(self: Arc<Self>, socket: &EIoSocket<Self>, packet: Packet) {
        socket
            .data
            .partial_bin_packet
            .lock()
            .unwrap()
            .replace(packet);
    }

    /// Propagate a packet to a its target namespace
    fn sock_propagate_packet(self: Arc<Self>, packet: Packet, sid: i64) -> Result<(), Error> {
        if let Some(ns) = self.ns.get(&packet.ns) {
            ns.recv(sid, packet.inner)
        } else {
            debug!("invalid namespace requested: {}", packet.ns);
            Ok(())
        }
    }

    fn get_ns(&self, path: &str) -> Option<Arc<Namespace<A>>> {
        self.ns.get(path).cloned()
    }
}

#[derive(Default)]
pub struct SocketData {
    /// Partial binary packet that is being received
    /// Stored here until all the binary payloads are received
    pub partial_bin_packet: Mutex<Option<Packet>>,
}

#[engineioxide::async_trait]
impl<A: Adapter> EngineIoHandler for Client<A> {
    type Data = SocketData;

    fn on_connect(self: Arc<Self>, socket: &EIoSocket<Self>) {
        debug!("eio socket connect {}", socket.sid);
    }
    fn on_disconnect(self: Arc<Self>, socket: &EIoSocket<Self>) {
        debug!("eio socket disconnect {}", socket.sid);
        self.ns.values().for_each(|ns| {
            ns.disconnect(socket.sid).ok();
        });
    }

    async fn on_message(self: Arc<Self>, msg: String, socket: &EIoSocket<Self>) {
        debug!("Received message: {:?}", msg);
        let packet = match Packet::try_from(msg) {
            Ok(packet) => packet,
            Err(e) => {
                debug!("socket serialization error: {}", e);
                socket.emit_close();
                return;
            }
        };
        debug!("Packet: {:?}", packet);
        let res = match packet.inner {
            PacketData::Connect(auth) => self.sock_connect(auth, packet.ns, socket).await,
            PacketData::BinaryEvent(_, _, _) | PacketData::BinaryAck(_, _) => {
                self.sock_recv_bin_packet(socket, packet);
                Ok(())
            }
            _ => self.sock_propagate_packet(packet, socket.sid),
        };
        if let Err(err) = res {
            error!("error while processing packet: {}", err);
            socket.emit_close();
        }
    }

    /// When a binary payload is received from a socket, it is applied to the partial binary packet
    ///
    /// If the packet is complete, it is propagated to the namespace
    async fn on_binary(self: Arc<Self>, data: Vec<u8>, socket: &EIoSocket<Self>) {
        if self.apply_payload_on_packet(data, socket) {
            if let Some(packet) = socket.data.partial_bin_packet.lock().unwrap().take() {
                if let Err(e) = self.sock_propagate_packet(packet, socket.sid) {
                    debug!(
                        "error while propagating packet to socket {}: {}",
                        socket.sid, e
                    );
                    socket.emit_close();
                }
            }
        }
    }
}
