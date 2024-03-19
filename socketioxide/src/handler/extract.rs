//! ### Extractors for [`ConnectHandler`](super::ConnectHandler), [`ConnectMiddleware`](super::ConnectMiddleware),
//! [`MessageHandler`](super::MessageHandler)
//! and [`DisconnectHandler`](super::DisconnectHandler).
//!
//! They can be used to extract data from the context of the handler and get specific params. Here are some examples of extractors:
//! * [`Data`]: extracts and deserialize to json any data, if a deserialization error occurs the handler won't be called:
//!     - for [`ConnectHandler`](super::ConnectHandler): extracts and deserialize to json the auth data
//!     - for [`ConnectMiddleware`](super::ConnectMiddleware): extract and deserialize to json the auth data.
//! In case of error, the middleware chain stops and a `connect_error` event is sent.
//!     - for [`MessageHandler`](super::MessageHandler): extracts and deserialize to json the message data
//! * [`TryData`]: extracts and deserialize to json any data but with a `Result` type in case of error:
//!     - for [`ConnectHandler`](super::ConnectHandler) and [`ConnectMiddleware`](super::ConnectMiddleware):
//! extracts and deserialize to json the auth data
//!     - for [`MessageHandler`](super::MessageHandler): extracts and deserialize to json the message data
//! * [`SocketRef`]: extracts a reference to the [`Socket`]
//! * [`Bin`]: extract a binary payload for a given message. Because it consumes the event it should be the last argument
//! * [`AckSender`]: Can be used to send an ack response to the current message event
//! * [`ProtocolVersion`](crate::ProtocolVersion): extracts the protocol version
//! * [`TransportType`](crate::TransportType): extracts the transport type
//! * [`DisconnectReason`]: extracts the reason of the disconnection
//! * [`State`]: extracts a reference to a state previously set with [`SocketIoBuilder::with_state`](crate::io::SocketIoBuilder).
//!
//! ### You can also implement your own Extractor with the [`FromConnectParts`], [`FromMessageParts`] and [`FromDisconnectParts`] traits
//! When implementing these traits, if you clone the [`Arc<Socket>`] make sure that it is dropped at least when the socket is disconnected.
//! Otherwise it will create a memory leak. It is why the [`SocketRef`] extractor is used instead of cloning the socket for common usage.
//!
//! #### Example that extracts a user id from the query params
//! ```rust
//! # use socketioxide::handler::{FromConnectParts, FromMessageParts};
//! # use socketioxide::adapter::Adapter;
//! # use socketioxide::socket::Socket;
//! # use std::sync::Arc;
//! # use std::convert::Infallible;
//! # use socketioxide::SocketIo;
//!
//! struct UserId(String);
//!
//! #[derive(Debug)]
//! struct UserIdNotFound;
//! impl std::fmt::Display for UserIdNotFound {
//!     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//!         write!(f, "User id not found")
//!     }
//! }
//! impl std::error::Error for UserIdNotFound {}
//!
//! impl<A: Adapter> FromConnectParts<A> for UserId {
//!     type Error = Infallible;
//!     fn from_connect_parts(s: &Arc<Socket<A>>, _: &Option<String>) -> Result<Self, Self::Error> {
//!         // In a real app it would be better to parse the query params with a crate like `url`
//!         let uri = &s.req_parts().uri;
//!         let uid = uri
//!             .query()
//!             .and_then(|s| s.split('&').find(|s| s.starts_with("id=")).map(|s| &s[3..]))
//!             .unwrap_or_default();
//!         // Currently, it is not possible to have lifetime on the extracted data
//!         Ok(UserId(uid.to_string()))
//!     }
//! }
//!
//! // Here, if the user id is not found, the handler won't be called
//! // and a tracing `error` log will be printed (if the `tracing` feature is enabled)
//! impl<A: Adapter> FromMessageParts<A> for UserId {
//!     type Error = UserIdNotFound;
//!
//!     fn from_message_parts(
//!         s: &Arc<Socket<A>>,
//!         _: &mut serde_json::Value,
//!         _: &mut Vec<Vec<u8>>,
//!         _: &Option<i64>,
//!     ) -> Result<Self, UserIdNotFound> {
//!         // In a real app it would be better to parse the query params with a crate like `url`
//!         let uri = &s.req_parts().uri;
//!         let uid = uri
//!             .query()
//!             .and_then(|s| s.split('&').find(|s| s.starts_with("id=")).map(|s| &s[3..]))
//!             .ok_or(UserIdNotFound)?;
//!         // Currently, it is not possible to have lifetime on the extracted data
//!         Ok(UserId(uid.to_string()))
//!     }
//! }
//!
//! fn handler(user_id: UserId) {
//!     println!("User id: {}", user_id.0);
//! }
//! let (svc, io) = SocketIo::new_svc();
//! io.ns("/", handler);
//! // Use the service with your favorite http server
use std::convert::Infallible;
use std::sync::Arc;

