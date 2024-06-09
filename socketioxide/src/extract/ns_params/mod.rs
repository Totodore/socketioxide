use std::{convert::Infallible, fmt, sync::Arc};

use serde::de::DeserializeOwned;

use crate::{
    adapter::Adapter,
    handler::{ConnectMiddleware, FromConnectParts},
    socket::Socket,
};

mod de;

#[derive(Debug)]
pub struct NsParam<T>(pub T);
impl<A: Adapter, T: DeserializeOwned> FromConnectParts<A> for NsParam<T> {
    type Error = Infallible;
    fn from_connect_parts(
        _: &Arc<Socket<A>>,
        _: &Option<String>,
        params: &matchit::Params<'_, '_>,
    ) -> Result<Self, Infallible> {
        Ok(NsParam(de::from_params(params)))
    }
}

/// A middleware helper that save the namespace parameters in the socket extensions.
#[derive(Debug, Default)]
pub struct KeepNsParam<T>(std::marker::PhantomData<T>);
impl<T: DeserializeOwned + Clone + Send + Sync + 'static> KeepNsParam<T> {
    pub fn new() -> Self {
        Self(std::marker::PhantomData)
    }
}
impl<A: Adapter, T: DeserializeOwned + Clone + Send + Sync + 'static> ConnectMiddleware<A, ()>
    for KeepNsParam<T>
{
    fn call<'a>(
        &'a self,
        s: Arc<Socket<A>>,
        auth: &'a Option<String>,
        params: &'a matchit::Params<'_, '_>,
    ) -> impl futures_core::Future<Output = Result<(), Box<dyn std::fmt::Display + Send>>> + Send
    {
        if let Ok(NsParam(param)) = NsParam::<T>::from_connect_parts(&s, auth, params) {
            s.extensions.insert(param);
        } else {
            #[cfg(feature = "tracing")]
            tracing::warn!("Failed to extract namespace param from connect parts");
        }
        async move { Ok(()) }
    }
}
super::__impl_deref!(NsParam<T>: T);
