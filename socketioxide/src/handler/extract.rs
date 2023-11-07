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

use std::sync::Arc;

use super::connect::FromConnectParts;
use crate::{
    adapter::{Adapter, LocalAdapter},
    socket::Socket,
    SendError,
};
use serde::de::DeserializeOwned;

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
    fn from_connect_parts(_: &Arc<Socket<A>>, auth: &Option<String>) -> Result<Self, ()> {
        auth.as_ref()
            .map(|a| serde_json::from_str::<T>(a))
            .unwrap_or(serde_json::from_str::<T>("{}"))
            .map(Data)
            .map_err(|_e| {
                #[cfg(feature = "tracing")]
                tracing::error!("Error deserializing auth data: {}", _e);
            })
    }
}

/// An Extractor that returns the serialized auth data
pub struct TryData<T: DeserializeOwned>(pub Result<T, serde_json::Error>);

impl<T, A> FromConnectParts<A> for TryData<T>
where
    T: DeserializeOwned,
    A: Adapter,
{
    fn from_connect_parts(_: &Arc<Socket<A>>, auth: &Option<String>) -> Result<Self, ()> {
        let v: Result<T, serde_json::Error> = auth
            .as_ref()
            .map(|a| serde_json::from_str(a))
            .unwrap_or(serde_json::from_str("{}"));
        Ok(TryData(v))
    }
}

/// An Extractor that returns a reference to a [`Socket`]
pub struct SocketRef<A: Adapter = LocalAdapter>(Arc<Socket<A>>);

impl<A: Adapter> FromConnectParts<A> for SocketRef<A> {
    fn from_connect_parts(s: &Arc<Socket<A>>, _: &Option<String>) -> Result<Self, ()> {
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
