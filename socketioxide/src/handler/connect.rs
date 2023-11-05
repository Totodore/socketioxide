use std::sync::Arc;

use futures::Future;
use serde::de::DeserializeOwned;

use crate::{adapter::Adapter, Socket};

use super::MakeErasedHandler;

/// A Type Erased [`NamespaceHandler`] so it can be stored in a HashMap
pub(crate) type BoxedNamespaceHandler<A> = Box<dyn ErasedNamespaceHandler<A>>;
pub(crate) trait ErasedNamespaceHandler<A: Adapter>: Send + Sync + 'static {
    fn call(&self, s: Arc<Socket<A>>, auth: Option<String>);
}

impl<A: Adapter, T, H> MakeErasedHandler<H, A, T>
where
    T: Send + Sync + 'static,
    H: NamespaceHandler<A, T> + Send + Sync + 'static,
{
    pub fn new_ns_boxed(inner: H) -> Box<dyn ErasedNamespaceHandler<A>> {
        Box::new(MakeErasedHandler::new(inner))
    }
}

impl<A: Adapter, T, H> ErasedNamespaceHandler<A> for MakeErasedHandler<H, A, T>
where
    H: NamespaceHandler<A, T> + Send + Sync + 'static,
    T: Send + Sync + 'static,
{
    #[inline(always)]
    fn call(&self, s: Arc<Socket<A>>, auth: Option<String>) {
        self.handler.call(s, auth);
    }
}

/// The Namespace Handler is implemented for functions with the following signatures:
/// ```ignore
/// fn(Arc<Socket<A>>) -> Fut + Send + Sync + 'static,
/// fn(Arc<Socket<A>>, T) -> Fut + Send + Sync + 'static,
/// fn(Arc<Socket<A>>, Result<T, serde_json::Error>) -> Fut + Send + Sync + 'static,
/// ```
/// Thanks to the multiple trait implementation, you can decide to extract the auth data or not
pub trait NamespaceHandler<A: Adapter, T>: Send + Sync + 'static {
    fn call(&self, s: Arc<Socket<A>>, auth: Option<String>);

    fn phantom(&self) -> std::marker::PhantomData<T> {
        std::marker::PhantomData
    }
}

impl<F, A, Fut> NamespaceHandler<A, ((),)> for F
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

impl<F, A, T, Fut> NamespaceHandler<A, ((), T)> for F
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

impl<F, A, T, Fut> NamespaceHandler<A, ((), (), T)> for F
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
