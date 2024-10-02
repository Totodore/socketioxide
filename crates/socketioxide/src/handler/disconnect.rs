//! [`DisconnectHandler`] trait and implementations, used to handle the disconnect event.
//! It has a flexible axum-like API, you can put any arguments as long as it implements the [`FromDisconnectParts`] trait.
//!
//! You can also implement the [`FromDisconnectParts`] trait for your own types.
//! See the [`extract`](crate::extract) module doc for more details on available extractors.
//!
//! Handlers can be _optionally_ async.
//!
//! ## Example with sync closures
//! ```rust
//! # use socketioxide::SocketIo;
//! # use serde_json::Error;
//! # use socketioxide::extract::*;
//! # use socketioxide::socket::DisconnectReason;
//! let (svc, io) = SocketIo::new_svc();
//! io.ns("/", |s: SocketRef| {
//!     s.on_disconnect(|s: SocketRef, reason: DisconnectReason| {
//!         println!("Socket {} was disconnected because {} ", s.id, reason);
//!     });
//! });
//! ```
//!
//! ## Example with async closures
//! ```rust
//! # use socketioxide::SocketIo;
//! # use serde_json::Error;
//! # use socketioxide::extract::*;
//! let (svc, io) = SocketIo::new_svc();
//! io.ns("/", |s: SocketRef| {
//!     s.on_disconnect(move |s: SocketRef| async move {
//!         println!("Socket {} was disconnected", s.id);
//!     });
//! });
//! ```
//!
//! ## Example with async non anonymous functions
//! ```rust
//! # use socketioxide::SocketIo;
//! # use serde_json::Error;
//! # use socketioxide::extract::*;
//! # use socketioxide::socket::DisconnectReason;
//! async fn handler(s: SocketRef, reason: DisconnectReason) {
//!     tokio::time::sleep(std::time::Duration::from_secs(1)).await;
//!     println!("Socket disconnected on {} namespace with id and reason: {} {}", s.ns(), s.id, reason);
//! }
//!
//! let (svc, io) = SocketIo::new_svc();
//!
//! // You can reuse the same handler for multiple sockets
//! io.ns("/", |s: SocketRef| {
//!     s.on_disconnect(handler);
//! });
//! io.ns("/admin", |s: SocketRef| {
//!     s.on_disconnect(handler);
//! });
//! ```
use std::sync::Arc;

use futures_core::Future;

use crate::{
    adapter::Adapter,
    socket::{DisconnectReason, Socket},
};

use super::MakeErasedHandler;

/// A Type Erased [`DisconnectHandler`] so it can be stored in a HashMap
pub(crate) type BoxedDisconnectHandler<A> = Box<dyn ErasedDisconnectHandler<A>>;
pub(crate) trait ErasedDisconnectHandler<A: Adapter>: Send + Sync + 'static {
    fn call(&self, s: Arc<Socket<A>>, reason: DisconnectReason);
}

impl<A: Adapter, T, H> MakeErasedHandler<H, A, T>
where
    T: Send + Sync + 'static,
    H: DisconnectHandler<A, T> + Send + Sync + 'static,
{
    pub fn new_disconnect_boxed(inner: H) -> Box<dyn ErasedDisconnectHandler<A>> {
        Box::new(MakeErasedHandler::new(inner))
    }
}

impl<A: Adapter, T, H> ErasedDisconnectHandler<A> for MakeErasedHandler<H, A, T>
where
    H: DisconnectHandler<A, T> + Send + Sync + 'static,
    T: Send + Sync + 'static,
{
    #[cfg_attr(feature = "tracing", tracing::instrument(level = "trace", skip(self, s), fields(id = ?s.id)))]
    #[inline(always)]
    fn call(&self, s: Arc<Socket<A>>, reason: DisconnectReason) {
        self.handler.call(s, reason);
    }
}

/// A trait used to extract the arguments from the disconnect event.
/// The `Result` associated type is used to return an error if the extraction fails,
/// in this case the [`DisconnectHandler`] is not called.
///
/// * See the [`disconnect`](super::disconnect) module doc for more details on disconnect handler.
/// * See the [`extract`](crate::extract) module doc for more details on available extractors.
#[rustversion::attr(
    since(1.78),
    diagnostic::on_unimplemented(
        note = "Function argument is not a valid socketio extractor. \nSee `https://docs.rs/socketioxide/latest/socketioxide/extract/index.html` for details",
        label = "Invalid extractor"
    )
)]
pub trait FromDisconnectParts<A: Adapter>: Sized {
    /// The error type returned by the extractor
    type Error: std::error::Error + 'static;

