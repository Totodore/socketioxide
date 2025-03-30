#![allow(dead_code)]

use futures_core::Stream;
use socketioxide_core::Uid;
use socketioxide_mongodb::{
    drivers::{Driver, Item},
    CustomMongoDbAdapter, MongoDbAdapterConfig, MongoDbAdapterCtr,
};
use std::{
    convert::Infallible,
    pin::Pin,
    sync::{Arc, RwLock},
    task,
};
use tokio::sync::mpsc;

use socketioxide::{adapter::Emitter, SocketIo, SocketIoConfig};

/// Spawns a number of servers with a stub driver for testing.
/// Every server will be connected to every other server.
pub fn spawn_servers<const N: usize>() -> [SocketIo<CustomMongoDbAdapter<Emitter, StubDriver>>; N] {
    let sync_buff = Arc::new(RwLock::new(Vec::with_capacity(N)));

    [0; N].map(|_| {
        let server_id = Uid::new();
        let (driver, mut rx, tx) = StubDriver::new();

        // pipe messages to all other servers
        sync_buff.write().unwrap().push((server_id, tx));
        let sync_buff = sync_buff.clone();
        tokio::spawn(async move {
            while let Some((chan, data)) = rx.recv().await {
                tracing::debug!("received data to broadcast {:?}", chan);
                for (server_id, tx) in sync_buff.read().unwrap().iter() {
                    if chan.get_origin() != *server_id {
                        tracing::debug!("sending data for {:?}", chan);
                        tx.try_send((chan.clone(), data.clone())).unwrap();
                    }
                }
            }
        });

        let adapter = MongoDbAdapterCtr::new_with_driver(driver, MongoDbAdapterConfig::default());
        let mut config = SocketIoConfig::default();
        config.server_id = server_id;
        let (_svc, io) = SocketIo::builder()
            .with_config(config)
            .with_adapter::<CustomMongoDbAdapter<_, _>>(adapter)
            .build_svc();
        io
    })
}

type ResponseHandlers = Vec<(Uid, mpsc::Sender<Item>)>;
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
    while let Some((head, data)) = rx.recv().await {
        let handlers = handlers.read().unwrap();
        tracing::debug!(
            handlers = handlers.len(),
            "received data to broadcast {:?}",
            head
        );
        for (uid, handler) in &*handlers {
            if *uid != head.get_origin() {
                handler.try_send((head.clone(), data.clone())).unwrap();
            }
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

    async fn emit(&self, (head, data): &Item) -> Result<(), Self::Error> {
        self.tx.try_send((head.clone(), data.clone())).unwrap();
        Ok(())
    }
    async fn watch(&self, server_id: Uid) -> Result<Self::EvStream, Self::Error> {
        let (tx, rx) = mpsc::channel(255);
        self.handlers.write().unwrap().push((server_id, tx));
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
