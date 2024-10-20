use std::convert::Infallible;
use std::sync::Arc;

use crate::adapter::Adapter;
use crate::handler::{FromConnectParts, FromDisconnectParts, FromMessageParts};
use crate::socket::{DisconnectReason, Socket};
use socketioxide_core::Value;

#[cfg(feature = "extensions")]
#[cfg_attr(docsrs, doc(cfg(feature = "extensions")))]
pub use extensions_extract::*;

/// It was impossible to find the given extension.
pub struct ExtensionNotFound<T>(std::marker::PhantomData<T>);

impl<T> std::fmt::Display for ExtensionNotFound<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Extension of type {} not found, maybe you forgot to insert it in the extensions map?",
            std::any::type_name::<T>()
        )
    }
}
impl<T> std::fmt::Debug for ExtensionNotFound<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ExtensionNotFound {}", std::any::type_name::<T>())
    }
}
impl<T> std::error::Error for ExtensionNotFound<T> {}

fn extract_http_extension<T: Clone + Send + Sync + 'static>(
    s: &Arc<Socket<impl Adapter>>,
) -> Result<T, ExtensionNotFound<T>> {
    s.req_parts()
        .extensions
        .get::<T>()
        .cloned()
        .ok_or(ExtensionNotFound(std::marker::PhantomData))
}

/// An Extractor that returns a clone extension from the request parts.
pub struct HttpExtension<T>(pub T);
/// An Extractor that returns a clone extension from the request parts if it exists.
pub struct MaybeHttpExtension<T>(pub Option<T>);

impl<A: Adapter, T: Clone + Send + Sync + 'static> FromConnectParts<A> for HttpExtension<T> {
    type Error = ExtensionNotFound<T>;
    fn from_connect_parts(
        s: &Arc<Socket<A>>,
        _: &Option<Value>,
    ) -> Result<Self, ExtensionNotFound<T>> {
        extract_http_extension(s).map(HttpExtension)
    }
}

impl<A: Adapter, T: Clone + Send + Sync + 'static> FromConnectParts<A> for MaybeHttpExtension<T> {
    type Error = Infallible;
    fn from_connect_parts(s: &Arc<Socket<A>>, _: &Option<Value>) -> Result<Self, Infallible> {
        Ok(MaybeHttpExtension(extract_http_extension(s).ok()))
    }
}

impl<A: Adapter, T: Clone + Send + Sync + 'static> FromDisconnectParts<A> for HttpExtension<T> {
    type Error = ExtensionNotFound<T>;
    fn from_disconnect_parts(
        s: &Arc<Socket<A>>,
        _: DisconnectReason,
    ) -> Result<Self, ExtensionNotFound<T>> {
        extract_http_extension(s).map(HttpExtension)
    }
}
impl<A: Adapter, T: Clone + Send + Sync + 'static> FromDisconnectParts<A>
    for MaybeHttpExtension<T>
{
    type Error = Infallible;
    fn from_disconnect_parts(s: &Arc<Socket<A>>, _: DisconnectReason) -> Result<Self, Infallible> {
        Ok(MaybeHttpExtension(extract_http_extension(s).ok()))
    }
}

impl<A: Adapter, T: Clone + Send + Sync + 'static> FromMessageParts<A> for HttpExtension<T> {
    type Error = ExtensionNotFound<T>;
    fn from_message_parts(
        s: &Arc<Socket<A>>,
        _: &mut Value,
        _: &Option<i64>,
    ) -> Result<Self, ExtensionNotFound<T>> {
        extract_http_extension(s).map(HttpExtension)
    }
}
impl<A: Adapter, T: Clone + Send + Sync + 'static> FromMessageParts<A> for MaybeHttpExtension<T> {
    type Error = Infallible;
    fn from_message_parts(
        s: &Arc<Socket<A>>,
        _: &mut Value,
        _: &Option<i64>,
    ) -> Result<Self, Infallible> {
        Ok(MaybeHttpExtension(extract_http_extension(s).ok()))
    }
}

super::__impl_deref!(HttpExtension);
super::__impl_deref!(MaybeHttpExtension<T>: Option<T>);

#[cfg(feature = "extensions")]
mod extensions_extract {
    use super::*;

    fn extract_extension<T: Clone + Send + Sync + 'static>(
        s: &Arc<Socket<impl Adapter>>,
    ) -> Result<T, ExtensionNotFound<T>> {
        s.extensions
            .get::<T>()
            .ok_or(ExtensionNotFound(std::marker::PhantomData))
    }

    /// An Extractor that returns the extension of the given type.
    /// If the extension is not found,
    /// the handler won't be called and an error log will be print if the `tracing` feature is enabled.
    ///
    /// You can use [`MaybeExtension`] if the extensions you are requesting _may_ not exists.
    pub struct Extension<T>(pub T);

    /// An Extractor that returns the extension of the given type T if it exists or [`None`] otherwise.
    pub struct MaybeExtension<T>(pub Option<T>);

    impl<A: Adapter, T: Clone + Send + Sync + 'static> FromConnectParts<A> for Extension<T> {
        type Error = ExtensionNotFound<T>;
        fn from_connect_parts(
            s: &Arc<Socket<A>>,
            _: &Option<Value>,
        ) -> Result<Self, ExtensionNotFound<T>> {
            extract_extension(s).map(Extension)
        }
    }
    impl<A: Adapter, T: Clone + Send + Sync + 'static> FromConnectParts<A> for MaybeExtension<T> {
        type Error = Infallible;
        fn from_connect_parts(s: &Arc<Socket<A>>, _: &Option<Value>) -> Result<Self, Infallible> {
            Ok(MaybeExtension(extract_extension(s).ok()))
        }
    }
    impl<A: Adapter, T: Clone + Send + Sync + 'static> FromDisconnectParts<A> for Extension<T> {
        type Error = ExtensionNotFound<T>;
        fn from_disconnect_parts(
            s: &Arc<Socket<A>>,
            _: DisconnectReason,
        ) -> Result<Self, ExtensionNotFound<T>> {
            extract_extension(s).map(Extension)
        }
    }
    impl<A: Adapter, T: Clone + Send + Sync + 'static> FromDisconnectParts<A> for MaybeExtension<T> {
        type Error = Infallible;
        fn from_disconnect_parts(
            s: &Arc<Socket<A>>,
            _: DisconnectReason,
        ) -> Result<Self, Infallible> {
            Ok(MaybeExtension(extract_extension(s).ok()))
        }
    }
    impl<A: Adapter, T: Clone + Send + Sync + 'static> FromMessageParts<A> for Extension<T> {
        type Error = ExtensionNotFound<T>;
        fn from_message_parts(
            s: &Arc<Socket<A>>,
            _: &mut Value,
            _: &Option<i64>,
        ) -> Result<Self, ExtensionNotFound<T>> {
            extract_extension(s).map(Extension)
        }
    }
    impl<A: Adapter, T: Clone + Send + Sync + 'static> FromMessageParts<A> for MaybeExtension<T> {
        type Error = Infallible;
        fn from_message_parts(
            s: &Arc<Socket<A>>,
            _: &mut Value,
            _: &Option<i64>,
        ) -> Result<Self, Infallible> {
            Ok(MaybeExtension(extract_extension(s).ok()))
        }
    }
    super::super::__impl_deref!(Extension);
    super::super::__impl_deref!(MaybeExtension<T>: Option<T>);
}
