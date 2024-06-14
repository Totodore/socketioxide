use std::{fmt, sync::Arc};

use de::NsParamDeserializationError;
use serde::de::DeserializeOwned;
use smallvec::SmallVec;

use crate::{
    adapter::Adapter,
    handler::{ConnectMiddleware, FromConnectParts},
    socket::Socket,
};

mod de;

/// A buffer that holds the namespace parameters.
/// It should not be used directly, use the [`NsParam`] extractor instead.
#[derive(Debug, Default)]
pub struct NsParamBuff<'a>(SmallVec<[(String, &'a str); 3]>);
impl<'k, 'v> From<matchit::Params<'k, 'v>> for NsParamBuff<'v> {
    fn from(params: matchit::Params<'k, 'v>) -> Self {
        let mut vec = SmallVec::new();
        for (k, v) in params.iter() {
            vec.push((k.to_string(), v));
        }
        Self(vec)
    }
}
impl<'a, 'v> IntoIterator for &'a NsParamBuff<'v> {
    type Item = &'a (String, &'v str);
    type IntoIter = std::slice::Iter<'a, (String, &'v str)>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

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
        params: &'a NsParamBuff<'_>,
    ) -> impl futures_core::Future<Output = Result<(), Box<dyn fmt::Display + Send>>> + Send {
        if let Ok(param) = NsParam::<T>::from_connect_parts(&s, auth, params) {
            s.extensions.insert(param);
        } else {
            #[cfg(feature = "tracing")]
            tracing::warn!("Failed to extract namespace param from connect parts");
        }
        async move { Ok(()) }
    }
}
super::__impl_deref!(NsParam<T>: T);
