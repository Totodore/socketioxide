use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use crate::{
    adapter::Adapter, errors::Error, handshake::Handshake, packet::PacketData, socket::Socket,
    SocketIoConfig,
};
use crate::{client::SocketData, errors::AdapterError};
use engineioxide::sid_generator::Sid;
use futures::future::BoxFuture;

pub type EventCallback<A> =
    Arc<dyn Fn(Arc<Socket<A>>) -> BoxFuture<'static, ()> + Send + Sync + 'static>;

pub type NsHandlers<A> = HashMap<String, EventCallback<A>>;

pub struct Namespace<A: Adapter> {
    pub path: String,
    pub(crate) adapter: A,
    callback: EventCallback<A>,
    sockets: RwLock<HashMap<Sid, Arc<Socket<A>>>>,
}

impl<A: Adapter> Namespace<A> {
    pub fn new(path: impl Into<String>, callback: EventCallback<A>) -> Arc<Self> {
        let mut path: String = path.into();
        if !path.starts_with('/') {
            path = format!("/{}", path);
        }
        Arc::new_cyclic(|ns| Self {
            path,
            callback,
            sockets: HashMap::new().into(),
            adapter: A::new(ns.clone()),
        })
    }

    /// Connects a socket to a namespace
    pub fn connect(
        self: Arc<Self>,
        sid: Sid,
        esocket: Arc<engineioxide::Socket<SocketData>>,
        handshake: Handshake,
        config: Arc<SocketIoConfig>,
    ) {
        let socket: Arc<Socket<A>> =
            Socket::new(sid, self.clone(), handshake, esocket, config).into();
        self.sockets.write().unwrap().insert(sid, socket.clone());
        tokio::spawn((self.callback)(socket));
    }

    /// Remove a socket from a namespace and propagate the event to the adapter
    pub fn remove_socket(&self, sid: Sid) -> Result<(), AdapterError> {
        self.sockets.write().unwrap().remove(&sid);
        self.adapter
            .del_all(sid)
            .map_err(|err| AdapterError(Box::new(err)))
    }

    pub fn has(&self, sid: Sid) -> bool {
        self.sockets.read().unwrap().values().any(|s| s.sid == sid)
    }

    pub fn recv(&self, sid: Sid, packet: PacketData) -> Result<(), Error> {
        match packet {
            PacketData::Connect(_) => unreachable!("connect packets should be handled before"),
            PacketData::ConnectError(_) => Ok(()),
            packet => self.get_socket(sid)?.recv(packet),
        }
    }
    pub fn get_socket(&self, sid: Sid) -> Result<Arc<Socket<A>>, Error> {
        self.sockets
            .read()
            .unwrap()
            .get(&sid)
            .cloned()
            .ok_or(Error::SocketGone(sid))
    }
    pub fn get_sockets(&self) -> Vec<Arc<Socket<A>>> {
        self.sockets.read().unwrap().values().cloned().collect()
    }

    /// Close the entire namespace :
    /// * Close the adapter
    /// * Close all the sockets and their underlying connections
    /// * Remove all the sockets from the namespace
    pub async fn close(&self) {
        self.adapter.close().ok();
        tracing::debug!("closing all sockets in namespace {}", self.path);
        let sockets = self.sockets.read().unwrap().clone();
        futures::future::join_all(sockets.values().map(|s| s.close_underlying_transport())).await;
        self.sockets.write().unwrap().shrink_to_fit();
        tracing::debug!("all sockets in namespace {} closed", self.path);
    }
}

#[cfg(test)]
impl<A: Adapter> Namespace<A> {
    pub fn new_dummy<const S: usize>(sockets: [Sid; S]) -> Arc<Self> {
        use futures::future::FutureExt;
        let ns = Namespace::new("/", Arc::new(|_| async move {}.boxed()));
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

impl<A: Adapter + std::fmt::Debug> std::fmt::Debug for Namespace<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Namespace")
            .field("path", &self.path)
            .field("adapter", &self.adapter)
            .field("sockets", &self.sockets)
            .finish()
    }
}
