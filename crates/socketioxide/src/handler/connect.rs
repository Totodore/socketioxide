//! [`ConnectHandler`] trait and implementations, used to handle the connect event.
//! It has a flexible axum-like API, you can put any arguments as long as it implements the [`FromConnectParts`] trait.
//!
//! You can also implement the [`FromConnectParts`] trait for your own types.
//! See the [`extract`](crate::extract) module doc for more details on available extractors.
//!
//! Handlers can be _optionally_ async.
//!
//! # Middlewares
//! [`ConnectHandlers`](ConnectHandler) can have middlewares, they are called before the connection
//! of the socket to the namespace and therefore before the handler.
//!
//! <div class="warning">
//!     Because the socket is not yet connected to the namespace,
//!     you can't send messages to it from the middleware.
//! </div>
//!
//! Middlewares can be sync or async and can be chained.
//! They are defined with the [`ConnectMiddleware`] trait which is automatically implemented for any
//! closure with up to 16 arguments with the following signature:
//! * `FnOnce(*args) -> Result<(), E> where E: Display`
//! * `async FnOnce(*args) -> Result<(), E> where E: Display`
//!
//! Arguments must implement the [`FromConnectParts`] trait in the exact same way than handlers.
//!
//! ## Example with sync closures
//! ```rust
//! # use socketioxide::SocketIo;
//! # use socketioxide::extract::*;
//! let (svc, io) = SocketIo::new_svc();
//! // Here the handler is sync,
//! // if there is a serialization error, the handler is not called
//! io.ns("/nsp", move |io: SocketIo, s: SocketRef, Data(auth): Data<String>| {
//!     println!("Socket connected on /nsp namespace with id: {} and data: {}", s.id, auth);
//! });
//! ```
//!
//! ## Example with async closures
//! ```rust
//! # use socketioxide::SocketIo;
//! # use socketioxide::extract::*;
//! let (svc, io) = SocketIo::new_svc();
//! // Here the handler is async and extract the current socket and the auth payload
//! io.ns("/", move |io: SocketIo, s: SocketRef, TryData(auth): TryData<String>| async move {
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
//!
//! ## Example with middlewares
//!
//! ```rust
//! # use socketioxide::handler::ConnectHandler;
//! # use socketioxide::extract::*;
//! # use socketioxide::SocketIo;
//! fn handler(s: SocketRef) {
//!     println!("socket connected on / namespace with id: {}", s.id);
//! }
//!
//! #[derive(Debug)]
//! struct AuthError;
//! impl std::fmt::Display for AuthError {
//!     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//!         write!(f, "AuthError")
//!     }
//! }
//! impl std::error::Error for AuthError {}
//!
//! fn middleware(s: SocketRef, Data(token): Data<String>) -> Result<(), AuthError> {
//!     println!("second middleware called");
//!     if token != "secret" {
//!         Err(AuthError)
//!     } else {
//!         Ok(())
//!     }
//! }
//!
//! // Middlewares can be sync or async
//! async fn other_middleware(s: SocketRef) -> Result<(), AuthError> {
//!     println!("first middleware called");
//!     if s.req_parts().uri.query().map(|q| q.contains("secret")).unwrap_or_default() {
//!         Err(AuthError)
//!     } else {
//!         Ok(())
//!     }
//! }
//!
//! let (_, io) = SocketIo::new_layer();
//! io.ns("/", handler.with(middleware).with(other_middleware));
//! ```

use std::pin::Pin;
use std::sync::Arc;

use crate::{adapter::Adapter, socket::Socket};
use futures_core::Future;
use socketioxide_core::Value;

use super::MakeErasedHandler;

/// A Type Erased [`ConnectHandler`] so it can be stored in a HashMap
pub(crate) type BoxedConnectHandler<A> = Box<dyn ErasedConnectHandler<A>>;

type MiddlewareRes = Result<(), Box<dyn std::fmt::Display + Send>>;
type MiddlewareResFut<'a> = Pin<Box<dyn Future<Output = MiddlewareRes> + Send + 'a>>;

pub(crate) trait ErasedConnectHandler<A: Adapter>: Send + Sync + 'static {
    #[cfg_attr(feature = "tracing", tracing::instrument(level = "trace", skip(self, s), fields(id = ?s.id)))]
    fn call(&self, s: Arc<Socket<A>>, auth: Option<Value>);
    fn call_middleware<'a>(
        &'a self,
        s: Arc<Socket<A>>,
        auth: &'a Option<Value>,
    ) -> MiddlewareResFut<'a>;

    fn boxed_clone(&self) -> BoxedConnectHandler<A>;
}

