//! [`DisconnectHandler`] trait and implementations, used to handle the disconnect event.
//! It has a flexible axum-like API, you can put any arguments as long as it implements the [`FromDisconnectParts`] trait.
//!
//! You can also implement the [`FromDisconnectParts`] trait for your own types.
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
//!     s.on_disconnect(async |s: SocketRef| {
//!         println!("Socket {} was disconnected", s.id);
//!     });
//! });
//! ```
//!
//! ## Example with an async non-anonymous functions
//! ```rust
//! # use socketioxide::SocketIo;
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
//! io.ns("/", async |s: SocketRef| {
//!     s.on_disconnect(handler);
//! });
//! io.ns("/admin", async |s: SocketRef| {
//!     s.on_disconnect(handler);
//! });
//! ```
use std::future::Future;
use std::sync::Arc;

use crate::{
    adapter::Adapter,
    socket::{DisconnectReason, Socket},
};

use super::MakeErasedHandler;

/// A Type Erased [`DisconnectHandler`] so it can be stored in a HashMap
pub(crate) type BoxedDisconnectHandler<A> = Box<dyn ErasedDisconnectHandler<A>>;
pub(crate) trait ErasedDisconnectHandler<A: Adapter>: Send + Sync + 'static {
    fn call_with_defer(
        &self,
        s: Arc<Socket<A>>,
        reason: DisconnectReason,
        defer: fn(Arc<Socket<A>>),
    );
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
    #[cfg_attr(feature = "tracing", tracing::instrument(level = "trace", skip(self, s, defer), fields(id = ?s.id)))]
    #[inline(always)]
    fn call_with_defer(
        &self,
        s: Arc<Socket<A>>,
        reason: DisconnectReason,
        defer: fn(Arc<Socket<A>>),
    ) {
        self.handler.call_with_defer(s, reason, defer);
    }
}

/// A trait used to extract the arguments from the disconnect event.
/// The `Result` associated type is used to return an error if the extraction fails,
/// in this case the [`DisconnectHandler`] is not called.
///
/// * See the [`disconnect`](super::disconnect) module doc for more details on disconnect handler.
/// * See the [`extract`](crate::extract) module doc for more details on available extractors.
#[diagnostic::on_unimplemented(
    note = "This function argument is not a valid socketio extractor.
See `https://docs.rs/socketioxide/latest/socketioxide/extract/index.html` for details\n",
    label = "Invalid extractor"
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
#[diagnostic::on_unimplemented(
    note = "This function is not a DisconnectHandler. Check that:
* It is a clonable async `FnOnce` that returns nothing.
* All its arguments are valid disconnect extractors.
* If you use a custom adapter, it must be generic over the adapter type.
See `https://docs.rs/socketioxide/latest/socketioxide/extract/index.html` for details.\n",
    label = "Invalid DisconnectHandler"
)]
pub trait DisconnectHandler<A: Adapter, T>: Send + Sync + 'static {
    /// Call the handler with the given arguments.
    fn call(&self, s: Arc<Socket<A>>, reason: DisconnectReason) {
        self.call_with_defer(s, reason, |_| ());
    }

    /// Call the handler and issue a deferred function that will be executed **after** the disconnect handler
    fn call_with_defer(
        &self,
        s: Arc<Socket<A>>,
        reason: DisconnectReason,
        defer: fn(Arc<Socket<A>>),
    );

    #[doc(hidden)]
    fn phantom(&self) -> std::marker::PhantomData<T> {
        std::marker::PhantomData
    }
}

macro_rules! impl_handler_async {
    (
        [$($ty:ident),*]
    ) => {
        #[diagnostic::do_not_recommend]
        #[allow(non_snake_case, unused)]
        impl<A, F, Fut, $($ty,)*> DisconnectHandler<A, ($($ty,)*)> for F
        where
            F: FnOnce($($ty,)*) -> Fut + Send + Sync + Clone + 'static,
            Fut: Future<Output = ()> + Send + 'static,
            A: Adapter,
            $( $ty: FromDisconnectParts<A> + Send, )*
        {
            fn call_with_defer(&self, s: Arc<Socket<A>>, reason: DisconnectReason, defer: fn(Arc<Socket<A>>)) {
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

                tokio::spawn(async move {
                    fut.await;
                    defer(s);
                });
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
