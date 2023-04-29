use std::{
    collections::HashMap,
    pin::Pin,
    sync::{Arc, RwLock},
};

use futures::Future;

use crate::{
    adapter::{Adapter, LocalAdapter},
    client::Client,
    errors::Error,
    handshake::Handshake,
    packet::PacketData,
    socket::Socket,
};

pub type EventCallback<A> = Arc<
    dyn Fn(Arc<Socket<A>>) -> Pin<Box<dyn Future<Output = ()> + Send + Sync + 'static>>
        + Send
        + Sync
        + 'static,
>;

pub type NsHandlers<A> = HashMap<String, EventCallback<A>>;

pub struct Namespace<A: Adapter> {
    pub path: String,
    callback: EventCallback<A>,
    sockets: RwLock<HashMap<i64, Arc<Socket<A>>>>,
    adapter: A,
}

impl Namespace<LocalAdapter> {
    pub fn builder() -> NamespaceBuilder<LocalAdapter> {
        NamespaceBuilder::new()
    }
}

impl<A: Adapter> Namespace<A> {
    //TODO: enforce path format
    pub fn new(path: String, callback: EventCallback<A>) -> Arc<Self> {
        Arc::new_cyclic(|ns| Self {
            path,
            callback,
            sockets: HashMap::new().into(),
            adapter: A::new(ns.clone()),
        })
    }

    /// Connects a socket to a namespace
    pub fn connect(&self, sid: i64, client: Arc<Client<A>>, handshake: Handshake) {
        let socket: Arc<Socket<A>> =
            Socket::new(client.clone(), handshake, self.path.clone(), sid).into();
        self.sockets.write().unwrap().insert(sid, socket.clone());
        tokio::spawn((self.callback)(socket));
    }

    fn disconnect(&self, sid: i64) {
        self.sockets.write().unwrap().remove(&sid);
    }

    pub fn has(&self, sid: i64) -> bool {
        self.sockets.read().unwrap().values().any(|s| s.sid == sid)
    }

    /// Called when a namespace receive a particular packet that should be transmitted to the socket
    pub fn socket_recv(&self, sid: i64, packet: PacketData) -> Result<(), Error> {
        if let Some(socket) = self.get_socket(sid) {
            socket.recv(packet)?;
        }
        Ok(())
    }

    pub fn recv(&self, sid: i64, packet: PacketData) -> Result<(), Error> {
        match packet {
            PacketData::Disconnect => {
                self.disconnect(sid);
                Ok(())
            }
            PacketData::Connect(_) => {
                unreachable!("connect packets should not be handled in namespace")
            }
            PacketData::ConnectError(_) => Ok(()),
            packet => self.socket_recv(sid, packet),
        }
    }
    fn get_socket(&self, sid: i64) -> Option<Arc<Socket<A>>> {
        self.sockets.read().unwrap().get(&sid).cloned()
    }
}

pub struct NamespaceBuilder<A: Adapter = LocalAdapter> {
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
        F: Future<Output = ()> + Send + Sync + 'static,
    {
        let handler = Arc::new(move |socket| Box::pin(callback(socket)) as _);
        self.ns_handlers.insert(path.into(), handler);
        self
    }
    pub fn add_many<C, F>(mut self, paths: Vec<impl Into<String>>, callback: C) -> Self
    where
        C: Fn(Arc<Socket<A>>) -> F + Send + Sync + 'static,
        F: Future<Output = ()> + Send + Sync + 'static,
    {
        let handler = Arc::new(move |socket| Box::pin(callback(socket)) as _);
        for path in paths {
            self.ns_handlers.insert(path.into(), handler.clone());
        }
        self
    }

    pub fn build(self) -> NsHandlers<A> {
        self.ns_handlers
    }
}
