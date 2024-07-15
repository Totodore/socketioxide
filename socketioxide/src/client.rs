use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use bytes::Bytes;
use engineioxide::handler::EngineIoHandler;
use engineioxide::socket::{DisconnectReason as EIoDisconnectReason, Socket as EIoSocket};
use engineioxide::Str;
use futures_util::{FutureExt, TryFutureExt};

use engineioxide::sid::Sid;
use matchit::{Match, Router};
use tokio::sync::oneshot;

use crate::adapter::Adapter;
use crate::handler::ConnectHandler;
use crate::ns::NamespaceCtr;
use crate::socket::DisconnectReason;
use crate::{
    errors::Error,
    ns::Namespace,
    packet::{Packet, PacketData},
    SocketIoConfig,
};
use crate::{ProtocolVersion, SocketIo};

pub struct Client<A: Adapter> {
    pub(crate) config: SocketIoConfig,
    nsps: RwLock<HashMap<Str, Arc<Namespace<A>>>>,
    router: RwLock<Router<NamespaceCtr<A>>>,

    #[cfg(feature = "state")]
    pub(crate) state: state::TypeMap![Send + Sync],
}

/// ==== impl Client ====

impl<A: Adapter> Client<A> {
    pub fn new(
        config: SocketIoConfig,
        #[cfg(feature = "state")] mut state: state::TypeMap![Send + Sync],
    ) -> Self {
        #[cfg(feature = "state")]
        state.freeze();

        Self {
            config,
            nsps: RwLock::new(HashMap::new()),
            router: RwLock::new(Router::new()),
            #[cfg(feature = "state")]
            state,
        }
    }

    /// Called when a socket connects to a new namespace
    fn sock_connect(
        &self,
        auth: Option<String>,
        ns_path: Str,
        esocket: &Arc<engineioxide::Socket<SocketData<A>>>,
    ) {
        #[cfg(feature = "tracing")]
        tracing::debug!("auth: {:?}", auth);
        let protocol: ProtocolVersion = esocket.protocol.into();
        let connect =
            move |ns: Arc<Namespace<A>>, esocket: Arc<engineioxide::Socket<SocketData<A>>>| async move {
                if ns.connect(esocket.id, esocket.clone(), auth).await.is_ok() {
                    // cancel the connect timeout task for v5
                    if let Some(tx) = esocket.data.connect_recv_tx.lock().unwrap().take() {
                        tx.send(()).ok();
                    }
                }
            };

        if let Some(ns) = self.get_ns(&ns_path) {
            tokio::spawn(connect(ns, esocket.clone()));
        } else if let Ok(Match { value: ns_ctr, .. }) = self.router.read().unwrap().at(&ns_path) {
            // We have to create a new `Str` otherwise, we would keep a ref to the original connect packet
            // for the entire lifetime of the Namespace.
            let path = Str::copy_from_slice(&ns_path);
            let ns = ns_ctr.get_new_ns(path.clone());
            self.nsps.write().unwrap().insert(path, ns.clone());
            tokio::spawn(connect(ns, esocket.clone()));
        } else if protocol == ProtocolVersion::V4 && ns_path == "/" {
            #[cfg(feature = "tracing")]
            tracing::error!(
                "the root namespace \"/\" must be defined before any connection for protocol V4 (legacy)!"
            );
            esocket.close(EIoDisconnectReason::TransportClose);
        } else {
            let packet: String = Packet::connect_error(ns_path, "Invalid namespace").into();
            if let Err(_e) = esocket.emit(packet) {
                #[cfg(feature = "tracing")]
                tracing::error!("error while sending invalid namespace packet: {}", _e);
            }
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
    fn spawn_connect_timeout_task(&self, socket: Arc<EIoSocket<SocketData<A>>>) {
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
        let path = Str::from(path);
        let ns = Namespace::new(path.clone(), callback);
        self.nsps.write().unwrap().insert(path, ns);
    }

    pub fn add_dyn_ns<C, T>(&self, path: String, callback: C) -> Result<(), matchit::InsertError>
    where
        C: ConnectHandler<A, T>,
        T: Send + Sync + 'static,
    {
        #[cfg(feature = "tracing")]
        tracing::debug!("adding dynamic namespace {}", &path);

        let ns = NamespaceCtr::new(callback);
        self.router.write().unwrap().insert(path, ns)
    }

    /// Deletes a namespace handler and closes all the connections to it
    pub fn delete_ns(&self, path: &str) {
        #[cfg(feature = "v4")]
        if path == "/" {
            panic!("the root namespace \"/\" cannot be deleted for the socket.io v4 protocol. See https://socket.io/docs/v3/namespaces/#main-namespace for more info");
        }

        #[cfg(feature = "tracing")]
        tracing::debug!("deleting namespace {}", path);
        if let Some(ns) = self.nsps.write().unwrap().remove(path) {
            ns.close(DisconnectReason::ServerNSDisconnect)
                .now_or_never();
        }
    }

    pub fn get_ns(&self, path: &str) -> Option<Arc<Namespace<A>>> {
        self.nsps.read().unwrap().get(path).cloned()
    }

    /// Closes all engine.io connections and all clients
    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self)))]
    pub(crate) async fn close(&self) {
        #[cfg(feature = "tracing")]
        tracing::debug!("closing all namespaces");
        let ns = { std::mem::take(&mut *self.nsps.write().unwrap()) };
        futures_util::future::join_all(
            ns.values()
                .map(|ns| ns.close(DisconnectReason::ClosingServer)),
        )
        .await;
        #[cfg(feature = "tracing")]
        tracing::debug!("all namespaces closed");
    }
}

