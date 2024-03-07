use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use bytes::Bytes;
use engineioxide::handler::EngineIoHandler;
use engineioxide::socket::{DisconnectReason as EIoDisconnectReason, Socket as EIoSocket};
use futures::TryFutureExt;

use engineioxide::sid::Sid;
use tokio::sync::oneshot;

use crate::adapter::Adapter;
use crate::handler::ConnectHandler;
use crate::ProtocolVersion;
use crate::{
    errors::Error,
    ns::Namespace,
    packet::{Packet, PacketData},
    SocketIoConfig,
};

#[derive(Debug)]
pub struct Client<A: Adapter> {
    pub(crate) config: Arc<SocketIoConfig>,
    ns: RwLock<HashMap<Cow<'static, str>, Arc<Namespace<A>>>>,
}

impl<A: Adapter> Client<A> {
    pub fn new(config: Arc<SocketIoConfig>) -> Self {
        #[cfg(feature = "state")]
        crate::state::freeze_state();

        Self {
            config,
            ns: RwLock::new(HashMap::new()),
        }
    }

    /// Called when a socket connects to a new namespace
    fn sock_connect(
        &self,
        auth: Option<String>,
        ns_path: &str,
        esocket: &Arc<engineioxide::Socket<SocketData>>,
    ) -> Result<(), Error> {
        #[cfg(feature = "tracing")]
        tracing::debug!("auth: {:?}", auth);

        let sid = esocket.id;
        if let Some(ns) = self.get_ns(ns_path) {
            ns.connect(sid, esocket.clone(), auth, self.config.clone())?;

            // cancel the connect timeout task for v5
            if let Some(tx) = esocket.data.connect_recv_tx.lock().unwrap().take() {
                tx.send(()).ok();
            }

            Ok(())
        } else if ProtocolVersion::from(esocket.protocol) == ProtocolVersion::V4 && ns_path == "/" {
            #[cfg(feature = "tracing")]
            tracing::error!(
                "the root namespace \"/\" must be defined before any connection for protocol V4 (legacy)!"
            );
            esocket.close(EIoDisconnectReason::TransportClose);
            Ok(())
        } else {
            let packet = Packet::invalid_namespace(ns_path).into();
            if let Err(_e) = esocket.emit(packet) {
                #[cfg(feature = "tracing")]
                tracing::error!("error while sending invalid namespace packet: {}", _e);
            }
            Ok(())
        }
    }

    /// Propagate a packet to a its target namespace
    fn sock_propagate_packet(&self, packet: Packet<'_>, sid: Sid) -> Result<(), Error> {
        if let Some(ns) = self.get_ns(&packet.ns) {
            ns.recv(sid, packet.inner)
        } else {
            #[cfg(feature = "tracing")]
            tracing::debug!("invalid namespace requested: {}", packet.ns);
            Ok(())
        }
    }

    /// Spawn a task that will close the socket if it is not connected to a namespace
    /// after the [`SocketIoConfig::connect_timeout`] duration
    fn spawn_connect_timeout_task(&self, socket: Arc<EIoSocket<SocketData>>) {
        #[cfg(feature = "tracing")]
        tracing::debug!("spawning connect timeout task");
        let (tx, rx) = oneshot::channel();
        socket.data.connect_recv_tx.lock().unwrap().replace(tx);

        tokio::spawn(
            tokio::time::timeout(self.config.connect_timeout, rx).map_err(move |_| {
                #[cfg(feature = "tracing")]
                tracing::debug!("connect timeout for socket {}", socket.id);
                socket.close(EIoDisconnectReason::TransportClose);
            }),
        );
    }

    /// Adds a new namespace handler
    pub fn add_ns<C, T>(&self, path: Cow<'static, str>, callback: C)
    where
        C: ConnectHandler<A, T>,
        T: Send + Sync + 'static,
    {
        #[cfg(feature = "tracing")]
        tracing::debug!("adding namespace {}", path);
        let ns = Namespace::new(path.clone(), callback);
        self.ns.write().unwrap().insert(path, ns);
    }

    /// Deletes a namespace handler
    pub fn delete_ns(&self, path: &str) {
        #[cfg(feature = "tracing")]
        tracing::debug!("deleting namespace {}", path);
        self.ns.write().unwrap().remove(path);
    }

    pub fn get_ns(&self, path: &str) -> Option<Arc<Namespace<A>>> {
        self.ns.read().unwrap().get(path).cloned()
    }

    /// Closes all engine.io connections and all clients
    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self)))]
    pub(crate) async fn close(&self) {
        #[cfg(feature = "tracing")]
        tracing::debug!("closing all namespaces");
        let ns = self.ns.read().unwrap().clone();
        futures::future::join_all(ns.values().map(|ns| ns.close())).await;
        #[cfg(feature = "tracing")]
        tracing::debug!("all namespaces closed");
    }
}

#[derive(Debug, Default)]
pub struct SocketData {
    /// Partial binary packet that is being received
    /// Stored here until all the binary payloads are received
    pub partial_bin_packet: Mutex<Option<Packet<'static>>>,

    /// Channel used to notify the socket that it has been connected to a namespace for v5
    pub connect_recv_tx: Mutex<Option<oneshot::Sender<()>>>,
}

