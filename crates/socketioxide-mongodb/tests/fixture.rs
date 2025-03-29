#![allow(dead_code)]

use futures_core::Stream;
use socketioxide_mongodb::{
    drivers::{Driver, Item},
    CustomMongoDbAdapter, MongoDbAdapterConfig, MongoDbAdapterCtr,
};
use std::{
    convert::Infallible,
    pin::Pin,
    sync::{Arc, RwLock},
    task,
    time::Duration,
};
use tokio::sync::mpsc;

use socketioxide::{adapter::Emitter, SocketIo};

/// Spawns a number of servers with a stub driver for testing.
/// Every server will be connected to every other server.
pub fn spawn_servers<const N: usize>() -> [SocketIo<CustomMongoDbAdapter<Emitter, StubDriver>>; N] {
    let sync_buff = Arc::new(RwLock::new(Vec::with_capacity(N)));

    [0; N].map(|_| {
        let (driver, mut rx, tx) = StubDriver::new();

        // pipe messages to all other servers
        sync_buff.write().unwrap().push(tx);
        let sync_buff = sync_buff.clone();
        tokio::spawn(async move {
            while let Some((chan, data)) = rx.recv().await {
                tracing::debug!("received data to broadcast {:?}", chan);
                for tx in sync_buff.read().unwrap().iter() {
                    tracing::debug!("sending data for {:?}", chan);
                    tx.try_send((chan.clone(), data.clone())).unwrap();
                }
            }
        });

        let adapter = MongoDbAdapterCtr::new_with_driver(driver, MongoDbAdapterConfig::default());
        let (_svc, io) = SocketIo::builder()
            .with_adapter::<CustomMongoDbAdapter<_, _>>(adapter)
            .build_svc();
        io
    })
}

/// Spawns a number of servers with a stub driver for testing.
/// The internal server count is set to N + 2 to trigger a timeout when expecting N responses.
pub fn spawn_buggy_servers<const N: usize>(
    timeout: Duration,
) -> [SocketIo<CustomMongoDbAdapter<Emitter, StubDriver>>; N] {
    let sync_buff = Arc::new(RwLock::new(Vec::with_capacity(N)));

    [0; N].map(|_| {
        let (driver, mut rx, tx) = StubDriver::new(); // Fake server count to trigger timeout

        // pipe messages to all other servers
        sync_buff.write().unwrap().push(tx);
        let sync_buff = sync_buff.clone();
        tokio::spawn(async move {
            while let Some((item_id, data)) = rx.recv().await {
                tracing::debug!("received data to broadcast {:?}", item_id);
                for tx in sync_buff.read().unwrap().iter() {
                    tracing::debug!("sending data for {:?}", item_id);
                    tx.try_send((item_id.clone(), data.clone())).unwrap();
                }
            }
        });

        let config = MongoDbAdapterConfig::new().with_request_timeout(timeout);
        let adapter = MongoDbAdapterCtr::new_with_driver(driver, config);
        let (_svc, io) = SocketIo::builder()
            .with_adapter::<CustomMongoDbAdapter<_, _>>(adapter)
            .build_svc();
        io
    })
}

type ResponseHandlers = Vec<mpsc::Sender<Item>>;
#[derive(Debug, Clone)]
pub struct StubDriver {
    tx: mpsc::Sender<Item>,
    handlers: Arc<RwLock<ResponseHandlers>>,
}

impl StubDriver {
    pub fn new() -> (Self, mpsc::Receiver<Item>, mpsc::Sender<Item>) {
        let (tx, rx) = mpsc::channel(255); // driver emitter
        let (tx1, rx1) = mpsc::channel(255); // driver receiver
        let handlers = Arc::new(RwLock::new(Vec::new()));

        tokio::spawn(pipe_handlers(rx1, handlers.clone()));

        let driver = Self { tx, handlers };
        (driver, rx, tx1)
    }
}

async fn pipe_handlers(mut rx: mpsc::Receiver<Item>, handlers: Arc<RwLock<ResponseHandlers>>) {
    while let Some((chan, data)) = rx.recv().await {
        let handlers = handlers.read().unwrap();
        tracing::debug!(?handlers, "received data to broadcast {:?}", chan);
        for handler in &*handlers {
            handler.try_send((chan.clone(), data.clone())).unwrap();
        }
    }
}

pin_project_lite::pin_project! {
    pub struct StreamWrapper {
        #[pin]
        rx: mpsc::Receiver<Item>,
    }
}

impl Stream for StreamWrapper {
    type Item = Result<Item, Infallible>;

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
    ) -> task::Poll<Option<Self::Item>> {
        self.project().rx.poll_recv(cx).map(|el| el.map(Ok))
    }
}

impl Driver for StubDriver {
    type Error = Infallible;
    type EvStream = StreamWrapper;

    async fn emit(&self, data: &Item) -> Result<(), Self::Error> {
        for handler in &*self.handlers.read().unwrap() {
            handler.try_send(data.clone()).unwrap();
        }
        Ok(())
    }
    async fn watch(&self) -> Result<Self::EvStream, Self::Error> {
        let (tx, rx) = mpsc::channel(255);
        self.handlers.write().unwrap().push(tx);
        Ok(StreamWrapper { rx })
    }
}

#[macro_export]
macro_rules! timeout_rcv_err {
    ($srx:expr) => {
        tokio::time::timeout(std::time::Duration::from_millis(10), $srx.recv())
            .await
            .unwrap_err();
    };
}

#[macro_export]
macro_rules! timeout_rcv {
    ($srx:expr) => {
        TryInto::<String>::try_into(
            tokio::time::timeout(std::time::Duration::from_millis(10), $srx.recv())
                .await
                .unwrap()
                .unwrap(),
        )
        .unwrap()
    };
    ($srx:expr, $t:expr) => {
        TryInto::<String>::try_into(
            tokio::time::timeout(std::time::Duration::from_millis($t), $srx.recv())
                .await
                .unwrap()
                .unwrap(),
        )
        .unwrap()
    };
}
