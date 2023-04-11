use std::{
    collections::HashMap,
    pin::Pin,
    sync::{
        atomic::{AtomicI64, Ordering},
        Arc, RwLock,
    },
    time::Duration,
};

use futures::{Future, TryFutureExt};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use tokio::sync::oneshot;

use crate::{
    client::Client,
    errors::{AckError, Error},
    handshake::Handshake,
    packet::{BinaryPacket, Packet, PacketData},
};

type BoxAsyncFut<T> = Pin<Box<dyn Future<Output = T> + Send + Sync + 'static>>;

//TODO: Define a trait to handle all types of data (serialized, binary, etc.)
trait MessageCaller: Send + Sync + 'static {
    fn call(
        &self,
        s: Arc<Socket>,
        v: Value,
        p: Option<Vec<Vec<u8>>>,
        ack_id: Option<i64>,
    ) -> Result<(), Error>;
}

struct MessageHandler<Param, ParamCb, F>
where
    Param: Send + Sync + 'static,
    ParamCb: Send + Sync + 'static,
{
    param: std::marker::PhantomData<Param>,
    param_cb: std::marker::PhantomData<ParamCb>,
    handler: F,
}

impl<Param, RetV, F> MessageCaller for MessageHandler<Param, RetV, F>
where
    Param: DeserializeOwned + Send + Sync + 'static,
    RetV: Serialize + Send + Sync + 'static,
    F: Fn(Arc<Socket>, Param, Option<Vec<Vec<u8>>>) -> BoxAsyncFut<Result<RetV, Error>>
        + Send
        + Sync
        + 'static,
{
    fn call(
        &self,
        s: Arc<Socket>,
        v: Value,
        p: Option<Vec<Vec<u8>>>,
        ack_id: Option<i64>,
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
        let owned_socket = s.clone();
        let fut = (self.handler)(s, v, p);
        if let Some(ack_id) = ack_id {
            // Send ack for the message if it has an ack_id
            tokio::spawn(
                fut.and_then(move |val| async move { owned_socket.send_ack(ack_id, val).await }),
            );
        } else {
            tokio::spawn(fut);
        }
        Ok(())
    }
}

pub struct Socket {
    client: Arc<Client>,
    message_handlers: RwLock<HashMap<String, Box<dyn MessageCaller>>>,
    ack_message: RwLock<HashMap<i64, oneshot::Sender<(Value, Option<Vec<Vec<u8>>>)>>>,
    ack_counter: AtomicI64,
    pub handshake: Handshake,
    pub ns: String,
    pub sid: i64,
}

impl Socket {
    pub fn new(client: Arc<Client>, handshake: Handshake, ns: String, sid: i64) -> Self {
        Self {
            client,
            message_handlers: RwLock::new(HashMap::new()),
            ack_message: RwLock::new(HashMap::new()),
            ack_counter: AtomicI64::new(0),
            handshake,
            ns,
            sid,
        }
    }

    pub fn on_event<C, F, V, RetV>(&self, event: impl Into<String>, callback: C)
    where
        C: Fn(Arc<Socket>, V, Option<Vec<Vec<u8>>>) -> F + Send + Sync + 'static,
        F: Future<Output = Result<RetV, Error>> + Send + Sync + 'static,
        V: DeserializeOwned + Send + Sync + 'static,
        RetV: Serialize + Send + Sync + 'static,
    {
        let handler = Box::new(move |s, v, p| Box::pin(callback(s, v, p)) as _);
        self.message_handlers.write().unwrap().insert(
            event.into(),
            Box::new(MessageHandler {
                param: std::marker::PhantomData,
                param_cb: std::marker::PhantomData,
                handler,
            }),
        );
    }

    pub async fn emit(&self, event: impl Into<String>, data: impl Serialize) -> Result<(), Error> {
        let ns = self.ns.clone();
        let data = serde_json::to_value(data)?;
        self.client
            .emit(self.sid, Packet::<()>::event(ns, event.into(), data, None))
            .await
    }

    pub async fn emit_bin(
        &self,
        event: impl Into<String>,
        data: impl Serialize,
        payload: Vec<Vec<u8>>,
    ) -> Result<(), Error> {
        let ns = self.ns.clone();
        let data = serde_json::to_value(data)?;
        self.client
            .emit(
                self.sid,
                Packet::<()>::bin_event(ns, event.into(), data, payload, None),
            )
            .await
    }

    pub async fn emit_with_ack_timeout<V>(
        &self,
        event: impl Into<String>,
        data: impl Serialize,
        timeout: Duration,
    ) -> Result<(V, Option<Vec<Vec<u8>>>), AckError>
    where
        V: DeserializeOwned + Send + Sync + 'static,
    {
        let ns = self.ns.clone();
        let data = serde_json::to_value(data)?;
        let (tx, rx) = oneshot::channel();
        let ack = self.ack_counter.fetch_add(1, Ordering::SeqCst) + 1;
        self.ack_message.write().unwrap().insert(ack, tx);
        let packet = Packet::<()>::event(ns, event.into(), data, Some(ack));
        self.client.emit(self.sid, packet).await?;
        let v = tokio::time::timeout(timeout, rx).await??;
        Ok((serde_json::from_value(v.0)?, v.1))
    }

    pub async fn emit_with_ack<V>(
        &self,
        event: impl Into<String>,
        data: impl Serialize,
    ) -> Result<(V, Option<Vec<Vec<u8>>>), AckError>
    where
        V: DeserializeOwned + Send + Sync + 'static,
    {
        self.emit_with_ack_timeout(event, data, self.client.config.ack_timeout)
            .await
    }

    pub(crate) async fn send_ack(&self, ack_id: i64, data: impl Serialize) -> Result<(), Error> {
        let ns = self.ns.clone();
        let data = serde_json::to_value(&data)?;
        self.client
            .emit(self.sid, Packet::<()>::ack(ns, data, ack_id))
            .await
    }

    pub(crate) fn recv(self: Arc<Self>, packet: PacketData<Value>) -> Result<(), Error> {
        match packet {
            PacketData::Event(e, data, ack) => self.recv_event(e, data, ack),
            PacketData::EventAck(data, ack_id) => self.recv_ack(data, ack_id),
            PacketData::BinaryEvent(e, packet, ack) => self.recv_bin_event(e, packet, ack),
            PacketData::BinaryAck(packet, ack) => self.recv_bin_ack(packet, ack),
            _ => unreachable!(),
        }
    }

    fn recv_event(self: Arc<Self>, e: String, data: Value, ack: Option<i64>) -> Result<(), Error> {
        if let Some(handler) = self.message_handlers.read().unwrap().get(&e) {
            handler.call(self.clone(), data, None, ack)?;
        }
        Ok(())
    }

    fn recv_bin_event(
        self: Arc<Self>,
        e: String,
        packet: BinaryPacket,
        ack: Option<i64>,
    ) -> Result<(), Error> {
        if let Some(handler) = self.message_handlers.read().unwrap().get(&e) {
            handler.call(self.clone(), packet.data, Some(packet.bin), ack)?;
        }
        Ok(())
    }

    pub fn recv_ack(self: Arc<Self>, data: Value, ack: i64) -> Result<(), Error> {
        if let Some(tx) = self.ack_message.write().unwrap().remove(&ack) {
            tx.send((data, None)).ok();
        }
        Ok(())
    }

    pub fn recv_bin_ack(self: Arc<Self>, packet: BinaryPacket, ack: i64) -> Result<(), Error> {
        if let Some(tx) = self.ack_message.write().unwrap().remove(&ack) {
            tx.send((packet.data, Some(packet.bin))).ok();
        }
        Ok(())
    }
}
