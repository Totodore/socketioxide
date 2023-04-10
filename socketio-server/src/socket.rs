use std::{
    collections::HashMap,
    pin::Pin,
    sync::{Arc, RwLock},
};

use futures::Future;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;

use crate::{client::Client, errors::Error, handshake::Handshake, packet::Packet};

trait MessageCaller: Send + Sync + 'static {
    fn call(&self, s: Arc<Socket>, v: Value) -> Result<(), Error>;
}

trait AckMessageCaller: Send + Sync + 'static {
    fn call(
        &self,
        s: Arc<Socket>,
        v: Value,
        ack: Box<
            dyn FnOnce() -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + Sync + 'static>>
                + Send
                + Sync
                + 'static,
        >,
    ) -> Result<(), Error>;
}
struct MessageHandler<Param, F>
where
    Param: Send + Sync + 'static,
{
    param: std::marker::PhantomData<Param>,
    handler: F,
}

impl<Param, F> MessageCaller for MessageHandler<Param, F>
where
    Param: DeserializeOwned + Send + Sync + 'static,
    F: Fn(Arc<Socket>, Param) -> Pin<Box<dyn Future<Output = ()> + Send + Sync + 'static>>
        + Send
        + Sync
        + 'static,
{
    fn call(&self, s: Arc<Socket>, v: Value) -> Result<(), Error> {
        // Unwrap array if it has only one element
        let v = match v {
            Value::Array(v) => {
                if v.len() == 1 {
                    v.into_iter().next().unwrap_or(Value::Null)
                } else {
                    Value::Array(v)
                }
            }
            v => v,
        };
        let v: Param = serde_json::from_value(v)?;
        tokio::spawn((self.handler)(s, v));
        Ok(())
    }
}

impl<Param, F> AckMessageCaller for MessageHandler<Param, F>
where
    Param: DeserializeOwned + Send + Sync + 'static,
    F: Fn(
            Arc<Socket>,
            Param,
            Box<
                dyn FnOnce() -> Pin<
                        Box<dyn Future<Output = Result<(), Error>> + Send + Sync + 'static>,
                    > + Send
                    + Sync
                    + 'static,
            >,
        ) -> Pin<Box<dyn Future<Output = ()> + Send + Sync + 'static>>
        + Send
        + Sync
        + 'static,
{
    fn call(
        &self,
        s: Arc<Socket>,
        v: Value,
        ack: Box<
            dyn FnOnce() -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + Sync + 'static>>
                + Send
                + Sync
                + 'static,
        >,
    ) -> Result<(), Error> {
        // Unwrap array if it has only one element
        let v = match v {
            Value::Array(v) => {
                if v.len() == 1 {
                    v.into_iter().next().unwrap_or(Value::Null)
                } else {
                    Value::Array(v)
                }
            }
            v => v,
        };
        let v: Param = serde_json::from_value(v)?;
        tokio::spawn((self.handler)(s, v, ack));
        Ok(())
    }
}

pub struct Socket {
    client: Arc<Client>,
    message_handlers: RwLock<HashMap<String, Box<dyn MessageCaller>>>,
    ack_message_handlers: RwLock<HashMap<String, Box<dyn AckMessageCaller>>>,
    pub handshake: Handshake,
    pub ns: String,
    pub sid: i64,
}

impl Socket {
    pub fn new(client: Arc<Client>, handshake: Handshake, ns: String, sid: i64) -> Self {
        Self {
            client,
            message_handlers: RwLock::new(HashMap::new()),
            ack_message_handlers: RwLock::new(HashMap::new()),
            handshake,
            ns,
            sid,
        }
    }

    pub fn on_event<C, F, V>(&self, event: impl Into<String>, callback: C)
    where
        C: Fn(Arc<Socket>, V) -> F + Send + Sync + 'static,
        F: Future<Output = ()> + Send + Sync + 'static,
        V: DeserializeOwned + Send + Sync + 'static,
    {
        let handler = Box::new(move |s, v| Box::pin(callback(s, v)) as _);
        self.message_handlers.write().unwrap().insert(
            event.into(),
            Box::new(MessageHandler {
                param: std::marker::PhantomData,
                handler,
            }),
        );
    }

    pub fn on_event_with_ack<C, F, V>(&self, event: impl Into<String>, callback: C)
    where
        C: Fn(
                Arc<Socket>,
                V,
                Box<
                    dyn FnOnce() -> Pin<
                            Box<dyn Future<Output = Result<(), Error>> + Send + Sync + 'static>,
                        > + Send
                        + Sync
                        + 'static,
                >,
            ) -> F
            + Send
            + Sync
            + 'static,
        F: Future<Output = ()> + Send + Sync + 'static,
        V: DeserializeOwned + Send + Sync + 'static,
    {
        let handler = Box::new(move |s, v, ack| Box::pin(callback(s, v, ack)) as _);
        self.ack_message_handlers.write().unwrap().insert(
            event.into(),
            Box::new(MessageHandler {
                param: std::marker::PhantomData,
                handler,
            }),
        );
    }

    pub async fn emit(&self, event: impl Into<String>, data: impl Serialize) -> Result<(), Error> {
        let client = self.client.clone();
        let sid = self.sid;
        let ns = self.ns.clone();
        let data = serde_json::to_value(data)?;
        client
            .emit(sid, Packet::<()>::event(ns, event.into(), data))
            .await
    }

    pub(crate) fn recv_event(self: Arc<Self>, e: String, data: Value) -> Result<(), Error> {
        if let Some(handler) = self.message_handlers.read().unwrap().get(&e) {
            handler.call(self.clone(), data)?;
        }
        Ok(())
    }

    pub(crate) fn recv_event_ack(
        self: Arc<Self>,
        e: String,
        data: Value,
        ack: i64,
    ) -> Result<(), Error> {
        if let Some(handler) = self.ack_message_handlers.read().unwrap().get(&e) {
            let sid = self.sid;
            let ns = self.ns.clone();
            let client = self.client.clone();
            let cb = Box::new(move || {
                Box::pin(async move {
                    client
                        .emit(sid, Packet::<()>::ack(ns, e, Value::Null, ack))
                        .await
                }) as _
            });
            handler.call(self.clone(), data, cb)?;
        }
        Ok(())
    }
}
