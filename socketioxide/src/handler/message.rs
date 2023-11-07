use std::sync::Arc;

use futures::Future;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;

use super::extract::SocketRef;
use crate::adapter::LocalAdapter;
use crate::errors::AckSenderError;
use crate::socket::Socket;
use crate::{adapter::Adapter, packet::Packet};

use super::MakeErasedHandler;

pub type AckResponse<T> = (T, Vec<Vec<u8>>);

pub(crate) type BoxedMessageHandler<A> = Box<dyn ErasedMessageHandler<A>>;

pub(crate) trait ErasedMessageHandler<A: Adapter>: Send + Sync + 'static {
    fn call(&self, s: Arc<Socket<A>>, v: Value, p: Vec<Vec<u8>>, ack_id: Option<i64>);
}
pub trait MessageHandler<A: Adapter, T>: Send + Sync + 'static {
    fn call(&self, s: Arc<Socket<A>>, v: Value, p: Vec<Vec<u8>>, ack_id: Option<i64>);
    fn phantom(&self) -> std::marker::PhantomData<T> {
        std::marker::PhantomData
    }
}

impl<A, T, H> MakeErasedHandler<H, A, T, ()>
where
    T: Send + Sync + 'static,
    H: MessageHandler<A, T> + Send + Sync + 'static,
    A: Adapter,
{
    pub fn new_message_boxed(inner: H) -> Box<dyn ErasedMessageHandler<A>> {
        Box::new(MakeErasedHandler::new(inner))
    }
}

impl<A, T, H> ErasedMessageHandler<A> for MakeErasedHandler<H, A, T, ()>
where
    T: Send + Sync + 'static,
    H: MessageHandler<A, T> + Send + Sync + 'static,
    A: Adapter,
{
    #[inline(always)]
    fn call(&self, s: Arc<Socket<A>>, v: Value, p: Vec<Vec<u8>>, ack_id: Option<i64>) {
        self.handler.call(s, v, p, ack_id);
    }
}

impl<F, A, T, Fut> MessageHandler<A, (T,)> for F
where
    T: DeserializeOwned + Send + Sync + 'static,
    F: Fn(SocketRef<A>, T, Vec<Vec<u8>>, AckSender<A>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
    A: Adapter,
{
    fn call(&self, s: Arc<Socket<A>>, v: Value, p: Vec<Vec<u8>>, ack_id: Option<i64>) {
        // Unwrap array if it has only one element
        let v = upwrap_array(v);
        let v: T = serde_json::from_value(v).unwrap();
        let owned_socket = s.clone();
        let s = SocketRef::new(s);
        let fut = self(s, v, p, AckSender::new(owned_socket, ack_id));
        tokio::spawn(fut);
    }
}

fn upwrap_array(v: Value) -> Value {
    match v {
        Value::Array(v) => {
            if v.len() == 1 {
                v.into_iter().next().unwrap_or(Value::Null)
            } else {
                Value::Array(v)
            }
        }
        v => v,
    }
}

/// AckSender is used to send an ack response to the client.
/// If the client did not request an ack, it will not send anything.
#[derive(Debug)]
pub struct AckSender<A: Adapter = LocalAdapter> {
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
            let ns = self.socket.ns();
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