/// A trait used to extract the arguments from the connect event.
/// The `Result` associated type is used to return an error if the extraction fails,
/// in this case the [`ConnectHandler`] is not called.
///
/// * See the [`connect`](super::connect) module doc for more details on connect handler.
/// * See the [`extract`](crate::extract) module doc for more details on available extractors.
#[rustversion::attr(
    since(1.78),
    diagnostic::on_unimplemented(
        note = "Function argument is not a valid socketio extractor.
See `https://docs.rs/socketioxide/latest/socketioxide/extract/index.html` for details",
        label = "Invalid extractor"
    )
)]
pub trait FromConnectParts<A: Adapter>: Sized {
    /// The error type returned by the extractor
    type Error: std::error::Error + Send + 'static;

    /// Extract the arguments from the connect event.
    /// If it fails, the handler is not called
    fn from_connect_parts(s: &Arc<Socket<A>>, auth: &Option<Value>) -> Result<Self, Self::Error>;
}

/// Define a middleware for the connect event.
/// It is implemented for closures with up to 16 arguments.
/// They must implement the [`FromConnectParts`] trait and return `Result<(), E> where E: Display`.
///
/// * See the [`connect`](super::connect) module doc for more details on connect middlewares.
/// * See the [`extract`](crate::extract) module doc for more details on available extractors.
#[rustversion::attr(
    since(1.78),
    diagnostic::on_unimplemented(
        note = "This function is not a ConnectMiddleware. Check that:
* It is a clonable sync or async `FnOnce` that returns `Result<(), E> where E: Display`.
* All its arguments are valid connect extractors.
See `https://docs.rs/socketioxide/latest/socketioxide/extract/index.html` for details.\n",
        label = "Invalid ConnectMiddleware"
    )
)]
pub trait ConnectMiddleware<A: Adapter, T>: Sized + Clone + Send + Sync + 'static {
    /// Call the middleware with the given arguments.
    fn call<'a>(
        &'a self,
        s: Arc<Socket<A>>,
        auth: &'a Option<Value>,
    ) -> impl Future<Output = MiddlewareRes> + Send;

    #[doc(hidden)]
    fn phantom(&self) -> std::marker::PhantomData<(A, T)> {
        std::marker::PhantomData
    }
}

/// Define a handler for the connect event.
/// It is implemented for closures with up to 16 arguments. They must implement the [`FromConnectParts`] trait.
///
/// * See the [`connect`](super::connect) module doc for more details on connect handler.
/// * See the [`extract`](crate::extract) module doc for more details on available extractors.
#[rustversion::attr(
    since(1.78),
    diagnostic::on_unimplemented(
        note = "This function is not a ConnectHandler. Check that:
* It is a clonable sync or async `FnOnce` that returns nothing.
* All its arguments are valid connect extractors.
See `https://docs.rs/socketioxide/latest/socketioxide/extract/index.html` for details.\n",
        label = "Invalid ConnectHandler"
    )
)]
pub trait ConnectHandler<A: Adapter, T>: Sized + Clone + Send + Sync + 'static {
    /// Call the handler with the given arguments.
    fn call(&self, s: Arc<Socket<A>>, auth: Option<Value>);

    /// Call the middleware with the given arguments.
    fn call_middleware<'a>(
        &'a self,
        _: Arc<Socket<A>>,
        _: &'a Option<Value>,
    ) -> MiddlewareResFut<'a> {
        Box::pin(async move { Ok(()) })
    }

    /// Wraps this [`ConnectHandler`] with a new [`ConnectMiddleware`].
    /// The new provided middleware will be called before the current one.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use socketioxide::handler::ConnectHandler;
    /// # use socketioxide::extract::*;
    /// # use socketioxide::SocketIo;
    /// fn handler(s: SocketRef) {
    ///     println!("socket connected on / namespace with id: {}", s.id);
    /// }
    ///
    /// #[derive(Debug)]
    /// struct AuthError;
    /// impl std::fmt::Display for AuthError {
    ///     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    ///         write!(f, "AuthError")
    ///     }
    /// }
    /// impl std::error::Error for AuthError {}
    ///
    /// fn middleware(s: SocketRef, Data(token): Data<String>) -> Result<(), AuthError> {
    ///     println!("second middleware called");
    ///     if token != "secret" {
    ///         Err(AuthError)
    ///     } else {
    ///         Ok(())
    ///     }
    /// }
    ///
    /// // Middlewares can be sync or async
    /// async fn other_middleware(s: SocketRef) -> Result<(), AuthError> {
    ///     println!("first middleware called");
    ///     if s.req_parts().uri.query().map(|q| q.contains("secret")).unwrap_or_default() {
    ///         Err(AuthError)
    ///     } else {
    ///         Ok(())
    ///     }
    /// }
    ///
    /// let (_, io) = SocketIo::new_layer();
    /// io.ns("/", handler.with(middleware).with(other_middleware));
    /// ```
    fn with<M, T1>(self, middleware: M) -> impl ConnectHandler<A, T>
    where
        M: ConnectMiddleware<A, T1> + Send + Sync + 'static,
        T: Send + Sync + 'static,
        T1: Send + Sync + 'static,
    {
        LayeredConnectHandler {
            handler: self,
            middleware,
            phantom: std::marker::PhantomData,
        }
    }

    #[doc(hidden)]
    fn phantom(&self) -> std::marker::PhantomData<T> {
        std::marker::PhantomData
    }
}
struct LayeredConnectHandler<A, H, M, T, T1> {
    handler: H,
    middleware: M,
    phantom: std::marker::PhantomData<(A, T, T1)>,
}
struct ConnectMiddlewareLayer<M, N, T, T1> {
    middleware: M,
    next: N,
    phantom: std::marker::PhantomData<(T, T1)>,
}

