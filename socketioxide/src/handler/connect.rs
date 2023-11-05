use std::sync::Arc;

use futures::Future;
use serde::de::DeserializeOwned;

use crate::{adapter::Adapter, Socket};

use super::MakeErasedHandler;

/// A Type Erased [`ConnectHandler`] so it can be stored in a HashMap
pub(crate) type BoxedConnectHandler<A> = Box<dyn ErasedConnectHandler<A>>;
pub(crate) trait ErasedConnectHandler<A: Adapter>: Send + Sync + 'static {
    fn call(&self, s: Arc<Socket<A>>, auth: Option<String>);
}

impl<A: Adapter, T, H> MakeErasedHandler<H, A, T>
where
    T: Send + Sync + 'static,
    H: ConnectHandler<A, T> + Send + Sync + 'static,
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
    #[inline(always)]
    fn call(&self, s: Arc<Socket<A>>, auth: Option<String>) {
        self.handler.call(s, auth);
    }
}

/// The [`ConnectHandler`] trait is implemented for functions with the following signatures:
/// ```ignore
/// fn(Arc<Socket<A>>) -> Fut + Send + Sync + 'static,
/// fn(Arc<Socket<A>>, T) -> Fut + Send + Sync + 'static,
/// fn(Arc<Socket<A>>, Result<T, serde_json::Error>) -> Fut + Send + Sync + 'static,
/// ```
/// Thanks to the multiple trait implementation, you can decide to extract the auth data or not
///
/// This handler is called when a new client connects to the server
pub trait ConnectHandler<A: Adapter, T>: Send + Sync + 'static {
    fn call(&self, s: Arc<Socket<A>>, auth: Option<String>);

    fn phantom(&self) -> std::marker::PhantomData<T> {
        std::marker::PhantomData
    }
}

impl<F, A, Fut> ConnectHandler<A, ((),)> for F
where
    F: Fn(Arc<Socket<A>>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + Sync + 'static,
    A: Adapter,
{
    fn call(&self, s: Arc<Socket<A>>, _: Option<String>) {
        let fut = self(s);
        tokio::spawn(fut);
    }
}

impl<F, A, T, Fut> ConnectHandler<A, ((), T)> for F
where
    T: DeserializeOwned + Send + Sync + 'static,
    F: Fn(Arc<Socket<A>>, T) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + Sync + 'static,

    A: Adapter,
{
    fn call(&self, s: Arc<Socket<A>>, auth: Option<String>) {
        let v: Result<T, serde_json::Error> = auth
            .map(|a| serde_json::from_str(&a))
            .unwrap_or(serde_json::from_str("{}"));

        if let Ok(v) = v {
            let fut = self(s, v);
            tokio::spawn(fut);
        }
    }
}

impl<F, A, T, Fut> ConnectHandler<A, ((), (), T)> for F
where
    T: DeserializeOwned + Send + Sync + 'static,
    F: Fn(Arc<Socket<A>>, Result<T, serde_json::Error>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + Sync + 'static,

    A: Adapter,
{
    fn call(&self, s: Arc<Socket<A>>, auth: Option<String>) {
        let v: Result<T, serde_json::Error> = auth
            .map(|a| serde_json::from_str(&a))
            .unwrap_or(serde_json::from_str("{}"));

        let fut = self(s, v);
        tokio::spawn(fut);
    }
}
