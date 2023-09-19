use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use engineioxide::handler::EngineIoHandler;
use engineioxide::socket::{DisconnectReason as EIoDisconnectReason, Socket as EIoSocket};
use serde_json::Value;

use engineioxide::sid_generator::Sid;
use tokio::sync::oneshot;
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
    pub(crate) config: Arc<SocketIoConfig>,
    ns: HashMap<String, Arc<Namespace<A>>>,
}

impl<A: Adapter> Client<A> {
    pub fn new(config: SocketIoConfig, ns_handlers: HashMap<String, EventCallback<A>>) -> Self {
        Self {
            config: config.into(),
            ns: ns_handlers
                .into_iter()
                .map(|(path, callback)| (path.clone(), Namespace::new(path, callback)))
                .collect(),
        }
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
    fn sock_connect(
        &self,
        auth: Value,
        ns_path: String,
        esocket: &EIoSocket<Self>,
    ) -> Result<(), serde_json::Error> {
        debug!("auth: {:?}", auth);
        let handshake = Handshake::new(auth, esocket.req_data.clone());
        let sid = esocket.sid;
        if let Some(ns) = self.get_ns(&ns_path) {
            let socket = ns.connect(sid, esocket.tx.clone(), handshake, self.config.clone());
            if let Some(tx) = esocket.data.connect_recv_tx.lock().unwrap().take() {
                tx.send(()).unwrap();
            }
            if let Err(err) = socket.send(Packet::connect(ns_path.clone(), sid)) {
                debug!("sending error during socket connection: {err:?}");
            }
            Ok(())
        } else {
            esocket
                .tx
                .try_send(Packet::invalid_namespace(ns_path).try_into()?)
                .unwrap();
            Ok(())
        }
    }

    /// Cache-in the socket data until all the binary payloads are received
    fn sock_recv_bin_packet(&self, socket: &EIoSocket<Self>, packet: Packet) {
        socket
            .data
            .partial_bin_packet
            .lock()
            .unwrap()
            .replace(packet);
    }

    /// Propagate a packet to a its target namespace
    fn sock_propagate_packet(&self, packet: Packet, sid: Sid) -> Result<(), Error> {
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

#[derive(Debug, Default)]
pub struct SocketData {
    /// Partial binary packet that is being received
    /// Stored here until all the binary payloads are received
    pub partial_bin_packet: Mutex<Option<Packet>>,

    /// Channel used to notify the socket that it has been connected to a namespace
    pub connect_recv_tx: Mutex<Option<oneshot::Sender<()>>>,
}

#[engineioxide::async_trait]
impl<A: Adapter> EngineIoHandler for Client<A> {
    type Data = SocketData;

    fn on_connect(&self, socket: &EIoSocket<Self>) {
        debug!("eio socket connect {}", socket.sid);
        let (tx, rx) = oneshot::channel();
        socket.data.connect_recv_tx.lock().unwrap().replace(tx);
        // tokio::spawn(tokio::time::timeout(timeout, rx).map_err(|_| {
        // debug!("connect timeout for socket {}", socket.sid);
        // (close_fn)(EIoDisconnectReason::TransportClose);
        // }));
    }
    fn on_disconnect(&self, socket: &EIoSocket<Self>, reason: EIoDisconnectReason) {
        debug!("eio socket disconnect {}", socket.sid);
        let data = self
            .ns
            .values()
            .filter_map(|ns| ns.get_socket(socket.sid).ok())
            .map(|s| s.close(reason.clone().into()));
        if let Err(e) = data.collect::<Result<Vec<_>, _>>() {
            error!("Adapter error when disconnecting {}: {}, in a multiple server scenario it could leads to desyncronisation issues", socket.sid, e);
        }
    }

    fn on_message(&self, msg: String, socket: &EIoSocket<Self>) {
        debug!("Received message: {:?}", msg);
        let packet = match Packet::try_from(msg) {
            Ok(packet) => packet,
            Err(e) => {
                debug!("socket serialization error: {}", e);
                socket.close(EIoDisconnectReason::PacketParsingError);
                return;
            }
        };
        debug!("Packet: {:?}", packet);

        let res: Result<(), Error> = match packet.inner {
            PacketData::Connect(auth) => self
                .sock_connect(auth, packet.ns, socket)
                .map_err(Into::into),
            PacketData::BinaryEvent(_, _, _) | PacketData::BinaryAck(_, _) => {
                self.sock_recv_bin_packet(socket, packet);
                Ok(())
            }
            _ => self.sock_propagate_packet(packet, socket.sid),
        };
        if let Err(ref err) = res {
            error!(
                "error while processing packet to socket {}: {}",
                socket.sid, err
            );
            if let Some(reason) = err.into() {
                socket.close(reason);
            }
        }
    }

    /// When a binary payload is received from a socket, it is applied to the partial binary packet
    ///
    /// If the packet is complete, it is propagated to the namespace
    fn on_binary(&self, data: Vec<u8>, socket: &EIoSocket<Self>) {
        if self.apply_payload_on_packet(data, socket) {
            if let Some(packet) = socket.data.partial_bin_packet.lock().unwrap().take() {
                if let Err(ref err) = self.sock_propagate_packet(packet, socket.sid) {
                    debug!(
                        "error while propagating packet to socket {}: {}",
                        socket.sid, err
                    );
                    if let Some(reason) = err.into() {
                        socket.close(reason);
                    }
                }
            }
        }
    }
}

impl<A: Adapter> Clone for Client<A> {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            ns: self.ns.clone(),
        }
    }
}
