//! All the adapters of a server share the same driver, and so the same connection: closing
//! the adapter of a namespace must only stop listening to the channels of this namespace.

use std::{
    collections::HashSet,
    convert::Infallible,
    pin::Pin,
    sync::{Arc, Mutex},
    task,
    time::Duration,
};

use futures_core::Stream;
use socketioxide::{SocketIo, adapter::Emitter};
use socketioxide_postgres::{
    CustomPostgresAdapter, PostgresAdapterConfig, PostgresAdapterCtr,
    drivers::{Driver, Notification},
};

#[derive(Debug, Clone, Default)]
struct StubDriver {
    /// The channels passed to each `listen` call.
    listened: Arc<Mutex<Vec<HashSet<String>>>>,
    /// The channels passed to each `close` call.
    closed: Arc<Mutex<Vec<HashSet<String>>>>,
}

struct StubNotification;
impl Notification for StubNotification {
    fn channel(&self) -> &str {
        ""
    }
    fn payload(&self) -> &str {
        ""
    }
}

struct PendingStream;
impl Stream for PendingStream {
    type Item = StubNotification;
    fn poll_next(
        self: Pin<&mut Self>,
        _: &mut task::Context<'_>,
    ) -> task::Poll<Option<Self::Item>> {
        task::Poll::Pending
    }
}

fn to_set(channels: &[&str]) -> HashSet<String> {
    channels.iter().map(ToString::to_string).collect()
}

impl Driver for StubDriver {
    type Error = Infallible;
    type Notification = StubNotification;
    type NotificationStream = PendingStream;

    async fn init(&self, _: &str) -> Result<(), Self::Error> {
        Ok(())
    }
    async fn listen(&self, channels: &[&str]) -> Result<Self::NotificationStream, Self::Error> {
        self.listened.lock().unwrap().push(to_set(channels));
        Ok(PendingStream)
    }
    async fn notify(&self, _: &str, _: &str) -> Result<(), Self::Error> {
        Ok(())
    }
    async fn push_attachment(&self, _: &str, _: &[u8]) -> Result<i64, Self::Error> {
        Ok(0)
    }
    async fn get_attachment(&self, _: &str, _: i64) -> Result<Vec<u8>, Self::Error> {
        Ok(vec![])
    }
    async fn cleanup_attachments(&self, _: &str, _: Duration) -> Result<(), Self::Error> {
        Ok(())
    }
    async fn close(&self, channels: &[&str]) -> Result<(), Self::Error> {
        self.closed.lock().unwrap().push(to_set(channels));
        Ok(())
    }
}

#[tokio::test]
async fn delete_ns_only_closes_its_own_channels() {
    let driver = StubDriver::default();
    let adapter =
        PostgresAdapterCtr::new_with_driver(driver.clone(), PostgresAdapterConfig::default());
    let (_svc, io): (_, SocketIo<CustomPostgresAdapter<Emitter, StubDriver>>) = SocketIo::builder()
        .with_adapter::<CustomPostgresAdapter<_, _>>(adapter)
        .build_svc();

    io.ns("/first", async || ()).await.unwrap();
    io.ns("/second", async || ()).await.unwrap();
    let (first, second) = {
        let listened = driver.listened.lock().unwrap();
        assert_eq!(listened.len(), 2);
        (listened[0].clone(), listened[1].clone())
    };
    assert!(first.is_disjoint(&second));

    io.delete_ns("/first");

    assert_eq!(*driver.closed.lock().unwrap(), vec![first]);
}