impl<A: Adapter, T, H> MakeErasedHandler<H, A, T>
where
    H: ConnectHandler<A, T> + Send + Sync + 'static,
    T: Send + Sync + 'static,
{
    pub fn new_ns_boxed(inner: H) -> Box<dyn ErasedConnectHandler<A>> {
        Box::new(MakeErasedHandler::new(inner))
    }
}

impl<A: Adapter, T, H> ErasedConnectHandler<A> for MakeErasedHandler<H, A, T>
where
    H: ConnectHandler<A, T> + Send + Sync + 'static,
    T: Send + Sync + 'static,
{
    fn call(&self, s: Arc<Socket<A>>, auth: Option<Value>) {
        self.handler.call(s, auth);
    }

    fn call_middleware<'a>(
        &'a self,
        s: Arc<Socket<A>>,
        auth: &'a Option<Value>,
    ) -> MiddlewareResFut<'a> {
        self.handler.call_middleware(s, auth)
    }

    fn boxed_clone(&self) -> BoxedConnectHandler<A> {
        Box::new(self.clone())
    }
}

impl<A, H, M, T, T1> ConnectHandler<A, T> for LayeredConnectHandler<A, H, M, T, T1>
where
    A: Adapter,
    H: ConnectHandler<A, T> + Send + Sync + 'static,
    M: ConnectMiddleware<A, T1> + Send + Sync + 'static,
    T: Send + Sync + 'static,
    T1: Send + Sync + 'static,
{
    fn call(&self, s: Arc<Socket<A>>, auth: Option<Value>) {
        self.handler.call(s, auth);
    }

    fn call_middleware<'a>(
        &'a self,
        s: Arc<Socket<A>>,
        auth: &'a Option<Value>,
    ) -> MiddlewareResFut<'a> {
        Box::pin(async move { self.middleware.call(s, auth).await })
    }

    fn with<M2, T2>(self, next: M2) -> impl ConnectHandler<A, T>
    where
        M2: ConnectMiddleware<A, T2> + Send + Sync + 'static,
        T2: Send + Sync + 'static,
    {
        LayeredConnectHandler {
            handler: self.handler,
            middleware: ConnectMiddlewareLayer {
                middleware: next,
                next: self.middleware,
                phantom: std::marker::PhantomData,
            },
            phantom: std::marker::PhantomData,
        }
    }
}
impl<A, H, N, T, T1> ConnectMiddleware<A, T1> for LayeredConnectHandler<A, H, N, T, T1>
where
    A: Adapter,
    H: ConnectHandler<A, T> + Send + Sync + 'static,
    N: ConnectMiddleware<A, T1> + Send + Sync + 'static,
    T: Send + Sync + 'static,
    T1: Send + Sync + 'static,
{
    async fn call<'a>(&'a self, s: Arc<Socket<A>>, auth: &'a Option<Value>) -> MiddlewareRes {
        self.middleware.call(s, auth).await
    }
}
impl<A, H, N, T, T1> Clone for LayeredConnectHandler<A, H, N, T, T1>
where
    H: Clone,
    N: Clone,
{
    fn clone(&self) -> Self {
        Self {
            handler: self.handler.clone(),
            middleware: self.middleware.clone(),
            phantom: self.phantom,
        }
    }
}
impl<M, N, T, T1> Clone for ConnectMiddlewareLayer<M, N, T, T1>
where
    M: Clone,
    N: Clone,
{
    fn clone(&self) -> Self {
        Self {
            middleware: self.middleware.clone(),
            next: self.next.clone(),
            phantom: self.phantom,
        }
    }
}

