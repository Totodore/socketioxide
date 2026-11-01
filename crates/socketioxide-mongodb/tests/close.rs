//! Closing a namespace must stop the tasks of its adapter: they own the change stream and
//! they would otherwise keep writing heartbeats and applying requests forever.

use std::{
    collections::HashMap,
    convert::Infallible,
    pin::Pin,
    sync::{Arc, Mutex},
    task,
    time::Duration,
};

use futures_core::Stream;
use socketioxide::{SocketIo, adapter::Emitter};
use socketioxide_core::Uid;
use socketioxide_mongodb::{
    CustomMongoDbAdapter, MongoDbAdapterConfig, MongoDbAdapterCtr,
    drivers::{Driver, Item},
};
use tokio::sync::mpsc;

const HB_INTERVAL: Duration = Duration::from_millis(5);

/// A driver that records what each namespace does.
#[derive(Debug, Clone, Default)]
struct StubDriver {
    /// The number of items emitted by each namespace.
    emitted: Arc<Mutex<HashMap<String, usize>>>,
    /// The sending side of the change stream of each namespace.
    streams: Arc<Mutex<HashMap<String, mpsc::Sender<Item>>>>,
}

impl StubDriver {
    fn emitted(&self, ns: &str) -> usize {
        *self.emitted.lock().unwrap().get(ns).unwrap_or(&0)
    }
    fn stream(&self, ns: &str) -> mpsc::Sender<Item> {
        self.streams.lock().unwrap().get(ns).unwrap().clone()
    }
}

struct EvStream(mpsc::Receiver<Item>);
impl Stream for EvStream {
    type Item = Result<Item, Infallible>;
    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
    ) -> task::Poll<Option<Self::Item>> {
        self.0.poll_recv(cx).map(|item| item.map(Ok))
    }
}

impl Driver for StubDriver {
    type Error = Infallible;
    type EvStream = EvStream;

    async fn watch(&self, _: Uid, ns: &str) -> Result<Self::EvStream, Self::Error> {
        let (tx, rx) = mpsc::channel(1);
        self.streams.lock().unwrap().insert(ns.to_string(), tx);
        Ok(EvStream(rx))
    }

    async fn emit(&self, item: &Item) -> Result<(), Self::Error> {
        *self
            .emitted
            .lock()
            .unwrap()
            .entry(item.ns.clone())
            .or_default() += 1;
        Ok(())
    }
}

fn create_server(driver: StubDriver) -> SocketIo<CustomMongoDbAdapter<Emitter, StubDriver>> {
    let config = MongoDbAdapterConfig::default().with_hb_interval(HB_INTERVAL);
    let adapter = MongoDbAdapterCtr::new_with_driver(driver, config);
    let (_svc, io) = SocketIo::builder()
        .with_adapter::<CustomMongoDbAdapter<_, _>>(adapter)
        .build_svc();
    io
}

/// Wait for the heartbeat task of the namespace to emit at least one more item.
async fn wait_heartbeat(driver: &StubDriver, ns: &str) {
    let start = driver.emitted(ns);
    tokio::time::timeout(Duration::from_secs(1), async {
        while driver.emitted(ns) == start {
            tokio::time::sleep(HB_INTERVAL).await;
        }
    })
    .await
    .expect("the namespace should emit heartbeats");
}

#[tokio::test]
async fn delete_ns_stops_adapter_tasks() {
    let driver = StubDriver::default();
    let io = create_server(driver.clone());
    io.ns("/test", async || ()).await.unwrap();
    wait_heartbeat(&driver, "/test").await;
    let stream = driver.stream("/test");

    io.delete_ns("/test");

    // The change stream is dropped with the task owning it.
    tokio::time::timeout(Duration::from_secs(1), stream.closed())
        .await
        .expect("the change stream should be closed");

    // No more heartbeats.
    let emitted = driver.emitted("/test");
    tokio::time::sleep(HB_INTERVAL * 5).await;
    assert_eq!(driver.emitted("/test"), emitted);
}

#[tokio::test]
async fn delete_ns_does_not_stop_other_namespaces() {
    let driver = StubDriver::default();
    let io = create_server(driver.clone());
    io.ns("/test", async || ()).await.unwrap();
    io.ns("/other", async || ()).await.unwrap();
    wait_heartbeat(&driver, "/other").await;
    let stream = driver.stream("/other");

    io.delete_ns("/test");
    tokio::time::sleep(HB_INTERVAL * 2).await;

    assert!(!stream.is_closed());
    wait_heartbeat(&driver, "/other").await;
}
