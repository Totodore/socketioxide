use futures_core::Stream;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

mod postgres;
mod sqlx;

pub type ChanItem = (String, String);

#[derive(Deserialize)]
pub struct Item {}

/// The driver trait can be used to support different LISTEN/NOTIFY backends.
/// It must share handlers/connection between its clones.
pub trait Driver: Clone + Send + Sync + 'static {
    type Error: std::error::Error + Send + 'static;
    type NotifStream<T: DeserializeOwned + 'static>: Stream<Item = T> + Send + 'static;

    fn init(&self, table: &str, channels: &[&str])
    -> impl Future<Output = Result<(), Self::Error>>;

    fn listen<T: DeserializeOwned + 'static>(
        &self,
        channel: &str,
    ) -> impl Future<Output = Result<Self::NotifStream<T>, Self::Error>> + Send;

    fn notify<T: Serialize + ?Sized>(
        &self,
        channel: &str,
        message: &T,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
}