    /// Extract the arguments from the disconnect event.
    /// If it fails, the handler is not called
    fn from_disconnect_parts(
        s: &Arc<Socket<A>>,
        reason: DisconnectReason,
    ) -> Result<Self, Self::Error>;
}

/// Define a handler for the disconnect event.
/// It is implemented for closures with up to 16 arguments. They must implement the [`FromDisconnectParts`] trait.
///
/// * See the [`disconnect`](super::disconnect) module doc for more details on disconnect handler.
/// * See the [`extract`](crate::extract) module doc for more details on available extractors.
#[rustversion::attr(
    since(1.78),
    diagnostic::on_unimplemented(
        note = "Function argument is not a valid socketio extractor. \nSee `https://docs.rs/socketioxide/latest/socketioxide/extract/index.html` for details",
        label = "Invalid extractor"
    )
)]
pub trait DisconnectHandler<A: Adapter, T>: Send + Sync + 'static {
    /// Call the handler with the given arguments.
    fn call(&self, s: Arc<Socket<A>>, reason: DisconnectReason);

    #[doc(hidden)]
    fn phantom(&self) -> std::marker::PhantomData<T> {
        std::marker::PhantomData
    }
}

mod private {
    #[derive(Debug, Copy, Clone)]
    pub enum Sync {}
    #[derive(Debug, Copy, Clone)]
    pub enum Async {}
}

macro_rules! impl_handler_async {
    (
        [$($ty:ident),*]
    ) => {
        #[allow(non_snake_case, unused)]
        impl<A, F, Fut, $($ty,)*> DisconnectHandler<A, (private::Async, $($ty,)*)> for F
        where
            F: FnOnce($($ty,)*) -> Fut + Send + Sync + Clone + 'static,
            Fut: Future<Output = ()> + Send + 'static,
            A: Adapter,
            $( $ty: FromDisconnectParts<A> + Send, )*
        {
            fn call(&self, s: Arc<Socket<A>>, reason: DisconnectReason) {
                $(
                    let $ty = match $ty::from_disconnect_parts(&s, reason) {
                        Ok(v) => v,
                        Err(_e) => {
                            #[cfg(feature = "tracing")]
                            tracing::error!("Error while extracting data: {}", _e);
                            return;
                        },
                    };
                )*

                let fut = (self.clone())($($ty,)*);
                tokio::spawn(fut);

            }
        }
    };
}

macro_rules! impl_handler {
    (
        [$($ty:ident),*]
    ) => {
        #[allow(non_snake_case, unused)]
        impl<A, F, $($ty,)*> DisconnectHandler<A, (private::Sync, $($ty,)*)> for F
        where
            F: FnOnce($($ty,)*) + Send + Sync + Clone + 'static,
            A: Adapter,
            $( $ty: FromDisconnectParts<A> + Send, )*
        {
            fn call(&self, s: Arc<Socket<A>>, reason: DisconnectReason) {
                $(
                    let $ty = match $ty::from_disconnect_parts(&s, reason) {
                        Ok(v) => v,
                        Err(_e) => {
                            #[cfg(feature = "tracing")]
                            tracing::error!("Error while extracting data: {}", _e);
                            return;
                        },
                    };
                )*

                (self.clone())($($ty,)*);
            }
        }
    };
}
#[rustfmt::skip]
macro_rules! all_the_tuples {
    ($name:ident) => {
        $name!([]);
        $name!([T1]);
        $name!([T1, T2]);
        $name!([T1, T2, T3]);
        $name!([T1, T2, T3, T4]);
        $name!([T1, T2, T3, T4, T5]);
        $name!([T1, T2, T3, T4, T5, T6]);
        $name!([T1, T2, T3, T4, T5, T6, T7]);
        $name!([T1, T2, T3, T4, T5, T6, T7, T8]);
        $name!([T1, T2, T3, T4, T5, T6, T7, T8, T9]);
        $name!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10]);
        $name!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11]);
        $name!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12]);
        $name!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13]);
        $name!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14]);
        $name!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15]);
        $name!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15, T16]);
    };
}

all_the_tuples!(impl_handler_async);
all_the_tuples!(impl_handler);
