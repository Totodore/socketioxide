//! [`ConnectHandler`] trait and implementations, used to handle the connect event.
//! It has a flexible axum-like API, you can put any arguments as long as it implements the [`FromConnectParts`] trait.
//!
//! You can also implement the [`FromConnectParts`] trait for your own types.
//! See the [`extract`](super::extract) module doc for more details on available extractors.
//!
//! Handlers can be _optionally_ async.
//!
//! ## Example with sync closures
//! ```rust
//! # use socketioxide::SocketIo;
//! # use serde_json::Error;
//! # use socketioxide::extract::*;
//! let (svc, io) = SocketIo::new_svc();
//! // Here the handler is sync,
//! // if there is a serialization error, the handler is not called
//! io.ns("/nsp", move |s: SocketRef, Data(auth): Data<String>| {
//!     println!("Socket connected on /nsp namespace with id: {} and data: {}", s.id, auth);
//! });
//! ```
//!
//! ## Example with async closures
//! ```rust
//! # use socketioxide::SocketIo;
//! # use serde_json::Error;
//! # use socketioxide::extract::*;
//! let (svc, io) = SocketIo::new_svc();
//! // Here the handler is async and extract the current socket and the auth payload
//! io.ns("/", move |s: SocketRef, TryData(auth): TryData<String>| async move {
//!     println!("Socket connected on / namespace with id and auth data: {} {:?}", s.id, auth);
//! });
//! // Here the handler is async and only extract the current socket.
//! // The auth payload won't be deserialized and will be dropped
//! io.ns("/async_nsp", move |s: SocketRef| async move {
//!     println!("Socket connected on /async_nsp namespace with id: {}", s.id);
//! });
//! ```
//!
//! ## Example with async non anonymous functions
//! ```rust
//! # use socketioxide::SocketIo;
//! # use serde_json::Error;
//! # use socketioxide::extract::*;
//! async fn handler(s: SocketRef, TryData(auth): TryData<String>) {
//!     tokio::time::sleep(std::time::Duration::from_secs(1)).await;
//!     println!("Socket connected on {} namespace with id and auth data: {} {:?}", s.ns(), s.id, auth);
//! }
//!
//! let (svc, io) = SocketIo::new_svc();
//!
//! // You can reuse the same handler for multiple namespaces
//! io.ns("/", handler);
//! io.ns("/admin", handler);
//! ```
use std::sync::Arc;

use futures::Future;

use crate::{adapter::Adapter, packet::Packet, socket::Socket};

use super::MakeErasedHandler;

/// A Type Erased [`ConnectHandler`] so it can be stored in a HashMap
pub(crate) type BoxedConnectHandler<A> = Box<dyn ErasedConnectHandler<A>>;
pub(crate) trait ErasedConnectHandler<A: Adapter>: Send + Sync + 'static {
    fn call(&self, s: Arc<Socket<A>>, auth: Option<String>);
}

/// A `connect` handler that is layered with a middleware.
/// It is used to add a middleware to a handler with the [`with`](ConnectHandler::with) method.
/// You can chain multiple middlewares with this method.
pub struct LayeredConnectHandler<H, N, A, T, T1> {
    next: N,
    handler: H,
    adapter: std::marker::PhantomData<A>,
    type_: std::marker::PhantomData<T>,
    type1_: std::marker::PhantomData<T1>,
}

impl<A: Adapter, T, H> MakeErasedHandler<H, A, T>
where
    T: Send + Sync + 'static,
    H: ConnectHandler<A, T> + Send + Sync + Clone + 'static,
{
    pub fn new_ns_boxed(inner: H) -> Box<dyn ErasedConnectHandler<A>> {
        Box::new(MakeErasedHandler::new(inner))
    }
}

