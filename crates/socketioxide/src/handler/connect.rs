//! [`ConnectHandler`] trait and implementations, used to handle the connect event.
//! It has a flexible axum-like API, you can put any arguments as long as it implements the [`FromConnectParts`] trait.
//!
//! You can also implement the [`FromConnectParts`] trait for your own types.
//! See the [`extract`](crate::extract) module doc for more details on available extractors.
//!
//! Handlers _must_ be async.
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
//! Middlewares must be async and can be chained.
//! They are defined with the [`ConnectMiddleware`] trait which is automatically implemented for any
//! closure with up to 16 arguments with the following signature:
//! * `async FnOnce(*args) -> Result<(), E> where E: Display`
//!
//! Arguments must implement the [`FromConnectParts`] trait in the exact same way than handlers.
//!
//! ## Example with async closures
//! ```rust
//! # use socketioxide::SocketIo;
//! # use socketioxide::extract::*;
//! let (svc, io) = SocketIo::new_svc();
//! // Here the handler is async and extract the current socket and the auth payload
//! io.ns("/", async |io: SocketIo, s: SocketRef, TryData(auth): TryData<String>| {
//!     println!("Socket connected on / namespace with id and auth data: {} {:?}", s.id, auth);
//! });
//! // Here the handler is async and only extract the current socket.
//! // The auth payload won't be deserialized and will be dropped
//! io.ns("/async_nsp", async |s: SocketRef| {
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
//! async fn handler(s: SocketRef) {
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
//! async fn middleware(s: SocketRef, Data(token): Data<String>) -> Result<(), AuthError> {
//!     println!("second middleware called");
//!     if token != "secret" {
//!         Err(AuthError)
//!     } else {
//!         Ok(())
//!     }
//! }
//!
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

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::{adapter::Adapter, socket::Socket};
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
#[diagnostic::on_unimplemented(
    note = "Function argument is not a valid socketio extractor.
See `https://docs.rs/socketioxide/latest/socketioxide/extract/index.html` for details",
    label = "Invalid extractor"
)]
pub trait FromConnectParts<A: Adapter>: Sized {
    /// The error type returned by the extractor
    type Error: std::error::Error + Send + 'static;

    /// Extract the arguments from the connect event.
    /// If it fails, the handler is not called
    fn from_connect_parts(s: &Arc<Socket<A>>, auth: &Option<Value>) -> Result<Self, Self::Error>;
}

/// Define a handler for the connect event.
/// It is implemented for closures with up to 16 arguments. They must implement the [`FromConnectParts`] trait.
///
/// * See the [`connect`](super::connect) module doc for more details on connect handler.
/// * See the [`extract`](crate::extract) module doc for more details on available extractors.
#[diagnostic::on_unimplemented(
    note = "This function is not a ConnectHandler. Check that:
* It is a clonable async `FnOnce` that returns nothing.
* All its arguments are valid connect extractors.
* If you use a custom adapter, it must be generic over the adapter type.
See `https://docs.rs/socketioxide/latest/socketioxide/extract/index.html` for details.\n",
    label = "Invalid ConnectHandler"
)]
pub trait ConnectHandler<A: Adapter, T>: Sized + Clone + Send + Sync + 'static {
    /// Call the handler with the given arguments.
    fn call(&self, s: Arc<Socket<A>>, auth: Option<Value>);

    type MiddlewareError: std::fmt::Display + Send + 'static;
    /// Call the middleware with the given arguments.
    fn call_middleware<'a>(
        &'a self,
        _: Arc<Socket<A>>,
        _: &'a Option<Value>,
    ) -> impl Future<Output = Result<(), Self::MiddlewareError>> + Send + 'a {
        async move { Ok(()) }
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
    /// async fn handler(s: SocketRef) {
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
    /// async fn middleware(s: SocketRef, Data(token): Data<String>) -> Result<(), AuthError> {
    ///     println!("second middleware called");
    ///     if token != "secret" {
    ///         Err(AuthError)
    ///     } else {
    ///         Ok(())
    ///     }
    /// }
    ///
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
        M: ConnectHandler<A, T1> + Send + Sync + 'static,
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
        let fut = self.handler.call_middleware(s, auth);
        Box::pin(async move { fut.await.map_err(|e| Box::new(e) as _) })
    }

    fn boxed_clone(&self) -> BoxedConnectHandler<A> {
        Box::new(self.clone())
    }
}

#[diagnostic::do_not_recommend]
impl<A, H, M, T, T1> ConnectHandler<A, T> for LayeredConnectHandler<A, H, M, T, T1>
where
    A: Adapter,
    H: ConnectHandler<A, T> + Send + Sync + 'static,
    M: ConnectHandler<A, T1> + Send + Sync + 'static,
    T: Send + Sync + 'static,
    T1: Send + Sync + 'static,
{
    fn call(&self, s: Arc<Socket<A>>, auth: Option<Value>) {
        self.handler.call(s, auth);
    }

    type MiddlewareError = M::MiddlewareError;
    fn call_middleware<'a>(
        &'a self,
        s: Arc<Socket<A>>,
        auth: &'a Option<Value>,
    ) -> impl Future<Output = Result<(), Self::MiddlewareError>> + Send + 'a {
        self.middleware.call_middleware(s, auth)
    }

    fn with<M2, T2>(self, next: M2) -> impl ConnectHandler<A, T>
    where
        M2: ConnectHandler<A, T2> + Send + Sync + 'static,
        T2: Send + Sync + 'static,
    {
        let t = ConnectMiddlewareLayer {
            middleware: self.middleware,
            next,
            phantom: std::marker::PhantomData,
        };
        fn test<B: Adapter, G: Send + Sync + 'static>(t: &impl ConnectHandler<B, G>) {}
        test(&t);
        LayeredConnectHandler {
            handler: self.handler,
            middleware: t,
            phantom: std::marker::PhantomData,
        }
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

#[diagnostic::do_not_recommend]
impl<A, M, N, T, T1> ConnectHandler<A, T> for ConnectMiddlewareLayer<M, N, T, T1>
where
    A: Adapter,
    M: ConnectHandler<A, T> + Send + Sync + 'static,
    N: ConnectHandler<A, T1, MiddlewareError = M::MiddlewareError> + Send + Sync + 'static,
    T: Send + Sync + 'static,
    T1: Send + Sync + 'static,
{
    fn call(&self, _s: Arc<Socket<A>>, _auth: Option<Value>) {}

    type MiddlewareError = M::MiddlewareError;
    fn call_middleware<'a>(
        &'a self,
        s: Arc<Socket<A>>,
        auth: &'a Option<Value>,
    ) -> impl Future<Output = Result<(), Self::MiddlewareError>> + Send + 'a {
        Box::pin(async move {
            self.middleware.call_middleware(s.clone(), auth).await?;
            self.next.call_middleware(s, auth).await
        })
    }
}

macro_rules! impl_handler_async {
    (
        [$($ty:ident),*]
    ) => {
        #[allow(non_snake_case, unused)]
        #[diagnostic::do_not_recommend]
        impl<A, F, Fut, $($ty,)*> ConnectHandler<A, ($($ty,)*)> for F
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

            fn call_middleware(&self, s: Arc<Socket<A>>, auth: &Option<Value>) -> impl Future<Output = ()> {
                $(
                    let $ty = match $ty::from_connect_parts(&s, auth) {
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
