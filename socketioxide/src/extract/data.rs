use std::convert::Infallible;
use std::sync::Arc;

use crate::handler::{FromConnectParts, FromMessage, FromMessageParts};
use crate::{adapter::Adapter, socket::Socket};
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
impl<T, A> FromConnectParts<A> for Data<T>
where
    T: DeserializeOwned,
    A: Adapter,
{
    type Error = serde_json::Error;
    fn from_connect_parts(
        _: &Arc<Socket<A>>,
        auth: &Option<String>,
        _: &matchit::Params<'_, '_>,
    ) -> Result<Self, Self::Error> {
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
        _: &mut Vec<Bytes>,
        _: &Option<i64>,
    ) -> Result<Self, Self::Error> {
        upwrap_array(v);
        serde_json::from_value(v.clone()).map(Data)
    }
}

/// An Extractor that returns the deserialized data related to the event.
pub struct TryData<T>(pub Result<T, serde_json::Error>);

impl<T, A> FromConnectParts<A> for TryData<T>
where
    T: DeserializeOwned,
    A: Adapter,
{
    type Error = Infallible;
    fn from_connect_parts(
        _: &Arc<Socket<A>>,
        auth: &Option<String>,
        _: &matchit::Params<'_, '_>,
    ) -> Result<Self, Infallible> {
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
impl<A: Adapter> FromMessage<A> for Bin {
    type Error = Infallible;
    fn from_message(
        _: Arc<Socket<A>>,
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