impl<A: Adapter, T, H> ErasedConnectHandler<A> for MakeErasedHandler<H, A, T>
where
    H: ConnectHandler<A, T> + Send + Sync + Clone + 'static,
    T: Send + Sync + 'static,
{
    fn call(&self, s: Arc<Socket<A>>, auth: Option<String>) {
        let handler = self.handler.clone();
        tokio::spawn(async move {
            if let Err(data) = handler.call(s.clone(), &auth).await {
                let ns = s.ns();
                let data = data.to_string();

                #[cfg(feature = "tracing")]
                tracing::trace!(ns, ?s.id, "emitting connect_error packet");

                if let Err(e) = s.send(Packet::connect_error(ns, &data)) {
                    #[cfg(feature = "tracing")]
                    tracing::trace!(?e, ns, ?s.id, data, "could not send connect_error packet");
                }
            }
        });
    }
}

/// A trait used to extract the arguments from the connect event.
/// The `Result` associated type is used to return an error if the extraction fails,
/// in this case the [`ConnectHandler`] is not called.
///
/// * See the [`connect`](super::connect) module doc for more details on connect handler.
/// * See the [`extract`](super::extract) module doc for more details on available extractors.
pub trait FromConnectParts<A: Adapter>: Sized {
    /// The error type returned by the extractor
    type Error: std::error::Error + 'static;

    /// Extract the arguments from the connect event.
    /// If it fails, the handler is not called
    fn from_connect_parts(s: &Arc<Socket<A>>, auth: &Option<String>) -> Result<Self, Self::Error>;
}

/// Define a handler for the connect event.
/// It is implemented for closures with up to 16 arguments. They must implement the [`FromConnectParts`] trait.
///
/// * See the [`connect`](super::connect) module doc for more details on connect handler.
/// * See the [`extract`](super::extract) module doc for more details on available extractors.
pub trait ConnectHandler<A: Adapter, T>: Send + Sync + 'static {
    /// The error type returned by the handler
    type Error: std::fmt::Display + 'static;

    /// Call the handler with the given arguments.
    fn call(
        &self,
        s: Arc<Socket<A>>,
        auth: &Option<String>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Test
    fn with<H, T1>(self, handler: H) -> LayeredConnectHandler<H, Self, A, T, T1>
    where
        Self: Sized,
        H: ConnectHandler<A, T1>,
        T1: Send + Sync + 'static,
        T: Send + Sync + 'static,
    {
        LayeredConnectHandler {
            next: self,
            handler,
            adapter: std::marker::PhantomData,
            type_: std::marker::PhantomData,
            type1_: std::marker::PhantomData,
        }
    }

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

    #[derive(Debug, Copy, Clone)]
    pub enum Handler {}
    #[derive(Debug, Copy, Clone)]
    pub enum Middleware {}
}

impl<H, N, A, T, T1> ConnectHandler<A, T1> for LayeredConnectHandler<H, N, A, T, T1>
where
    H: ConnectHandler<A, T1>,
    N: ConnectHandler<A, T>,
    A: Adapter,
    T: Send + Sync + 'static,
    T1: Send + Sync + 'static,
    H::Error: From<N::Error>,
{
    type Error = H::Error;
    fn call(
        &self,
        s: Arc<Socket<A>>,
        auth: &Option<String>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        async move {
            self.handler.call(s.clone(), auth).await?;
            self.next.call(s, auth).await?;
            Ok(())
        }
    }
}

impl<H, N, A, T, T1> Clone for LayeredConnectHandler<H, N, A, T, T1>
where
    H: ConnectHandler<A, T1> + Clone,
    N: ConnectHandler<A, T> + Clone,
    A: Adapter,
    T: Send + Sync + 'static,
    T1: Send + Sync + 'static,
{
    fn clone(&self) -> Self {
        Self {
            handler: self.handler.clone(),
            next: self.next.clone(),
            adapter: std::marker::PhantomData,
            type_: std::marker::PhantomData,
            type1_: std::marker::PhantomData,
        }
    }
}

