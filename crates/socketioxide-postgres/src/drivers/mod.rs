use futures_core::Stream;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::sync::mpsc;

// mod postgres;
mod sqlx;

pub type ChanItem = (String, String);

#[derive(Deserialize)]
pub struct Item {}

/// The driver trait can be used to support different LISTEN/NOTIFY backends.
/// It must share handlers/connection between its clones.
pub trait Driver: Clone + Send + Sync + 'static {
    type Error: std::error::Error + Send + 'static;
    type NotifStream: Stream<Item = Self::Notification> + Send + 'static;
    type Notification: Notification;

    fn init(&self, table: &str, channels: &[&str])
    -> impl Future<Output = Result<(), Self::Error>>;

    fn listen<T: DeserializeOwned + 'static>(
        &self,
        channel: &str,
    ) -> impl Future<Output = Result<Self::NotifStream, Self::Error>> + Send;

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

pin_project_lite::pin_project! {
    pub struct NotifStream<T> {
        #[pin]
        rx: mpsc::UnboundedReceiver<T>,
    }
}
impl<T: Notification> Stream for NotifStream<T> {
    type Item = T;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<T>> {
        match self.rx.poll_recv(cx) {
            std::task::Poll::Ready(notif) => std::task::Poll::Ready(notif),
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }
}
impl<T> NotifStream<T> {
    pub fn new(rx: mpsc::UnboundedReceiver<T>) -> Self {
        NotifStream { rx }
    }
}