impl<A: Adapter> EngineIoHandler for Client<A> {
    type Data = SocketData;

    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self, socket), fields(sid = socket.id.to_string())))]
    fn on_connect(&self, socket: Arc<EIoSocket<SocketData>>) {
        #[cfg(feature = "tracing")]
        tracing::debug!("eio socket connect");

        let protocol: ProtocolVersion = socket.protocol.into();

        // Connecting the client to the default namespace is mandatory if the SocketIO protocol is v4.
        // Because we connect by default to the root namespace, we should ensure before that the root namespace is defined
        #[cfg(feature = "v4")]
        if protocol == ProtocolVersion::V4 {
            #[cfg(feature = "tracing")]
            tracing::debug!("connecting to default namespace for v4");
            self.sock_connect(None, "/", &socket).unwrap();
        }

        if protocol == ProtocolVersion::V5 {
            self.spawn_connect_timeout_task(socket);
        }
    }

    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self, socket), fields(sid = socket.id.to_string())))]
    fn on_disconnect(&self, socket: Arc<EIoSocket<SocketData>>, reason: EIoDisconnectReason) {
        #[cfg(feature = "tracing")]
        tracing::debug!("eio socket disconnected");
        let _res: Result<Vec<_>, _> = self
            .ns
            .read()
            .unwrap()
            .values()
            .filter_map(|ns| ns.get_socket(socket.id).ok())
            .map(|s| s.close(reason.clone().into()))
            .collect();

        #[cfg(feature = "tracing")]
        match _res {
            Ok(vec) => {
                tracing::debug!("disconnect handle spawned for {} namespaces", vec.len())
            }
            Err(_e) => {
                tracing::debug!("error while disconnecting socket: {}", _e)
            }
        }
    }

    fn on_message(&self, msg: String, socket: Arc<EIoSocket<SocketData>>) {
        #[cfg(feature = "tracing")]
        tracing::debug!("Received message: {:?}", msg);
        let packet = match Packet::try_from(msg) {
            Ok(packet) => packet,
            Err(_e) => {
                #[cfg(feature = "tracing")]
                tracing::debug!("socket serialization error: {}", _e);
                socket.close(EIoDisconnectReason::PacketParsingError);
                return;
            }
        };
        #[cfg(feature = "tracing")]
        tracing::debug!("Packet: {:?}", packet);

        let res: Result<(), Error> = match packet.inner {
            PacketData::Connect(auth) => self
                .sock_connect(auth, &packet.ns, &socket)
                .map_err(Into::into),
            PacketData::BinaryEvent(_, _, _) | PacketData::BinaryAck(_, _) => {
                // Cache-in the socket data until all the binary payloads are received
                socket
                    .data
                    .partial_bin_packet
                    .lock()
                    .unwrap()
                    .replace(packet);
                Ok(())
            }
            _ => self.sock_propagate_packet(packet, socket.id),
        };
        if let Err(ref err) = res {
            #[cfg(feature = "tracing")]
            tracing::debug!(
                "error while processing packet to socket {}: {}",
                socket.id,
                err
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
        if apply_payload_on_packet(data.into(), &socket) {
            if let Some(packet) = socket.data.partial_bin_packet.lock().unwrap().take() {
                if let Err(ref err) = self.sock_propagate_packet(packet, socket.id) {
                    #[cfg(feature = "tracing")]
                    tracing::debug!(
                        "error while propagating packet to socket {}: {}",
                        socket.id,
                        err
                    );
                    if let Some(reason) = err.into() {
                        socket.close(reason);
                    }
                }
            }
        }
    }
}

/// Utility that applies an incoming binary payload to a partial binary packet
/// waiting to be filled with all the payloads
///
/// Returns true if the packet is complete and should be processed
fn apply_payload_on_packet(data: Bytes, socket: &EIoSocket<SocketData>) -> bool {
    #[cfg(feature = "tracing")]
    tracing::debug!("[sid={}] applying payload on packet", socket.id);
    if let Some(ref mut packet) = *socket.data.partial_bin_packet.lock().unwrap() {
        match packet.inner {
            PacketData::BinaryEvent(_, ref mut bin, _) | PacketData::BinaryAck(ref mut bin, _) => {
                bin.add_payload(data);
                bin.is_complete()
            }
            _ => unreachable!("partial_bin_packet should only be set for binary packets"),
        }
    } else {
        #[cfg(feature = "tracing")]
        tracing::debug!("[sid={}] socket received unexpected bin data", socket.id);
        false
    }
}

#[cfg(test)]
mod test {
    use tokio::sync::mpsc;

    use crate::adapter::LocalAdapter;
    const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(10);

    fn create_client() -> super::Client<LocalAdapter> {
        let config = crate::SocketIoConfig {
            connect_timeout: CONNECT_TIMEOUT,
            ..Default::default()
        };
        let client = Client::<LocalAdapter>::new(std::sync::Arc::new(config));
        client.add_ns("/".into(), || {});
        client
    }

    use super::*;
    #[tokio::test]
    async fn connect_timeout_fail() {
        let client = create_client();
        let (tx, mut rx) = mpsc::channel(1);
        let close_fn = Box::new(move |_, _| tx.try_send(()).unwrap());
        let sock = Arc::new(EIoSocket::new_dummy(Sid::new(), close_fn));
        client.on_connect(sock.clone());
        tokio::time::timeout(CONNECT_TIMEOUT * 2, rx.recv())
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn connect_timeout() {
        let client = create_client();
        let (tx, mut rx) = mpsc::channel(1);
        let close_fn = Box::new(move |_, _| tx.try_send(()).unwrap());
        let sock = Arc::new(EIoSocket::new_dummy(Sid::new(), close_fn));
        client.on_connect(sock.clone());
        client.on_message("0".into(), sock.clone());
        tokio::time::timeout(CONNECT_TIMEOUT * 2, rx.recv())
            .await
            .unwrap_err();
    }
}
