use std::convert::Infallible;
use std::sync::Arc;

use crate::handler::{FromConnectParts, FromMessage, FromMessageParts};
use crate::socket::Socket;
use bytes::Bytes;
use serde::de::DeserializeOwned;
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

/// An Extractor that returns the serialized auth data without checking errors.
/// If a deserialization error occurs, the [`ConnectHandler`](crate::handler::ConnectHandler) won't be called
/// and an error log will be print if the `tracing` feature is enabled.
pub struct Data<T>(pub T);
impl<T> FromConnectParts for Data<T>
where
    T: DeserializeOwned,
{
    type Error = serde_json::Error;
    fn from_connect_parts(
        _: &Arc<Socket>,
        auth: &Option<String>,
        _: &Arc<state::TypeMap![Send + Sync]>,
    ) -> Result<Self, Self::Error> {
        auth.as_ref()
            .map(|a| serde_json::from_str::<T>(a))
            .unwrap_or(serde_json::from_str::<T>("{}"))
            .map(Data)
    }
}
impl<T> FromMessageParts for Data<T>
where
    T: DeserializeOwned,
{
    type Error = serde_json::Error;
    fn from_message_parts(
        _: &Arc<Socket>,
        v: &mut serde_json::Value,
        _: &mut Vec<Bytes>,
        _: &Option<i64>,
    ) -> Result<Self, Self::Error> {
        upwrap_array(v);
        serde_json::from_value(v.clone()).map(Data)
    }
}

/// An Extractor that returns the deserialized data related to the event.
pub struct TryData<T>(pub Result<T, serde_json::Error>);

impl<T> FromConnectParts for TryData<T>
where
    T: DeserializeOwned,
{
    type Error = Infallible;
    fn from_connect_parts(_: &Arc<Socket>, auth: &Option<String>) -> Result<Self, Infallible> {
        let v: Result<T, serde_json::Error> = auth
            .as_ref()
            .map(|a| serde_json::from_str(a))
            .unwrap_or(serde_json::from_str("{}"));
        Ok(TryData(v))
    }
}
impl<T> FromMessageParts for TryData<T>
where
    T: DeserializeOwned,
{
    type Error = Infallible;
    fn from_message_parts(
        _: &Arc<Socket>,
        v: &mut serde_json::Value,
        _: &mut Vec<Bytes>,
        _: &Option<i64>,
    ) -> Result<Self, Infallible> {
        upwrap_array(v);
        Ok(TryData(serde_json::from_value(v.clone())))
    }
}

/// An Extractor that returns the binary data of the message.
/// If there is no binary data, it will contain an empty vec.
pub struct Bin(pub Vec<Bytes>);
impl FromMessage for Bin {
    type Error = Infallible;
    fn from_message(
        _: Arc<Socket>,
        _: serde_json::Value,
        bin: Vec<Bytes>,
        _: Option<i64>,
    ) -> Result<Self, Infallible> {
        Ok(Bin(bin))
    }
}

super::__impl_deref!(Bin: Vec<Bytes>);
super::__impl_deref!(TryData<T>: Result<T, serde_json::Error>);
super::__impl_deref!(Data);
