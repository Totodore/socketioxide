use std::{fmt, sync::Arc};

use de::NsParamDeserializationError;
use serde::de::DeserializeOwned;

use crate::{
    adapter::Adapter,
    handler::connect::NsParamBuff,
    handler::{ConnectMiddleware, FromConnectParts},
    socket::Socket,
};

mod de;
#[cfg(feature = "extensions")]
pub use ns_param_ext::*;

/// An Extractor that deserialize the namespace path parameters of the namespace to the provided type.
///
/// This extractor only works for the [`ConnectHandler`](crate::handler::ConnectHandler)
/// and [`ConnectMiddleware`](crate::handler::ConnectMiddleware).
/// If you want to keep path parameters for later usage you can use the [`KeepNsParam`] middleware helper with the `extensions` feature enabled.
///
/// # Errors
///
/// In case of deserialization error, if the tracing feature is enabled, an error log will be emitted.
///
/// # Example
/// ```
/// # use socketioxide::{SocketIo, extract::{NsParam, SocketRef}};
/// #[derive(Debug, serde::Deserialize)]
/// struct Params {
///     id: String,
///     user_id: String
/// }
///
/// fn handler(socket: SocketRef, NsParam(params): NsParam<Params>) {
///     println!("new socket with params: {:?}", params);
/// }
/// let (_svc, io) = SocketIo::new_svc();
///
/// io.ns("/{id}/user/{user_id}", handler).unwrap();
/// ```
#[derive(Debug, Clone)]
pub struct NsParam<T>(pub T);
impl<A: Adapter, T: DeserializeOwned> FromConnectParts<A> for NsParam<T> {
    type Error = NsParamDeserializationError;
    fn from_connect_parts(
        _: &Arc<Socket<A>>,
        _: &Option<String>,
        params: &NsParamBuff<'_>,
    ) -> Result<Self, Self::Error> {
        let data: T = de::from_params(params)?;
        Ok(NsParam(data))
    }
}

#[cfg(feature = "extensions")]
#[cfg_attr(docsrs, doc(cfg(feature = "extensions")))]
mod ns_param_ext {
    use super::*;
    /// A middleware helper that save the namespace parameters in the socket extensions.
    /// You can the use then [`Extension`](crate::extract::Extension) extractor to get the deserialized ns parameters.
    ///
    /// # Errors
    ///
    /// In case of deserialization error, if the tracing feature is enabled, an error log will be emitted.
    ///
    /// # Example
    /// ```
    /// # use socketioxide::{SocketIo, extract::{Extension, KeepNsParam, SocketRef}, handler::ConnectHandler};
    /// #[derive(Debug, Clone, serde::Deserialize)]
    /// struct Params {
    ///     id: String,
    ///     user_id: String
    /// }
    ///
    /// fn handler(s: SocketRef) {
    ///     s.on("event", |Extension(params): Extension<Params>| {
    ///         println!("socket initialized with {:?} received new event", params);
    ///     })
    /// }
    /// let (_svc, io) = SocketIo::new_svc();
    ///
    /// io.ns("/{id}/user/{user_id}", handler.with(KeepNsParam::<Params>::new())).unwrap();
    /// ```
    #[derive(Debug, Default)]
    pub struct KeepNsParam<T: DeserializeOwned + Clone + Send + Sync + 'static>(
        std::marker::PhantomData<T>,
    );
    impl<T: DeserializeOwned + Clone + Send + Sync + 'static> KeepNsParam<T> {
        /// Create a new [`KeepNsParam`] middleware helper
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
            params: &'a NsParamBuff<'_>,
        ) -> impl futures_core::Future<Output = Result<(), Box<dyn fmt::Display + Send>>> + Send
        {
            if let Ok(param) = NsParam::<T>::from_connect_parts(&s, auth, params) {
                s.extensions.insert(param);
            } else {
                #[cfg(feature = "tracing")]
                tracing::warn!("Failed to extract namespace param from connect parts");
            }
            async move { Ok(()) }
        }
    }
    super::super::__impl_deref!(NsParam<T>: T);
}
