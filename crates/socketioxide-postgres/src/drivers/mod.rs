//! Drivers are an abstraction over the PostgreSQL LISTEN/NOTIFY backend used by the adapter.
//! You can use the provided implementation or implement your own.

use futures_core::Stream;

/// A driver implementation for the [`sqlx`](https://docs.rs/sqlx) PostgreSQL backend.
#[cfg(feature = "sqlx")]
pub mod sqlx;

/// A driver implementation for the [`tokio-postgres`](https://docs.rs/tokio-postgres)
/// PostgreSQL backend.
#[cfg(feature = "tokio-postgres")]
pub mod tokio_postgres;

/// The driver trait can be used to support different LISTEN/NOTIFY backends.
/// It must share handlers/connection between its clones.
pub trait Driver: Clone + Send + Sync + 'static {
    /// The error type returned by the driver.
    type Error: std::error::Error + Send + 'static;
    /// The notification type yielded by the notification stream.
    type Notification: Notification;
    /// The stream of notifications returned by [`Driver::listen`].
    type NotificationStream: Stream<Item = Self::Notification> + Send;

    /// Initialize the driver. This is called once when the adapter is created.
    /// It should create the necessary tables or schema if needed.
    fn init(&self, table: &str) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Subscribe to the given NOTIFY channels and return a stream of notifications.
    fn listen(
        &self,
        channels: &[&str],
    ) -> impl Future<Output = Result<Self::NotificationStream, Self::Error>> + Send;

    /// Send a NOTIFY message on the given channel with the given payload.
    fn notify(
        &self,
        channel: &str,
        message: &str,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Push an attachment when deferring a NOTIFY message to the attachment table.
    fn push_attachment(
        &self,
        table: &str,
        attachment: &[u8],
    ) -> impl Future<Output = Result<i32, Self::Error>> + Send;

    fn get_attachment(
        &self,
        table: &str,
        id: i32,
    ) -> impl Future<Output = Result<Vec<u8>, Self::Error>> + Send;

    /// UNLISTEN from every channel.
    fn close(&self) -> impl Future<Output = Result<(), Self::Error>> + Send;
}

/// A trait representing a PostgreSQL NOTIFY notification.
pub trait Notification: Send + 'static {
    /// The channel name on which the notification was received.
    fn channel(&self) -> &str;
    /// The payload of the notification.
    fn payload(&self) -> &str;
}
