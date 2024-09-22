use std::convert::Infallible;
use std::sync::Arc;

use crate::handler::{FromConnectParts, FromMessageParts};
use crate::parser::DecodeError;
use crate::{adapter::Adapter, socket::Socket};
use bytes::Bytes;
use serde::de::DeserializeOwned;
use socketioxide_core::parser::Parse;
use socketioxide_core::Value;

/// An Extractor that returns the deserialized data without checking errors.
/// If a deserialization error occurs, the handler won't be called
/// and an error log will be print if the `tracing` feature is enabled.
pub struct Data<T>(pub T);
impl<T, A> FromConnectParts<A> for Data<T>
where
    T: DeserializeOwned,
    A: Adapter,
{
    type Error = DecodeError;
    fn from_connect_parts(s: &Arc<Socket<A>>, auth: &Option<Value>) -> Result<Self, Self::Error> {
        let parser = s.parser();
        parser.decode_default(auth.as_ref()).map(Data)
    }
}

impl<T, A> FromMessageParts<A> for Data<T>
where
    T: DeserializeOwned,
    A: Adapter,
{
    type Error = DecodeError;
    fn from_message_parts(
        s: &Arc<Socket<A>>,
        v: &mut Value,
        _: &mut Vec<Bytes>,
        _: &Option<i64>,
    ) -> Result<Self, Self::Error> {
        let parser = s.parser();
        parser.decode_value(v, true).map(Data)
    }
}

/// An Extractor that returns the deserialized data related to the event.
pub struct TryData<T>(pub Result<T, DecodeError>);

impl<T, A> FromConnectParts<A> for TryData<T>
where
    T: DeserializeOwned,
    A: Adapter,
{
    type Error = Infallible;
    fn from_connect_parts(s: &Arc<Socket<A>>, auth: &Option<Value>) -> Result<Self, Infallible> {
        let parser = s.parser();
        Ok(TryData(parser.decode_default(auth.as_ref())))
    }
}
impl<T, A> FromMessageParts<A> for TryData<T>
where
    T: DeserializeOwned,
    A: Adapter,
{
    type Error = Infallible;
    fn from_message_parts(
        s: &Arc<Socket<A>>,
        v: &mut Value,
        _: &mut Vec<Bytes>,
        _: &Option<i64>,
    ) -> Result<Self, Infallible> {
        let parser = s.parser();
        Ok(TryData(parser.decode_value(v, true)))
    }
}

super::__impl_deref!(TryData<T>: Result<T, DecodeError>);
super::__impl_deref!(Data);
