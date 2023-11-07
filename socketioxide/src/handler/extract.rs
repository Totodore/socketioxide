//! Extractors for handlers:
//! * [`ConnectHandler`](super::ConnectHandler)
//! * [`MessageHandler`](super::MessageHandler)
//!
//! They can be used to extract data from the context of the handler and get specific params. Here are some examples of extractors:
//! * [`Data`](Data): extracts and deserialize to json any data, if a deserialize error occurs the handler won't be called
//!     - for [`ConnectHandler`](super::ConnectHandler): extracts and deserialize to json the auth data
//!     - for [`MessageHandler`](super::MessageHandler): extracts and deserialize to json the message data
//! * [`TryData`](TryData): extracts and deserialize to json any data but with a `Result` type in case of error
//!     - for [`ConnectHandler`](super::ConnectHandler): extracts and deserialize to json the auth data
//!     - for [`MessageHandler`](super::MessageHandler): extracts and deserialize to json the message data
//! * [`SocketRef`](SocketRef): extracts a reference to the [`Socket`]

use std::convert::Infallible;
use std::sync::Arc;

use super::message::FromMessageParts;
use super::{connect::FromConnectParts, message::FromMessage};
use crate::{
    adapter::{Adapter, LocalAdapter},
    packet::Packet,
    socket::Socket,
    AckSenderError, SendError,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;

/// Utility function to unwrap an array with a single element
fn upwrap_array(v: &mut Value) {
    match v {
        Value::Array(vec) if vec.len() == 1 => {
            *v = vec.pop().unwrap();
        }
        _ => (),
    }
}

/// An Extractor that returns the serialized auth data without checking errors
///
/// If a deserialize error occurs, the [`ConnectHandler`](super::ConnectHandler) won't be called
/// and an error log will be print if the `tracing` feature is enabled
pub struct Data<T: DeserializeOwned>(pub T);
impl<T, A> FromConnectParts<A> for Data<T>
where
    T: DeserializeOwned,
    A: Adapter,
{
    type Error = serde_json::Error;
    fn from_connect_parts(_: &Arc<Socket<A>>, auth: &Option<String>) -> Result<Self, Self::Error> {
        auth.as_ref()
            .map(|a| serde_json::from_str::<T>(a))
            .unwrap_or(serde_json::from_str::<T>("{}"))
            .map(Data)
    }
}
impl<T, A> FromMessageParts<A> for Data<T>
where
    T: DeserializeOwned,
    A: Adapter,
{
    type Error = serde_json::Error;
    fn from_message_parts(
        _: &Arc<Socket<A>>,
        v: &mut serde_json::Value,
        _: &mut Vec<Vec<u8>>,
        _: &Option<i64>,
    ) -> Result<Self, Self::Error> {
        upwrap_array(v);
        serde_json::from_value(v.clone()).map(Data)
    }
}

/// An Extractor that returns the serialized auth data
pub struct TryData<T: DeserializeOwned>(pub Result<T, serde_json::Error>);

impl<T, A> FromConnectParts<A> for TryData<T>
where
    T: DeserializeOwned,
    A: Adapter,
{
    type Error = Infallible;
    fn from_connect_parts(_: &Arc<Socket<A>>, auth: &Option<String>) -> Result<Self, Infallible> {
        let v: Result<T, serde_json::Error> = auth
            .as_ref()
            .map(|a| serde_json::from_str(a))
            .unwrap_or(serde_json::from_str("{}"));
        Ok(TryData(v))
    }
}
impl<T, A> FromMessageParts<A> for TryData<T>
where
    T: DeserializeOwned,
    A: Adapter,
{
    type Error = Infallible;
    fn from_message_parts(
        _: &Arc<Socket<A>>,
        v: &mut serde_json::Value,
        _: &mut Vec<Vec<u8>>,
        _: &Option<i64>,
    ) -> Result<Self, Infallible> {
        upwrap_array(v);
        Ok(TryData(serde_json::from_value(v.clone())))
    }
}
/// An Extractor that returns a reference to a [`Socket`]
pub struct SocketRef<A: Adapter = LocalAdapter>(Arc<Socket<A>>);

impl<A: Adapter> FromConnectParts<A> for SocketRef<A> {
    type Error = Infallible;
    fn from_connect_parts(s: &Arc<Socket<A>>, _: &Option<String>) -> Result<Self, Infallible> {
        Ok(SocketRef(s.clone()))
    }
}
impl<A: Adapter> FromMessageParts<A> for SocketRef<A> {
    type Error = Infallible;
    fn from_message_parts(
        s: &Arc<Socket<A>>,
        _: &mut serde_json::Value,
        _: &mut Vec<Vec<u8>>,
        _: &Option<i64>,
    ) -> Result<Self, Infallible> {
        Ok(SocketRef(s.clone()))
    }
}

impl<A: Adapter> std::ops::Deref for SocketRef<A> {
    type Target = Socket<A>;
    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<A: Adapter> SocketRef<A> {
    #[inline(always)]
    pub(crate) fn new(socket: Arc<Socket<A>>) -> Self {
        Self(socket)
    }

    /// Disconnect the socket from the current namespace,
    ///
    /// It will also call the disconnect handler if it is set.
    #[inline(always)]
    pub fn disconnect(self) -> Result<(), SendError> {
        self.0.disconnect()
    }
}

/// An Extractor that returns the binary data of the message
/// if there is no binary data, it will return an empty vector
pub struct Bin(pub Vec<Vec<u8>>);
impl<A: Adapter> FromMessage<A> for Bin {
    type Error = Infallible;
    fn from_message(
        _: Arc<Socket<A>>,
        _: serde_json::Value,
        bin: Vec<Vec<u8>>,
        _: Option<i64>,
    ) -> Result<Self, Infallible> {
        Ok(Bin(bin))
    }
}

/// An Extractor to send an ack response corresponding to the current event
///
/// If the client did not request an ack, it will not send anything.
#[derive(Debug)]
pub struct AckSender<A: Adapter = LocalAdapter> {
    binary: Vec<Vec<u8>>,
    socket: Arc<Socket<A>>,
    ack_id: Option<i64>,
}
impl<A: Adapter> FromMessageParts<A> for AckSender<A> {
    type Error = Infallible;
    fn from_message_parts(
        s: &Arc<Socket<A>>,
        _: &mut serde_json::Value,
        _: &mut Vec<Vec<u8>>,
        ack_id: &Option<i64>,
    ) -> Result<Self, Infallible> {
        Ok(Self::new(s.clone(), *ack_id))
    }
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