impl<A, M, N, T, T1> ConnectMiddleware<A, T> for ConnectMiddlewareLayer<M, N, T, T1>
where
    A: Adapter,
    M: ConnectMiddleware<A, T> + Send + Sync + 'static,
    N: ConnectMiddleware<A, T1> + Send + Sync + 'static,
    T: Send + Sync + 'static,
    T1: Send + Sync + 'static,
{
    async fn call<'a>(&'a self, s: Arc<Socket<A>>, auth: &'a Option<Value>) -> MiddlewareRes {
        self.middleware.call(s.clone(), auth).await?;
        self.next.call(s, auth).await
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
        impl<A, F, Fut, $($ty,)*> ConnectHandler<A, (private::Async, $($ty,)*)> for F
        where
            F: FnOnce($($ty,)*) -> Fut + Send + Sync + Clone + 'static,
            Fut: Future<Output = ()> + Send + 'static,
            A: Adapter,
            $( $ty: FromConnectParts<A> + Send, )*
        {
            fn call(&self, s: Arc<Socket<A>>, auth: Option<Value>) {
                $(
                    let $ty = match $ty::from_connect_parts(&s, &auth) {
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
        impl<A, F, $($ty,)*> ConnectHandler<A, (private::Sync, $($ty,)*)> for F
        where
            F: FnOnce($($ty,)*) + Send + Sync + Clone + 'static,
            A: Adapter,
            $( $ty: FromConnectParts<A> + Send, )*
        {
            fn call(&self, s: Arc<Socket<A>>, auth: Option<Value>) {
                $(
                    let $ty = match $ty::from_connect_parts(&s, &auth) {
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

macro_rules! impl_middleware_async {
    (
        [$($ty:ident),*]
    ) => {
        #[allow(non_snake_case, unused)]
        impl<A, F, Fut, E, $($ty,)*> ConnectMiddleware<A, (private::Async, $($ty,)*)> for F
        where
            F: FnOnce($($ty,)*) -> Fut + Send + Sync + Clone + 'static,
            Fut: Future<Output = Result<(), E>> + Send + 'static,
            A: Adapter,
            E: std::fmt::Display + Send + 'static,
            $( $ty: FromConnectParts<A> + Send, )*
        {
            async fn call<'a>(
                &'a self,
                s: Arc<Socket<A>>,
                auth: &'a Option<Value>,
            ) -> MiddlewareRes {
                $(
                    let $ty = match $ty::from_connect_parts(&s, auth) {
                        Ok(v) => v,
                        Err(e) => {
                            #[cfg(feature = "tracing")]
                            tracing::error!("Error while extracting data: {}", e);
                            return Err(Box::new(e) as _);
                        },
                    };
                )*

                let res = (self.clone())($($ty,)*).await;
                if let Err(e) = res {
                    #[cfg(feature = "tracing")]
                    tracing::trace!("middleware returned error: {}", e);
                    Err(Box::new(e) as _)
                } else {
                    Ok(())
                }
            }
        }
    };
}

macro_rules! impl_middleware {
    (
        [$($ty:ident),*]
    ) => {
        #[allow(non_snake_case, unused)]
        impl<A, F, E, $($ty,)*> ConnectMiddleware<A, (private::Sync, $($ty,)*)> for F
        where
            F: FnOnce($($ty,)*) -> Result<(), E> + Send + Sync + Clone + 'static,
            A: Adapter,
            E: std::fmt::Display + Send + 'static,
            $( $ty: FromConnectParts<A> + Send, )*
        {
            async fn call<'a>(
                &'a self,
                s: Arc<Socket<A>>,
                auth: &'a Option<Value>,
            ) -> MiddlewareRes {
                $(
                    let $ty = match $ty::from_connect_parts(&s, auth) {
                        Ok(v) => v,
                        Err(e) => {
                            #[cfg(feature = "tracing")]
                            tracing::error!("Error while extracting data: {}", e);
                            return Err(Box::new(e) as _);
                        },
                    };
                )*

                let res = (self.clone())($($ty,)*);
                if let Err(e) = res {
                    #[cfg(feature = "tracing")]
                    tracing::trace!("middleware returned error: {}", e);
                    Err(Box::new(e) as _)
                } else {
                    Ok(())
                }
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

all_the_tuples!(impl_middleware_async);
all_the_tuples!(impl_middleware);
