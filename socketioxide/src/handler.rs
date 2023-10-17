use std::sync::Arc;

use futures::future::BoxFuture;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;

use crate::errors::AckSenderError;
use crate::{adapter::Adapter, errors::Error, packet::Packet, Socket};

pub type AckResponse<T> = (T, Vec<Vec<u8>>);

pub(crate) type BoxedMessageHandler<A> = Box<dyn MessageCaller<A>>;
pub(crate) type BoxedNamespaceHandler<A> = Box<dyn NamespaceCaller<A>>;
pub(crate) trait MessageCaller<A: Adapter>: Send + Sync + 'static {
    fn call(
        &self,
        s: Arc<Socket<A>>,
        v: Value,
        p: Vec<Vec<u8>>,
        ack_id: Option<i64>,
    ) -> Result<(), Error>;
}

pub(crate) trait NamespaceCaller<A: Adapter>: Send + Sync + 'static {
    fn call(&self, s: Arc<Socket<A>>, auth: Option<String>) -> Result<(), serde_json::Error>;
}

pub(crate) struct CallbackHandler<Param, F, A>
where
    Param: Send + Sync + 'static,
{
    param: std::marker::PhantomData<Param>,
    adapter: std::marker::PhantomData<A>,
    handler: F,
}

impl<Param, F, A> CallbackHandler<Param, F, A>
where
    Param: DeserializeOwned + Send + Sync + 'static,
    F: Fn(Arc<Socket<A>>, Param, Vec<Vec<u8>>, AckSender<A>) -> BoxFuture<'static, ()>
        + Send
        + Sync
        + 'static,
    A: Adapter,
{
    pub fn boxed_message_handler(handler: F) -> Box<Self> {
        Box::new(Self {
            param: std::marker::PhantomData,
            adapter: std::marker::PhantomData,
            handler,
        })
    }
}

impl<Param, F, A> CallbackHandler<Param, F, A>
where
    Param: DeserializeOwned + Send + Sync + 'static,
    F: Fn(Arc<Socket<A>>, Param) -> BoxFuture<'static, ()> + Send + Sync + 'static,
    A: Adapter,
{
    pub fn boxed_ns_handler(handler: F) -> Box<Self> {
        Box::new(Self {
            param: std::marker::PhantomData,
            adapter: std::marker::PhantomData,
            handler,
        })
    }
}

impl<Param, F, A> MessageCaller<A> for CallbackHandler<Param, F, A>
where
    Param: DeserializeOwned + Send + Sync + 'static,
    F: Fn(Arc<Socket<A>>, Param, Vec<Vec<u8>>, AckSender<A>) -> BoxFuture<'static, ()>
        + Send
        + Sync
        + 'static,
    A: Adapter,
{
    fn call(
        &self,
        s: Arc<Socket<A>>,
        v: Value,
        p: Vec<Vec<u8>>,
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
        let fut = (self.handler)(s, v, p, AckSender::new(owned_socket, ack_id));
        tokio::spawn(fut);
        Ok(())
    }
}

impl<Param, F, A> NamespaceCaller<A> for CallbackHandler<Param, F, A>
where
    Param: DeserializeOwned + Send + Sync + 'static,
    F: Fn(Arc<Socket<A>>, Param) -> BoxFuture<'static, ()> + Send + Sync + 'static,
    A: Adapter,
{
    fn call(&self, s: Arc<Socket<A>>, auth: Option<String>) -> Result<(), serde_json::Error> {
        let v: Param = serde_json::from_str(&auth.unwrap_or("{}".to_string()))?;
        let fut = (self.handler)(s, v);
        tokio::spawn(fut);
        Ok(())
    }
}

/// AckSender is used to send an ack response to the client.
/// If the client did not request an ack, it will not send anything.
#[derive(Debug)]
pub struct AckSender<A: Adapter> {
    binary: Vec<Vec<u8>>,
    socket: Arc<Socket<A>>,
    ack_id: Option<i64>,
}
impl<A: Adapter> AckSender<A> {
    pub(crate) fn new(socket: Arc<Socket<A>>, ack_id: Option<i64>) -> Self {
        Self {
            binary: vec![],
            socket,
            ack_id,
        }
    }

    /// Add binary data to the ack response.
    pub fn bin(mut self, bin: Vec<Vec<u8>>) -> Self {
        self.binary = bin;
        self
    }

    /// Send the ack response to the client.
    pub fn send(self, data: impl Serialize) -> Result<(), AckSenderError<A>> {
        if let Some(ack_id) = self.ack_id {
            let ns = self.socket.ns().clone();
            let data = match serde_json::to_value(&data) {
                Err(err) => {
                    return Err(AckSenderError::SendError {
                        send_error: err.into(),
                        socket: self.socket,
                    })
                }
                Ok(data) => data,
            };

            let packet = if self.binary.is_empty() {
                Packet::ack(ns, data, ack_id)
            } else {
                Packet::bin_ack(ns, data, self.binary, ack_id)
            };
            self.socket
                .send(packet)
                .map_err(|err| AckSenderError::SendError {
                    send_error: err,
                    socket: self.socket,
                })
        } else {
            Ok(())
        }
    }
}
