use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use serde_json::Value;

use crate::{
    client::Client,
    packet::{Packet, PacketData},
    socket::Socket,
};

pub type EventCallback = fn(Arc<Socket>);

//TODO: Dynamic type
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

    /// Connects a client to a namespace
    pub fn connect(&self, sid: i64, client: Arc<Client>) {
        let socket: Arc<Socket> = Socket::new(client, self.path.clone(), sid).into();
        self.sockets.write().unwrap().insert(sid, socket.clone());
        (self.callback)(socket);
    }

    /// Called when a namespace receive a particular packet that should be transmitted to the socket
    pub fn recv_event(&self, sid: i64, e: String, data: Value) {
        if let Some(socket) = self.get_socket(sid) {
            socket.recv_event(e, data);
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

    pub fn add<P: Into<String>>(mut self, path: P, callback: EventCallback) -> Self {
        self.ns_handlers.insert(path.into(), callback);
        self
    }
    pub fn add_many<P: Into<String>>(mut self, paths: Vec<P>, callback: EventCallback) -> Self {
        for path in paths {
            self.ns_handlers.insert(path.into(), callback);
        }
        self
    }

    pub fn build(self) -> NsHandlers {
        self.ns_handlers
    }
}
