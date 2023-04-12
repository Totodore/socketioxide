use std::{
    collections::HashMap,
    pin::Pin,
    sync::{Arc, RwLock},
};

use futures::Future;

use crate::{
    client::Client, errors::Error, handshake::Handshake, packet::PacketData, socket::Socket,
};

pub type EventCallback = Arc<
    dyn Fn(Arc<Socket>) -> Pin<Box<dyn Future<Output = ()> + Send + Sync + 'static>>
        + Send
        + Sync
        + 'static,
>;

pub type NsHandlers = HashMap<String, EventCallback>;

pub struct Namespace {
    pub path: String,
    callback: EventCallback,
    sockets: RwLock<HashMap<i64, Arc<Socket>>>,
}

impl Namespace {
    //TODO: enforce path format
    pub fn new(path: String, callback: EventCallback) -> Self {
        Self {
            path,
            callback,
            sockets: HashMap::new().into(),
        }
    }
    pub fn builder() -> NamespaceBuilder {
        NamespaceBuilder::new()
    }

    /// Connects a socket to a namespace
    pub fn connect(&self, sid: i64, client: Arc<Client>, handshake: Handshake) {
        let socket: Arc<Socket> = Socket::new(client, handshake, self.path.clone(), sid).into();
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
            PacketData::Connect(_) => unreachable!(),
            PacketData::ConnectError(_) => unreachable!(),
            PacketData::Disconnect => {
                self.disconnect(sid);
                Ok(())
            }
            packet => self.socket_recv(sid, packet),
        }
    }
    fn get_socket(&self, sid: i64) -> Option<Arc<Socket>> {
        self.sockets.read().unwrap().get(&sid).cloned()
    }
}

pub struct NamespaceBuilder {
    ns_handlers: HashMap<String, EventCallback>,
}

impl NamespaceBuilder {
    fn new() -> Self {
        Self {
            ns_handlers: HashMap::new(),
        }
    }

    pub fn add<C, F>(mut self, path: impl Into<String>, callback: C) -> Self
    where
        C: Fn(Arc<Socket>) -> F + Send + Sync + 'static,
        F: Future<Output = ()> + Send + Sync + 'static,
    {
        let handler = Arc::new(move |socket| Box::pin(callback(socket)) as _);
        self.ns_handlers.insert(path.into(), handler);
        self
    }
    pub fn add_many<C, F>(mut self, paths: Vec<impl Into<String>>, callback: C) -> Self
    where
        C: Fn(Arc<Socket>) -> F + Send + Sync + 'static,
        F: Future<Output = ()> + Send + Sync + 'static,
    {
        let handler = Arc::new(move |socket| Box::pin(callback(socket)) as _);
        for path in paths {
            self.ns_handlers.insert(path.into(), handler.clone());
        }
        self
    }

    pub fn build(self) -> NsHandlers {
        self.ns_handlers
    }
}