#[derive(Debug)]
pub struct SocketData<A: Adapter> {
    /// Partial binary packet that is being received
    /// Stored here until all the binary payloads are received
    pub partial_bin_packet: Mutex<Option<Packet<'static>>>,

    /// Channel used to notify the socket that it has been connected to a namespace for v5
    pub connect_recv_tx: Mutex<Option<oneshot::Sender<()>>>,

    /// Used to store the [`SocketIo`] instance so it can be accessed by any sockets
    pub io: OnceLock<SocketIo<A>>,
}
impl<A: Adapter> Default for SocketData<A> {
    fn default() -> Self {
        Self {
            partial_bin_packet: Default::default(),
            connect_recv_tx: Default::default(),
            io: OnceLock::new(),
        }
    }
}

impl<A: Adapter> EngineIoHandler for Client<A> {
    type Data = SocketData<A>;

    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self, socket), fields(sid = socket.id.to_string())))]
    fn on_connect(self: Arc<Self>, socket: Arc<EIoSocket<SocketData<A>>>) {
        socket.data.io.set(SocketIo::from(self.clone())).ok();

        #[cfg(feature = "tracing")]
        tracing::debug!("eio socket connect");

        let protocol: ProtocolVersion = socket.protocol.into();

        // Connecting the client to the default namespace is mandatory if the SocketIO protocol is v4.
        // Because we connect by default to the root namespace, we should ensure before that the root namespace is defined
        match protocol {
            ProtocolVersion::V4 => {
                #[cfg(feature = "tracing")]
                tracing::debug!("connecting to default namespace for v4");
                self.sock_connect(None, Str::from("/"), &socket);
            }
            ProtocolVersion::V5 => self.spawn_connect_timeout_task(socket),
        }
    }

    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self, socket), fields(sid = socket.id.to_string())))]
    fn on_disconnect(&self, socket: Arc<EIoSocket<SocketData<A>>>, reason: EIoDisconnectReason) {
        #[cfg(feature = "tracing")]
        tracing::debug!("eio socket disconnected");
        let socks: Vec<_> = self
            .nsps
            .read()
            .unwrap()
            .values()
            .filter_map(|ns| ns.get_socket(socket.id).ok())
            .collect();

        let _res: Result<Vec<_>, _> = socks
            .into_iter()
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

    fn on_message(&self, msg: Str, socket: Arc<EIoSocket<SocketData<A>>>) {
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
            PacketData::Connect(auth) => {
                self.sock_connect(auth, packet.ns, &socket);
                Ok(())
            }
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
    fn on_binary(&self, data: Bytes, socket: Arc<EIoSocket<SocketData<A>>>) {
        if apply_payload_on_packet(data, &socket) {
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
impl<A: Adapter> std::fmt::Debug for Client<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut f = f.debug_struct("Client");
        f.field("config", &self.config).field("nsps", &self.nsps);
        #[cfg(feature = "state")]
        let f = f.field("state", &self.state);
        f.finish()
    }
}

/// Utility that applies an incoming binary payload to a partial binary packet
/// waiting to be filled with all the payloads
///
/// Returns true if the packet is complete and should be processed
fn apply_payload_on_packet<A: Adapter>(data: Bytes, socket: &EIoSocket<SocketData<A>>) -> bool {
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

#[cfg(socketioxide_test)]
impl<A: Adapter> Client<A> {
    pub async fn new_dummy_sock(
        self: Arc<Self>,
        ns: &'static str,
        auth: impl serde::Serialize,
    ) -> (
        tokio::sync::mpsc::Sender<engineioxide::Packet>,
        tokio::sync::mpsc::Receiver<engineioxide::Packet>,
    ) {
        let buffer_size = self.config.engine_config.max_buffer_size;
        let sid = Sid::new();
        let (esock, rx) =
            EIoSocket::<SocketData<A>>::new_dummy_piped(sid, Box::new(|_, _| {}), buffer_size);
        esock.data.io.set(SocketIo::from(self.clone())).ok();
        let (tx1, mut rx1) = tokio::sync::mpsc::channel(buffer_size);
        tokio::spawn({
            let esock = esock.clone();
            let client = self.clone();
            async move {
                while let Some(packet) = rx1.recv().await {
                    match packet {
                        engineioxide::Packet::Message(msg) => {
                            client.on_message(msg, esock.clone());
                        }
                        engineioxide::Packet::Close => {
                            client
                                .on_disconnect(esock.clone(), EIoDisconnectReason::TransportClose);
                        }
                        engineioxide::Packet::Binary(bin) => {
                            client.on_binary(bin, esock.clone());
                        }
                        _ => {}
                    }
                }
            }
        });
        let p: String = Packet {
            ns: ns.into(),
            inner: PacketData::Connect(Some(serde_json::to_string(&auth).unwrap())),
        }
        .into();
        self.on_message(p.into(), esock.clone());

        // wait for the socket to be connected to the namespace
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        (tx1, rx)
    }
}
#[cfg(test)]
mod test {
    use super::*;
    use tokio::sync::mpsc;

    use crate::adapter::LocalAdapter;
    const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(10);

    fn create_client() -> Arc<super::Client<LocalAdapter>> {
        let config = crate::SocketIoConfig {
            connect_timeout: CONNECT_TIMEOUT,
            ..Default::default()
        };
        let client = Client::<LocalAdapter>::new(
            config,
            #[cfg(feature = "state")]
            Default::default(),
        );
        client.add_ns("/".into(), || {});
        Arc::new(client)
    }

    #[tokio::test]
    async fn get_ns() {
        let client = create_client();
        let ns = Namespace::new(Str::from("/"), || {});
        client.nsps.write().unwrap().insert(Str::from("/"), ns);
        assert!(client.get_ns("/").is_some());
    }

    #[tokio::test]
    async fn io_should_always_be_set() {
        let client = create_client();
        let close_fn = Box::new(move |_, _| {});
        let sock = EIoSocket::new_dummy(Sid::new(), close_fn);
        client.on_connect(sock.clone());
        assert!(sock.data.io.get().is_some());
    }

    #[tokio::test]
    async fn connect_timeout_fail() {
        let client = create_client();
        let (tx, mut rx) = mpsc::channel(1);
        let close_fn = Box::new(move |_, _| tx.try_send(()).unwrap());
        let sock = EIoSocket::new_dummy(Sid::new(), close_fn);
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
        let sock = EIoSocket::new_dummy(Sid::new(), close_fn);
        client.clone().on_connect(sock.clone());
        client.on_message("0".into(), sock.clone());
        tokio::time::timeout(CONNECT_TIMEOUT * 2, rx.recv())
            .await
            .unwrap_err();
    }
}
