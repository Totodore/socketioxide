//! [`MessageHandler`] trait and implementations, used to handle the message events.
//! It has a flexible axum-like API, you can put any arguments as long as it implements
//! the [`FromMessageParts`] trait or the [`FromMessage`] trait for the last argument.
//!
//! All the types that implement [`FromMessageParts`] also implement [`FromMessage`].
//!
//! You can also implement the [`FromMessageParts`] and [`FromMessage`] traits for your own types.
//! See the [`extract`](crate::extract) module doc for more details on available extractors.
//!
//! Handlers _must_ be async.
//!
//! ## Example with async closures
//! ```rust
//! # use socketioxide::SocketIo;
//! # use socketioxide::extract::*;
//! let (svc, io) = SocketIo::new_svc();
//! io.ns("/", async |s: SocketRef| {
//!     s.on("event", async |s: SocketRef, Data::<String>(data)| {
//!        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
//!        println!("Socket received event with data: {}", data);
//!     });
//!     // `Bin` extractor must be the last argument because it consumes the rest of the packet
//!     s.on("/binary_event", async |s: SocketRef, TryData::<String>(data)| {
//!       println!("Socket received event with data: {:?}", data);
//!     })
//! });
//! ```
//!
//! ## Example with an async non-anonymous handler
//! ```rust
//! # use socketioxide::SocketIo;
//! # use socketioxide::extract::*;
//! // async named event handler
//! async fn on_event(s: SocketRef, Data(data): Data<serde_json::Value>, ack: AckSender) {
//!     tokio::time::sleep(std::time::Duration::from_secs(1)).await;
//!     ack.send("Here is my acknowledgment!").ok();
//! }
//! let (svc, io) = SocketIo::new_svc();
//! io.ns("/", async |s: SocketRef| {
//!     s.on("event", on_event);
//!     // It is also possible to reuse handlers, like this:
//!     s.on("event_2", on_event);
//! });
//! ```
use std::future::Future;
use std::sync::Arc;

use socketioxide_core::Value;

use crate::socket::Socket;

use super::MakeErasedHandler;

/// A Type Erased [`MessageHandler`] so it can be stored in a HashMap
pub(crate) type BoxedMessageHandler = Box<dyn ErasedMessageHandler>;

pub(crate) trait ErasedMessageHandler: Send + Sync + 'static {
    fn call(&self, s: Arc<Socket>, v: Value, ack_id: Option<i64>);
}

/// Define a handler for the connect event.
/// It is implemented for closures with up to 16 arguments. They must implement the [`FromMessageParts`] trait or the [`FromMessage`] trait for the last one.
///
/// * See the [`message`](super::message) module doc for more details on message handler.
/// * See the [`extract`](crate::extract) module doc for more details on available extractors.
#[diagnostic::on_unimplemented(
    note = "This function is not a MessageHandler. Check that:
* It is a clonable async `FnOnce` that returns nothing.
* All its arguments are valid message extractors.
* If you use a custom adapter, it must be generic over the adapter type.
See `https://docs.rs/socketioxide/latest/socketioxide/extract/index.html` for details.\n",
    label = "Invalid MessageHandler"
)]
pub trait MessageHandler<T>: Send + Sync + 'static {
    /// Call the handler with the given arguments
    fn call(&self, s: Arc<Socket>, v: Value, ack_id: Option<i64>);

    #[doc(hidden)]
    fn phantom(&self) -> std::marker::PhantomData<T> {
        std::marker::PhantomData
    }
}

impl<T, H> MakeErasedHandler<H, T>
where
    T: Send + Sync + 'static,
    H: MessageHandler<T>,
{
    pub fn new_message_boxed(inner: H) -> Box<dyn ErasedMessageHandler> {
        Box::new(MakeErasedHandler::new(inner))
    }
}
impl<T, H> ErasedMessageHandler for MakeErasedHandler<H, T>
where
    T: Send + Sync + 'static,
    H: MessageHandler<T>,
{
    #[cfg_attr(feature = "tracing", tracing::instrument(level = "trace", skip(self, s), fields(id = ?s.id())))]
    #[inline(always)]
    fn call(&self, s: Arc<Socket>, v: Value, ack_id: Option<i64>) {
        self.handler.call(s, v, ack_id);
    }
}

mod private {
    #[derive(Debug, Clone, Copy)]
    pub enum ViaParts {}

    #[derive(Debug, Clone, Copy)]
    pub enum ViaRequest {}
}

