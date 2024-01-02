use std::{
    borrow::Cow,
    collections::HashMap,
    sync::{Arc, RwLock},
};

use crate::{
    adapter::{Adapter, AdapterBuilder},
    errors::Error,
    handler::{BoxedConnectHandler, ConnectHandler, MakeErasedHandler},
    packet::{Packet, PacketData},
    socket::Socket,
    SocketIoConfig,
};
use crate::{client::SocketData, errors::AdapterError};
use engineioxide::sid::Sid;

pub struct Namespace {
    pub path: Cow<'static, str>,
    pub(crate) adapter: Box<dyn Adapter>,
    handler: BoxedConnectHandler,
    sockets: RwLock<HashMap<Sid, Arc<Socket>>>,
}

impl Namespace {
    pub fn new<C, T>(path: Cow<'static, str>, handler: C, adapter: &AdapterBuilder) -> Arc<Self>
    where
        C: ConnectHandler<T> + Send + Sync + 'static,
        T: Send + Sync + 'static,
    {
        Arc::new_cyclic(|ns| Self {
            path,
            handler: MakeErasedHandler::new_ns_boxed(handler),
            sockets: HashMap::new().into(),
            adapter: adapter(ns.clone()),
        })
    }

    /// Connects a socket to a namespace
    pub fn connect(
        self: Arc<Self>,
        sid: Sid,
        esocket: Arc<engineioxide::Socket<SocketData>>,
        auth: Option<String>,
        config: Arc<SocketIoConfig>,
    ) -> Result<(), serde_json::Error> {
        let socket: Arc<Socket> = Socket::new(sid, self.clone(), esocket.clone(), config).into();

        self.sockets.write().unwrap().insert(sid, socket.clone());

        let protocol = esocket.protocol.into();
        if let Err(_e) = socket.send(Packet::connect(&self.path, socket.id, protocol)) {
            #[cfg(feature = "tracing")]
            tracing::debug!("error sending connect packet: {:?}, closing conn", _e);
            esocket.close(engineioxide::DisconnectReason::PacketParsingError);
            return Ok(());
        }

        self.handler.call(socket, auth);
        Ok(())
    }

    /// Removes a socket from a namespace and propagate the event to the adapter
    pub fn remove_socket(&self, sid: Sid) -> Result<(), AdapterError> {
        self.sockets.write().unwrap().remove(&sid);
        self.adapter
            .del_all(sid)
            .map_err(|err| AdapterError(Box::new(err)))
    }

    pub fn has(&self, sid: Sid) -> bool {
        self.sockets.read().unwrap().values().any(|s| s.id == sid)
    }

    pub fn recv(&self, sid: Sid, packet: PacketData<'_>) -> Result<(), Error> {
        match packet {
            PacketData::Connect(_) => unreachable!("connect packets should be handled before"),
            PacketData::ConnectError => Err(Error::InvalidPacketType),
            packet => self.get_socket(sid)?.recv(packet),
        }
    }
    pub fn get_socket(&self, sid: Sid) -> Result<Arc<Socket>, Error> {
        self.sockets
            .read()
            .unwrap()
            .get(&sid)
            .cloned()
            .ok_or(Error::SocketGone(sid))
    }
    pub fn get_sockets(&self) -> Vec<Arc<Socket>> {
        self.sockets.read().unwrap().values().cloned().collect()
    }

    /// Closes the entire namespace :
    /// * Closes the adapter
    /// * Closes all the sockets and their underlying connections
    /// * Removes all the sockets from the namespace
    pub async fn close(&self) {
        self.adapter.close().ok();
        #[cfg(feature = "tracing")]
        tracing::debug!("closing all sockets in namespace {}", self.path);
        let sockets = self.sockets.read().unwrap().clone();
        futures::future::join_all(sockets.values().map(|s| s.close_underlying_transport())).await;
        self.sockets.write().unwrap().shrink_to_fit();
        #[cfg(feature = "tracing")]
        tracing::debug!("all sockets in namespace {} closed", self.path);
    }
}

#[cfg(test)]
impl Namespace {
    pub fn new_dummy<const S: usize>(sockets: [Sid; S]) -> Arc<Self> {
        let ns = Namespace::new(Cow::Borrowed("/"), || {});
        for sid in sockets {
            ns.sockets
                .write()
                .unwrap()
                .insert(sid, Socket::new_dummy(sid, ns.clone()).into());
        }
        ns
    }

    pub fn clean_dummy_sockets(&self) {
        self.sockets.write().unwrap().clear();
    }
}

impl std::fmt::Debug for Namespace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Namespace")
            .field("path", &self.path)
            .field("adapter", &self.adapter)
            .field("sockets", &self.sockets)
            .finish()
    }
}
