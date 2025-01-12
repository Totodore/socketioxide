use std::convert::Infallible;
use std::sync::Arc;

use crate::handler::{FromConnectParts, FromMessageParts};
use crate::{adapter::Adapter, socket::Socket};
use serde::de::DeserializeOwned;
use socketioxide_core::parser::{Parse, ParserError};
use socketioxide_core::Value;

/// An Extractor that returns the deserialized data without checking errors.
/// If a deserialization error occurs, the handler won't be called
/// and an error log will be printed if the `tracing` feature is enabled.
///
/// ## Deserializing to a generic type (e.g. `Data<serde_json::Value>`).
/// Deserialization to a generic type is possible (note that it will be less performant/memory efficient).
/// However if you have binary data in you message. It is recommended to use [`rmpv::Value`]
/// rather than [`serde_json::Value`]. This is because [`rmpv::Value`] has a binary data variant contrary
/// to [`serde_json::Value`]. If you still use [`serde_json::Value`] with binary data,
/// it will be converted to a sequence of numbers which is less efficient than a binary blob.
///
/// Alternatively if your binary fields are at the top level of your object you can deserialize to a type
/// with a dynamic type in it: `(serde_json::Value, Bytes, Bytes)` or
/// ```
/// struct MyData {
///     data: serde_json::Value,
///     binary1: bytes::Bytes,
///     binary2: bytes::Bytes,
/// }
/// ```
///
/// [`rmpv::Value`]: https://docs.rs/rmpv
/// [`serde_json::Value`]: https://docs.rs/serde_json/latest/serde_json/value/
pub struct Data<T>(pub T);
impl<T, A> FromConnectParts<A> for Data<T>
where
    T: DeserializeOwned,
    A: Adapter,
{
    type Error = ParserError;
    fn from_connect_parts(s: &Arc<Socket<A>>, auth: &Option<Value>) -> Result<Self, Self::Error> {
        s.parser.decode_default(auth.as_ref()).map(Data)
    }
}

impl<T, A> FromMessageParts<A> for Data<T>
where
    T: DeserializeOwned,
    A: Adapter,
{
    type Error = ParserError;
    fn from_message_parts(
        s: &Arc<Socket<A>>,
        v: &mut Value,
        _: &Option<i64>,
    ) -> Result<Self, Self::Error> {
        s.parser.decode_value(v, true).map(Data)
    }
}

/// An Extractor that returns the deserialized incoming data or a deserialization error.
///
/// ## Deserializing to a generic type (e.g. `Data<serde_json::Value>`).
/// Deserialization to a generic type is possible (note that it will be less performant/memory efficient).
/// However if you have binary data in you message. It is recommended to use [`rmpv::Value`]
/// rather than [`serde_json::Value`]. This is because [`rmpv::Value`] has a binary data variant contrary
/// to [`serde_json::Value`]. If you still use [`serde_json::Value`] with binary data,
/// it will be converted to a sequence of numbers which is less efficient than a binary blob.
///
/// Alternatively if your binary fields are at the top level of your object you can deserialize to a type
/// with a dynamic type in it: `(serde_json::Value, Bytes, Bytes)` or
/// ```
/// struct MyData {
///     data: serde_json::Value,
///     binary1: bytes::Bytes,
///     binary2: bytes::Bytes,
/// }
/// ```
///
/// [`rmpv::Value`]: https://docs.rs/rmpv
/// [`serde_json::Value`]: https://docs.rs/serde_json/latest/serde_json/value/
pub struct TryData<T>(pub Result<T, ParserError>);

impl<T, A> FromConnectParts<A> for TryData<T>
where
    T: DeserializeOwned,
    A: Adapter,
{
    type Error = Infallible;
    fn from_connect_parts(s: &Arc<Socket<A>>, auth: &Option<Value>) -> Result<Self, Infallible> {
        Ok(TryData(s.parser.decode_default(auth.as_ref())))
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
        _: &Option<i64>,
    ) -> Result<Self, Infallible> {
        Ok(TryData(s.parser.decode_value(v, true)))
    }
}

super::__impl_deref!(TryData<T>: Result<T, ParserError>);
super::__impl_deref!(Data);