/// A trait used to extract arguments from the message event.
/// The `Result` associated type is used to return an error if the extraction fails, in this case the handler is not called.
///
/// * See the [`message`](super::message) module doc for more details on message handler.
/// * See the [`extract`](crate::extract) module doc for more details on available extractors.
#[diagnostic::on_unimplemented(
    note = "This function argument is not a valid socketio extractor.
See `https://docs.rs/socketioxide/latest/socketioxide/extract/index.html` for details\n",
    label = "Invalid extractor"
)]
pub trait FromMessageParts: Sized {
    /// The error type returned by the extractor
    type Error: std::error::Error + 'static;

    /// Extract the arguments from the message event.
    /// If it fails, the handler is not called.
    fn from_message_parts(
        s: &Arc<Socket>,
        v: &mut Value,
        ack_id: &Option<i64>,
    ) -> Result<Self, Self::Error>;
}

/// A trait used to extract and **consume** arguments from the message event.
/// The `Result` associated type is used to return an error if the extraction fails, in this case the handler is not called.
///
/// * See the [`message`](super::message) module doc for more details on message handler.
/// * See the [`extract`](crate::extract) module doc for more details on available extractors.
#[diagnostic::on_unimplemented(
    note = "This function argument is not a valid socketio extractor.
See `https://docs.rs/socketioxide/latest/socketioxide/extract/index.html` for details\n",
    label = "Invalid extractor"
)]
pub trait FromMessage<M = private::ViaRequest>: Sized {
    /// The error type returned by the extractor
    type Error: std::error::Error + 'static;

    /// Extract the arguments from the message event.
    /// If it fails, the handler is not called
    fn from_message(s: Arc<Socket>, v: Value, ack_id: Option<i64>) -> Result<Self, Self::Error>;
}

/// All the types that implement [`FromMessageParts`] also implement [`FromMessage`]
impl<T> FromMessage<private::ViaParts> for T
where
    T: FromMessageParts,
{
    type Error = T::Error;
    fn from_message(
        s: Arc<Socket>,
        mut v: Value,
        ack_id: Option<i64>,
    ) -> Result<Self, Self::Error> {
        Self::from_message_parts(&s, &mut v, &ack_id)
    }
}

/// Empty handler
impl<F, Fut> MessageHandler<()> for F
where
    F: FnOnce() -> Fut + Send + Sync + Clone + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    fn call(&self, _: Arc<Socket>, _: Value, _: Option<i64>) {
        let fut = (self.clone())();
        tokio::spawn(fut);
    }
}

macro_rules! impl_async_handler {
    (
        [$($ty:ident),*], $last:ident
    ) => {
        #[allow(non_snake_case, unused)]
        #[diagnostic::do_not_recommend]
        impl<F, M, $($ty,)* $last, Fut> MessageHandler<(M, $($ty,)* $last,)> for F
        where
            F: FnOnce($($ty,)* $last,) -> Fut + Send + Sync + Clone + 'static,
            Fut: Future<Output = ()> + Send + 'static,
            $( $ty: FromMessageParts + Send, )*
            $last: FromMessage<M> + Send,
        {
            fn call(&self, s: Arc<Socket>, mut v: Value, ack_id: Option<i64>) {
                $(
                    let $ty = match $ty::from_message_parts(&s, &mut v, &ack_id) {
                        Ok(v) => v,
                        Err(_e) => {
                            #[cfg(feature = "tracing")]
                            tracing::error!("Error while extracting data: {}", _e);
                            return;
                        },
                    };
                )*
                let last = match $last::from_message(s, v, ack_id) {
                    Ok(v) => v,
                    Err(_e) => {
                        #[cfg(feature = "tracing")]
                        tracing::error!("Error while extracting data: {}", _e);
                        return;
                    },
                };

                let fut = (self.clone())($($ty,)* last);
                tokio::spawn(fut);
            }
        }
    };
}

#[rustfmt::skip]
macro_rules! all_the_tuples {
    ($name:ident) => {
        $name!([], T1);
        $name!([T1], T2);
        $name!([T1, T2], T3);
        $name!([T1, T2, T3], T4);
        $name!([T1, T2, T3, T4], T5);
        $name!([T1, T2, T3, T4, T5], T6);
        $name!([T1, T2, T3, T4, T5, T6], T7);
        $name!([T1, T2, T3, T4, T5, T6, T7], T8);
        $name!([T1, T2, T3, T4, T5, T6, T7, T8], T9);
        $name!([T1, T2, T3, T4, T5, T6, T7, T8, T9], T10);
        $name!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10], T11);
        $name!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11], T12);
        $name!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12], T13);
        $name!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13], T14);
        $name!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14], T15);
        $name!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15], T16);
    };
}

all_the_tuples!(impl_async_handler);
