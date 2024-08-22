use std::convert::Infallible;
use std::sync::Arc;

use crate::handler::{FromConnectParts, FromMessage, FromMessageParts};
use crate::parser::value::ParseError;
use crate::{adapter::Adapter, parser::value::from_value, socket::Socket, Value};
use bytes::Bytes;
use serde::de::DeserializeOwned;

/// An Extractor that returns the deserialized data without checking errors.
/// If a deserialization error occurs, the handler won't be called
/// and an error log will be print if the `tracing` feature is enabled.
pub struct Data<T>(pub T);
impl<T, A> FromConnectParts<A> for Data<T>
where
    T: DeserializeOwned,
    A: Adapter,
{
    type Error = ParseError;
    fn from_connect_parts(_: &Arc<Socket<A>>, auth: &Option<Value>) -> Result<Self, Self::Error> {
        auth.as_ref()
            .map(|a| from_value(a))
            .unwrap_or(serde_json::from_str::<T>("{}").map_err(Self::Error::from))
            .map(Data)
    }
}

impl<T, A> FromMessageParts<A> for Data<T>
where
    T: DeserializeOwned,
    A: Adapter,
{
    type Error = ParseError;
    fn from_message_parts(
        _: &Arc<Socket<A>>,
        v: &mut Value,
        _: &mut Vec<Bytes>,
        _: &Option<i64>,
    ) -> Result<Self, Self::Error> {
        Ok(Data(from_value(v)?))
    }
}

/// An Extractor that returns the deserialized data related to the event.
pub struct TryData<T>(pub Result<T, ParseError>);

impl<T, A> FromConnectParts<A> for TryData<T>
where
    T: DeserializeOwned,
    A: Adapter,
{
    type Error = Infallible;
    fn from_connect_parts(_: &Arc<Socket<A>>, auth: &Option<Value>) -> Result<Self, Infallible> {
        let v: Result<T, ParseError> = auth
            .as_ref()
            .map(|a| from_value(a)) //TODO: clone
            .unwrap_or(serde_json::from_str("{}").map_err(ParseError::from));
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
        v: &mut Value,
        _: &mut Vec<Bytes>,
        _: &Option<i64>,
    ) -> Result<Self, Infallible> {
        Ok(TryData(from_value(v)))
    }
}

/// An Extractor that returns the binary data of the message.
/// If there is no binary data, it will contain an empty vec.
pub struct Bin(pub Vec<Bytes>);
impl<A: Adapter> FromMessage<A> for Bin {
    type Error = Infallible;
    fn from_message(
        _: Arc<Socket<A>>,
        _: Value,
        bin: Vec<Bytes>,
        _: Option<i64>,
    ) -> Result<Self, Infallible> {
        Ok(Bin(bin))
    }
}

super::__impl_deref!(Bin: Vec<Bytes>);
super::__impl_deref!(TryData<T>: Result<T, ParseError>);
super::__impl_deref!(Data);
