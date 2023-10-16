use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use engineioxide::handler::EngineIoHandler;
use engineioxide::socket::{DisconnectReason as EIoDisconnectReason, Socket as EIoSocket};
use futures::{Future, TryFutureExt};
use serde::de::DeserializeOwned;
use serde_json::Value;

use engineioxide::sid_generator::Sid;
use tokio::sync::oneshot;
use tracing::debug;
use tracing::error;

use crate::adapter::Adapter;
use crate::Socket;
use crate::{
    errors::Error,
    ns::Namespace,
    packet::{Packet, PacketData},
    SocketIoConfig,
};

#[derive(Debug)]
pub struct Client<A: Adapter> {
    pub(crate) config: Arc<SocketIoConfig>,
    ns: RwLock<HashMap<String, Arc<Namespace<A>>>>,
}

impl<A: Adapter> Client<A> {
    pub fn new(config: Arc<SocketIoConfig>) -> Self {
        Self {
            config,
            ns: RwLock::new(HashMap::new()),
        }
    }

    /// Apply an incoming binary payload to a partial binary packet waiting to be filled with all the payloads
    ///
    /// Returns true if the packet is complete and should be processed
    fn apply_payload_on_packet(&self, data: Vec<u8>, socket: &EIoSocket<SocketData>) -> bool {
        debug!("[sid={}] applying payload on packet", socket.id);
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
            debug!("[sid={}] socket received unexpected bin data", socket.id);
            false
        }
    }

    /// Called when a socket connects to a new namespace
    fn sock_connect(
        &self,
        auth: Value,
        ns_path: String,
        esocket: Arc<engineioxide::Socket<SocketData>>,
    ) -> Result<(), serde_json::Error> {
        debug!("auth: {:?}", auth);
        let sid = esocket.id;
        if let Some(ns) = self.get_ns(&ns_path) {
            let connect_packet = Packet::connect(ns_path.clone(), sid).try_into()?;
            if let Err(err) = esocket.emit(connect_packet) {
                debug!("sending error during socket connection: {err:?}");
            }
            ns.connect(sid, esocket.clone(), auth, self.config.clone())?;
            if let Some(tx) = esocket.data.connect_recv_tx.lock().unwrap().take() {
                tx.send(()).unwrap();
            }
            Ok(())
        } else {
            let packet = Packet::invalid_namespace(ns_path).try_into()?;
            esocket.emit(packet).unwrap();
            Ok(())
        }
    }

    /// Cache-in the socket data until all the binary payloads are received
    fn sock_recv_bin_packet(&self, socket: &EIoSocket<SocketData>, packet: Packet) {
        socket
            .data
            .partial_bin_packet
            .lock()
            .unwrap()
            .replace(packet);
    }

    /// Propagate a packet to a its target namespace
    fn sock_propagate_packet(&self, packet: Packet, sid: Sid) -> Result<(), Error> {
        if let Some(ns) = self.get_ns(&packet.ns) {
            ns.recv(sid, packet.inner)
        } else {
            debug!("invalid namespace requested: {}", packet.ns);
            Ok(())
        }
    }

    /// Add a new namespace handler
    pub fn add_ns<C, F, V>(&self, path: String, callback: C)
    where
        C: Fn(Arc<Socket<A>>, V) -> F + Send + Sync + 'static,
        F: Future<Output = ()> + Send + 'static,
        V: DeserializeOwned + Send + Sync + 'static,
    {
        debug!("adding namespace {}", path);
        let ns = Namespace::new(path.clone(), callback);
        self.ns.write().unwrap().insert(path, ns);
    }

    /// Delete a namespace handler
    pub fn delete_ns(&self, path: &str) {
        debug!("deleting namespace {}", path);
        self.ns.write().unwrap().remove(path);
    }

    pub fn get_ns(&self, path: &str) -> Option<Arc<Namespace<A>>> {
        self.ns.read().unwrap().get(path).cloned()
    }

    /// Close all engine.io connections and all clients
    #[tracing::instrument(skip(self))]
    pub(crate) async fn close(&self) {
        debug!("closing all namespaces");
        let ns = self.ns.read().unwrap().clone();
        futures::future::join_all(ns.values().map(|ns| ns.close())).await;
        debug!("all namespaces closed");
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

    fn on_connect(&self, socket: Arc<EIoSocket<SocketData>>) {
        debug!("eio socket connect {}", socket.id);
        let (tx, rx) = oneshot::channel();
        socket.data.connect_recv_tx.lock().unwrap().replace(tx);
        // let socket_tx = socket.tx.clone();
        let sid = socket.id;
        tokio::spawn(
            tokio::time::timeout(self.config.connect_timeout, rx).map_err(move |_| {
                debug!("connect timeout for socket {}", sid);
                socket.close(EIoDisconnectReason::TransportClose);
            }),
        );
    }

    #[tracing::instrument(skip(self, socket), fields(sid = socket.id.to_string()))]
    fn on_disconnect(&self, socket: Arc<EIoSocket<SocketData>>, reason: EIoDisconnectReason) {
        debug!("eio socket disconnected");
        let res: Result<Vec<_>, _> = self
            .ns
            .read()
            .unwrap()
            .values()
            .filter_map(|ns| ns.get_socket(socket.id).ok())
            .map(|s| s.close(reason.clone().into()))
            .collect();
        match res {
            Ok(vec) => debug!("disconnect handle spawned for {} namespaces", vec.len()),
            Err(e) => error!("error while disconnecting socket: {}", e),
        }
    }

    fn on_message(&self, msg: String, socket: Arc<EIoSocket<SocketData>>) {
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
                .sock_connect(auth, packet.ns, socket.clone())
                .map_err(Into::into),
            PacketData::BinaryEvent(_, _, _) | PacketData::BinaryAck(_, _) => {
                self.sock_recv_bin_packet(&socket, packet);
                Ok(())
            }
            _ => self.sock_propagate_packet(packet, socket.id),
        };
        if let Err(ref err) = res {
            error!(
                "error while processing packet to socket {}: {}",
                socket.id, err
            );
            if let Some(reason) = err.into() {
                socket.close(reason);
            }
        }
    }

    /// When a binary payload is received from a socket, it is applied to the partial binary packet
    ///
    /// If the packet is complete, it is propagated to the namespace
    fn on_binary(&self, data: Vec<u8>, socket: Arc<EIoSocket<SocketData>>) {
        if self.apply_payload_on_packet(data, &socket) {
            if let Some(packet) = socket.data.partial_bin_packet.lock().unwrap().take() {
                if let Err(ref err) = self.sock_propagate_packet(packet, socket.id) {
                    debug!(
                        "error while propagating packet to socket {}: {}",
                        socket.id, err
                    );
                    if let Some(reason) = err.into() {
                        socket.close(reason);
                    }
                }
            }
        }
    }
}
