use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use crate::errors::AdapterError;
use crate::{
    adapter::{Adapter, LocalAdapter},
    errors::Error,
    handshake::Handshake,
    packet::PacketData,
    socket::Socket,
    SocketIoConfig,
};
use engineioxide::sid_generator::Sid;
use engineioxide::SendPacket as EnginePacket;
use futures::{future::BoxFuture, Future};
use tokio::sync::mpsc;

pub type EventCallback<A> =
    Arc<dyn Fn(Arc<Socket<A>>) -> BoxFuture<'static, ()> + Send + Sync + 'static>;

pub struct NsHandlers<A: Adapter>(pub(crate) HashMap<String, EventCallback<A>>);
impl<A: Adapter> NsHandlers<A> {
    fn new(map: HashMap<String, EventCallback<A>>) -> Self {
        Self(map)
    }
}
impl<A: Adapter> Clone for NsHandlers<A> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

pub struct Namespace<A: Adapter> {
    pub path: String,
    pub(crate) adapter: A,
    callback: EventCallback<A>,
    sockets: RwLock<HashMap<Sid, Arc<Socket<A>>>>,
}

impl Namespace<LocalAdapter> {
    pub fn builder() -> NamespaceBuilder<LocalAdapter> {
        NamespaceBuilder::new()
    }

    pub fn builder_with_adapter<CustomAdapter: Adapter>() -> NamespaceBuilder<CustomAdapter> {
        NamespaceBuilder::new()
    }
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
        tx: mpsc::Sender<EnginePacket>,
        handshake: Handshake,
        config: Arc<SocketIoConfig>,
    ) -> Arc<Socket<A>> {
        let socket: Arc<Socket<A>> = Socket::new(sid, self.clone(), handshake, tx, config).into();
        self.sockets.write().unwrap().insert(sid, socket.clone());
        tokio::spawn((self.callback)(socket.clone()));
        socket
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

pub struct NamespaceBuilder<A: Adapter> {
    ns_handlers: HashMap<String, EventCallback<A>>,
}

impl<A: Adapter> NamespaceBuilder<A> {
    fn new() -> Self {
        Self {
            ns_handlers: HashMap::new(),
        }
    }

    pub fn add<C, F>(mut self, path: impl Into<String>, callback: C) -> Self
    where
        C: Fn(Arc<Socket<A>>) -> F + Send + Sync + 'static,
        F: Future<Output = ()> + Send + 'static,
    {
        let handler = Arc::new(move |socket| Box::pin(callback(socket)) as _);
        self.ns_handlers.insert(path.into(), handler);
        self
    }
    pub fn add_many<C, F>(mut self, paths: Vec<impl Into<String>>, callback: C) -> Self
    where
        C: Fn(Arc<Socket<A>>) -> F + Send + Sync + 'static,
        F: Future<Output = ()> + Send + 'static,
    {
        let handler = Arc::new(move |socket| Box::pin(callback(socket)) as _);
        for path in paths {
            self.ns_handlers.insert(path.into(), handler.clone());
        }
        self
    }

    pub fn build(self) -> NsHandlers<A> {
        NsHandlers::new(self.ns_handlers)
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