macro_rules! impl_handler_async {
    (
        [$($ty:ident),*]
    ) => {
        #[allow(non_snake_case, unused)]
        impl<A, F, Fut, $($ty,)*> ConnectHandler<A, (private::Handler, private::Async, $($ty,)*)> for F
        where
            F: FnOnce($($ty,)*) -> Fut + Send + Sync + Clone + 'static,
            Fut: Future<Output = ()> + Send + 'static,
            A: Adapter,
            $( $ty: FromConnectParts<A> + Send, )*
        {
			type Error = std::convert::Infallible;
            async fn call(&self, s: Arc<Socket<A>>, auth: &Option<String>) -> Result<(), Self::Error> {
                $(
                    let $ty = match $ty::from_connect_parts(&s, &auth) {
                        Ok(v) => v,
                        Err(_e) => {
                            #[cfg(feature = "tracing")]
                            tracing::error!("Error while extracting data: {}", _e);
                            return Ok(());
                        },
                    };
                )*

				(self.clone())($($ty,)*).await;
				Ok(())
            }
        }
    };
}

macro_rules! impl_handler {
    (
        [$($ty:ident),*]
    ) => {
        #[allow(non_snake_case, unused)]
        impl<A, F, $($ty,)*> ConnectHandler<A, (private::Handler, private::Sync, $($ty,)*)> for F
        where
            F: FnOnce($($ty,)*) + Send + Sync + Clone + 'static,
            A: Adapter,
            $( $ty: FromConnectParts<A> + Send, )*
        {
			type Error = std::convert::Infallible;
            async fn call(&self, s: Arc<Socket<A>>, auth: &Option<String>) -> Result<(), Self::Error> {
                $(
                    let $ty = match $ty::from_connect_parts(&s, &auth) {
                        Ok(v) => v,
                        Err(_e) => {
                            #[cfg(feature = "tracing")]
                            tracing::error!("Error while extracting data: {}", _e);
                            return Ok(());
                        },
                    };
                )*

                (self.clone())($($ty,)*);
				Ok(())
            }
        }
    };
}

macro_rules! impl_middleware {
    (
        [$($ty:ident),*]
    ) => {
        #[allow(non_snake_case, unused)]
        impl<A, F, E, $($ty,)*> ConnectHandler<A, (private::Middleware, private::Sync, $($ty,)*)> for F
        where
            F: FnOnce($($ty,)*) -> Result<(), E> + Send + Sync + Clone + 'static,
            A: Adapter,
			E: std::fmt::Display + 'static,
            $( $ty: FromConnectParts<A> + Send, )*
        {
			type Error = E;
            async fn call(&self, s: Arc<Socket<A>>, auth: &Option<String>) -> Result<(), Self::Error> {
                $(
                    let $ty = match $ty::from_connect_parts(&s, auth) {
                        Ok(v) => v,
                        Err(_e) => {
                            #[cfg(feature = "tracing")]
                            tracing::error!("Error while extracting data: {}", _e);
                            return Ok(());
                        },
                    };
                )*

                (self.clone())($($ty,)*)
            }
        }
    };
}

macro_rules! impl_async_middleware {
    (
        [$($ty:ident),*]
    ) => {
        #[allow(non_snake_case, unused)]
        impl<A, F, Fut, E, $($ty,)*> ConnectHandler<A, (private::Middleware, private::Async, $($ty,)*)> for F
        where
			F: FnOnce($($ty,)*) -> Fut + Send + Sync + Clone + 'static,
			Fut: Future<Output = Result<(), E>> + Send + 'static,
            A: Adapter,
			E: std::fmt::Display + Send + 'static,
            $( $ty: FromConnectParts<A> + Send, )*
        {
			type Error = E;
            async fn call(&self, s: Arc<Socket<A>>, auth: &Option<String>) -> Result<(), Self::Error> {
                $(
                    let $ty = match $ty::from_connect_parts(&s, auth) {
                        Ok(v) => v,
                        Err(_e) => {
                            #[cfg(feature = "tracing")]
                            tracing::error!("Error while extracting data: {}", _e);
                            return Ok(());
                        },
                    };
                )*

				(self.clone())($($ty,)*).await
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
all_the_tuples!(impl_middleware);
all_the_tuples!(impl_async_middleware);
