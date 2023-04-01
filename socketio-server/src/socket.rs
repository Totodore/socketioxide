use std::sync::{Arc, RwLock};

use serde_json::Value;

use crate::{
    client::Client,
    packet::{Packet},
};

pub type MessageHandlerCallback = Box<dyn Fn(&Socket, String, serde_json::Value) + Send + Sync + 'static>;

pub struct Socket {
    client: Arc<Client>,
    message_handler: RwLock<Option<MessageHandlerCallback>>,
    pub ns: String,
    pub sid: i64,
}

impl Socket {
    pub fn new(client: Arc<Client>, ns: String, sid: i64) -> Self {
        Self {
            client,
            message_handler: RwLock::new(None),
            ns,
            sid,
        }
    }

    pub fn on_message(&self, callback: impl Fn(&Socket, String, serde_json::Value) + Send + Sync + 'static) {
        self.message_handler.write().unwrap().replace(Box::new(callback));
    }


    //TODO: make this async
    pub async fn emit(&self, event: impl Into<String>, data: impl Into<Value>) {
        let client = self.client.clone();
        let sid = self.sid;
        let ns = self.ns.clone();
        client
            .emit(sid, Packet::event(ns, event.into(), data.into()))
            .await
            .unwrap();
    }

    pub(crate) fn recv_event(self: Arc<Self>, e: String, data: Value) {
        self.message_handler.read().unwrap().as_ref().map(|f| f(&self, e, data));
    }
}
