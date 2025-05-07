use serde::de::DeserializeOwned;

mod postgres;
mod sqlx;

pub type ChanItem = (String, String);

/// The driver trait can be used to support different LISTEN/NOTIFY backends.
/// It must share handlers/connection between its clones.
pub trait Driver: Clone + Send + Sync + 'static {
    type Error: std::error::Error + Send + 'static;
    type NotifStream<T: DeserializeOwned + 'static>: futures_core::Stream<Item = T> + Send + 'static;

    fn init(&self, table: &str, channels: &[&str])
    -> impl Future<Output = Result<(), Self::Error>>;

    fn listen<T: DeserializeOwned + 'static>(
        &self,
        channel: &str,
    ) -> impl Future<Output = Result<Self::NotifStream<T>, Self::Error>>;

    fn notify(&self, channel: &str, message: &str)
    -> impl Future<Output = Result<(), Self::Error>>;
}
