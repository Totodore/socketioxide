use futures_core::{Future, Stream};

#[cfg(feature = "mongodb")]
mod mongodb;

/// The driver trait can be used to support different MongoDB backends.
/// It must share handlers/connection between its clones.
pub trait Driver: Clone + Send + Sync + 'static {
    /// The error type for the driver.
    type Error: std::error::Error + Send + 'static;
    type EvStream: Stream<Item = Result<Vec<u8>, Self::Error>> + Send + 'static;

    fn watch(&self) -> impl Future<Output = Result<Self::EvStream, Self::Error>> + Send;

    fn emit(&self, data: &Vec<u8>) -> impl Future<Output = Result<(), Self::Error>> + Send;
}
