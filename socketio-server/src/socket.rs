use std::{
    pin::Pin,
    sync::{Arc, RwLock},
};

use futures::Future;
use serde::Serialize;
use serde_json::Value;
use tower::BoxError;

use crate::{client::Client, packet::Packet};

pub type MessageHandlerCallback<'a> = Box<
    dyn Fn(
            Arc<Socket>,
            String,
            serde_json::Value,
        ) -> Pin<Box<dyn Future<Output = Result<(), BoxError>> + Send + Sync + 'static>>
        + Send
        + Sync
        + 'static,
>;
pub struct Socket {
    client: Arc<Client>,
    message_handler: RwLock<Option<MessageHandlerCallback<'static>>>,
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

    pub fn on_message<C, F>(&self, callback: C)
    where
        C: Fn(Arc<Socket>, String, serde_json::Value) -> F + Send + Sync + 'static,
        F: Future<Output = Result<(), BoxError>> + Send + Sync + 'static,
    {
        let handler = Box::new(move |s, e, v| Box::pin(callback(s, e, v)) as _);
        self.message_handler.write().unwrap().replace(handler);
    }

    //TODO: make this async
    pub async fn emit(&self, event: impl Into<String>, data: impl Serialize) {
        let client = self.client.clone();
        let sid = self.sid;
        let ns = self.ns.clone();
        client
            .emit(sid, Packet::event(ns, event.into(), data))
            .await
            .unwrap();
    }

    pub(crate) fn recv_event(self: Arc<Self>, e: String, data: Value) {
        if let Some(handler) = self.message_handler.read().unwrap().as_ref() {
            tokio::spawn(handler(self.clone(), e, data));
        }
    }
}