use super::message::FromMessageParts;
use super::FromDisconnectParts;
use super::{connect::FromConnectParts, message::FromMessage};
use crate::errors::{DisconnectError, SendError};
use crate::socket::DisconnectReason;
use crate::{
    adapter::{Adapter, LocalAdapter},
    packet::Packet,
    socket::Socket,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;

#[cfg(feature = "state")]
#[cfg_attr(docsrs, doc(cfg(feature = "state")))]
pub use state_extract::*;

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
/// If a deserialization error occurs, the [`ConnectHandler`](super::ConnectHandler) won't be called
/// and an error log will be print if the `tracing` feature is enabled.
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

/// An Extractor that returns the deserialized data related to the event.
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
/// An Extractor that returns a reference to a [`Socket`].
#[derive(Debug)]
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
impl<A: Adapter> FromDisconnectParts<A> for SocketRef<A> {
    type Error = Infallible;
    fn from_disconnect_parts(s: &Arc<Socket<A>>, _: DisconnectReason) -> Result<Self, Infallible> {
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
impl<A: Adapter> PartialEq for SocketRef<A> {
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        self.0.id == other.0.id
    }
}
impl<A: Adapter> From<Arc<Socket<A>>> for SocketRef<A> {
    #[inline(always)]
    fn from(socket: Arc<Socket<A>>) -> Self {
        Self(socket)
    }
}

impl<A: Adapter> Clone for SocketRef<A> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<A: Adapter> SocketRef<A> {
    /// Disconnect the socket from the current namespace,
    ///
    /// It will also call the disconnect handler if it is set.
    #[inline(always)]
    pub fn disconnect(self) -> Result<(), DisconnectError> {
        self.0.disconnect()
    }
}

/// An Extractor that returns the binary data of the message.
/// If there is no binary data, it will contain an empty vec.
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

/// An Extractor to send an ack response corresponding to the current event.
/// If the client sent a normal message without expecting an ack, the ack callback will do nothing.
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
    pub fn send<T: Serialize>(self, data: T) -> Result<(), SendError<T>> {
        use crate::socket::PermitExt;
        if let Some(ack_id) = self.ack_id {
            let permit = match self.socket.reserve() {
                Ok(permit) => permit,
                Err(e) => {
                    #[cfg(feature = "tracing")]
                    tracing::debug!("sending error during emit message: {e:?}");
                    return Err(e.with_value(data).into());
                }
            };
            let ns = self.socket.ns();
            let data = serde_json::to_value(data)?;
            let packet = if self.binary.is_empty() {
                Packet::ack(ns, data, ack_id)
            } else {
                Packet::bin_ack(ns, data, self.binary, ack_id)
            };
            permit.send(packet);
            Ok(())
        } else {
            Ok(())
        }
    }
}

impl<A: Adapter> FromConnectParts<A> for crate::ProtocolVersion {
    type Error = Infallible;
    fn from_connect_parts(s: &Arc<Socket<A>>, _: &Option<String>) -> Result<Self, Infallible> {
        Ok(s.protocol())
    }
}
impl<A: Adapter> FromMessageParts<A> for crate::ProtocolVersion {
    type Error = Infallible;
    fn from_message_parts(
        s: &Arc<Socket<A>>,
        _: &mut serde_json::Value,
        _: &mut Vec<Vec<u8>>,
        _: &Option<i64>,
    ) -> Result<Self, Infallible> {
        Ok(s.protocol())
    }
}
impl<A: Adapter> FromDisconnectParts<A> for crate::ProtocolVersion {
    type Error = Infallible;
    fn from_disconnect_parts(s: &Arc<Socket<A>>, _: DisconnectReason) -> Result<Self, Infallible> {
        Ok(s.protocol())
    }
}

impl<A: Adapter> FromConnectParts<A> for crate::TransportType {
    type Error = Infallible;
    fn from_connect_parts(s: &Arc<Socket<A>>, _: &Option<String>) -> Result<Self, Infallible> {
        Ok(s.transport_type())
    }
}
impl<A: Adapter> FromMessageParts<A> for crate::TransportType {
    type Error = Infallible;
    fn from_message_parts(
        s: &Arc<Socket<A>>,
        _: &mut serde_json::Value,
        _: &mut Vec<Vec<u8>>,
        _: &Option<i64>,
    ) -> Result<Self, Infallible> {
        Ok(s.transport_type())
    }
}
impl<A: Adapter> FromDisconnectParts<A> for crate::TransportType {
    type Error = Infallible;
    fn from_disconnect_parts(s: &Arc<Socket<A>>, _: DisconnectReason) -> Result<Self, Infallible> {
        Ok(s.transport_type())
    }
}

impl<A: Adapter> FromDisconnectParts<A> for DisconnectReason {
    type Error = Infallible;
    fn from_disconnect_parts(
        _: &Arc<Socket<A>>,
        reason: DisconnectReason,
    ) -> Result<Self, Infallible> {
        Ok(reason)
    }
}

#[cfg(feature = "state")]
mod state_extract {
    use super::*;
    use crate::state::get_state;

    /// An Extractor that contains a reference to a state previously set with [`SocketIoBuilder::with_state`](crate::io::SocketIoBuilder).
    /// It implements [`std::ops::Deref`] to access the inner type so you can use it as a normal reference.
    ///
    /// The specified state type must be the same as the one set with [`SocketIoBuilder::with_state`](crate::io::SocketIoBuilder).
    /// If it is not the case, the handler won't be called and an error log will be print if the `tracing` feature is enabled.
    ///
    /// The state is shared between the entire socket.io app context.
    ///
    /// ### Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::{SocketRef, State}};
    /// # use serde::{Serialize, Deserialize};
    /// # use std::sync::atomic::AtomicUsize;
    /// # use std::sync::atomic::Ordering;
    /// #[derive(Default)]
    /// struct MyAppData {
    ///     user_cnt: AtomicUsize,
    /// }
    /// impl MyAppData {
    ///     fn add_user(&self) {
    ///         self.user_cnt.fetch_add(1, Ordering::SeqCst);
    ///     }
    ///     fn rm_user(&self) {
    ///         self.user_cnt.fetch_sub(1, Ordering::SeqCst);
    ///     }
    /// }
    /// let (_, io) = SocketIo::builder().with_state(MyAppData::default()).build_svc();
    /// io.ns("/", |socket: SocketRef, state: State<MyAppData>| {
    ///     state.add_user();
    ///     println!("User count: {}", state.user_cnt.load(Ordering::SeqCst));
    /// });
    pub struct State<T: 'static>(pub &'static T);
    /// It was impossible to find the given state and therefore the handler won't be called.
    #[derive(Debug, thiserror::Error)]
    #[error("State not found")]
    pub struct StateNotFound;

    impl<T> std::ops::Deref for State<T> {
        type Target = T;
        fn deref(&self) -> &Self::Target {
            self.0
        }
    }

    impl<A: Adapter, T: Send + Sync + 'static> FromConnectParts<A> for State<T> {
        type Error = StateNotFound;
        fn from_connect_parts(
            _: &Arc<Socket<A>>,
            _: &Option<String>,
        ) -> Result<Self, StateNotFound> {
            get_state::<T>().map(State).ok_or(StateNotFound)
        }
    }
    impl<A: Adapter, T: Send + Sync + 'static> FromDisconnectParts<A> for State<T> {
        type Error = StateNotFound;
        fn from_disconnect_parts(
            _: &Arc<Socket<A>>,
            _: DisconnectReason,
        ) -> Result<Self, StateNotFound> {
            get_state::<T>().map(State).ok_or(StateNotFound)
        }
    }
    impl<A: Adapter, T: Send + Sync + 'static> FromMessageParts<A> for State<T> {
        type Error = StateNotFound;
        fn from_message_parts(
            _: &Arc<Socket<A>>,
            _: &mut serde_json::Value,
            _: &mut Vec<Vec<u8>>,
            _: &Option<i64>,
        ) -> Result<Self, StateNotFound> {
            get_state::<T>().map(State).ok_or(StateNotFound)
        }
    }
}
