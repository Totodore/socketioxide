use futures_core::Stream;
use serde::Serialize;

pub mod sqlx;

/// The driver trait can be used to support different LISTEN/NOTIFY backends.
/// It must share handlers/connection between its clones.
pub trait Driver: Clone + Send + Sync + 'static {
    type Error: std::error::Error + Send + 'static;
    type Notification: Notification;
    type NotificationStream: Stream<Item = Self::Notification> + Send;

    fn init(&self, table: &str) -> impl Future<Output = Result<(), Self::Error>> + Send;

    fn listen(
        &self,
        channels: &[&str],
    ) -> impl Future<Output = Result<Self::NotificationStream, Self::Error>> + Send;

    fn notify<T: Serialize + ?Sized>(
        &self,
        channel: &str,
        message: &T,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
}

pub trait Notification: Send + 'static {
    fn channel(&self) -> &str;
    fn payload(&self) -> &str;
}
