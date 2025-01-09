#![allow(dead_code)]

use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, RwLock},
    time::Duration,
};
use tokio::sync::mpsc;

use socketioxide::{adapter::Emitter, SocketIo};
use socketioxide_redis::{
    drivers::{Driver, MessageStream},
    CustomRedisAdapter, RedisAdapterConfig, RedisAdapterCtr,
};

/// Spawns a number of servers with a stub driver for testing.
/// Every server will be connected to every other server.
pub fn spawn_servers<const N: usize>() -> [SocketIo<CustomRedisAdapter<Emitter, StubDriver>>; N] {
    let sync_buff = Arc::new(RwLock::new(Vec::with_capacity(N)));

    [0; N].map(|_| {
        let (driver, mut rx, tx) = StubDriver::new(N as u16);

        // pipe messages to all other servers
        sync_buff.write().unwrap().push(tx);
        let sync_buff = sync_buff.clone();
        tokio::spawn(async move {
            while let Some((chan, data)) = rx.recv().await {
                tracing::debug!("received data to broadcast {}", chan);
                for tx in sync_buff.read().unwrap().iter() {
                    tracing::debug!("sending data for {}", chan);
                    tx.try_send((chan.clone(), data.clone())).unwrap();
                }
            }
        });

        let adapter = RedisAdapterCtr::new_with_driver(driver, RedisAdapterConfig::default());
        let (_svc, io) = SocketIo::builder()
            .with_adapter::<CustomRedisAdapter<_, _>>(adapter)
            .build_svc();
        io
    })
}

/// Spawns a number of servers with a stub driver for testing.
/// The internal server count is set to N + 2 to trigger a timeout when expecting N responses.
pub fn spawn_buggy_servers<const N: usize>(
    timeout: Duration,
) -> [SocketIo<CustomRedisAdapter<Emitter, StubDriver>>; N] {
    let sync_buff = Arc::new(RwLock::new(Vec::with_capacity(N)));

    [0; N].map(|_| {
        let (driver, mut rx, tx) = StubDriver::new(N as u16 + 2); // Fake server count to trigger timeout

        // pipe messages to all other servers
        sync_buff.write().unwrap().push(tx);
        let sync_buff = sync_buff.clone();
        tokio::spawn(async move {
            while let Some((chan, data)) = rx.recv().await {
                tracing::debug!("received data to broadcast {}", chan);
                for tx in sync_buff.read().unwrap().iter() {
                    tracing::debug!("sending data for {}", chan);
                    tx.try_send((chan.clone(), data.clone())).unwrap();
                }
            }
        });

        let config = RedisAdapterConfig::new().with_request_timeout(timeout);
        let adapter = RedisAdapterCtr::new_with_driver(driver, config);
        let (_svc, io) = SocketIo::builder()
            .with_adapter::<CustomRedisAdapter<_, _>>(adapter)
            .build_svc();
        io
    })
}

type ChanItem = (String, Vec<u8>);
type ResponseHandlers = HashMap<String, mpsc::Sender<ChanItem>>;
#[derive(Debug, Clone)]
pub struct StubDriver {
    tx: mpsc::Sender<ChanItem>,
    handlers: Arc<RwLock<ResponseHandlers>>,
    num_serv: u16,
}

async fn pipe_handlers(mut rx: mpsc::Receiver<ChanItem>, handlers: Arc<RwLock<ResponseHandlers>>) {
    while let Some((chan, data)) = rx.recv().await {
        let handlers = handlers.read().unwrap();
        tracing::debug!(?handlers, "received data to broadcast {}", chan);
        if let Some(tx) = handlers.get(&chan) {
            tx.try_send((chan, data)).unwrap();
        }
    }
}
impl StubDriver {
    pub fn new(num_serv: u16) -> (Self, mpsc::Receiver<ChanItem>, mpsc::Sender<ChanItem>) {
        let (tx, rx) = mpsc::channel(255); // driver emitter
        let (tx1, rx1) = mpsc::channel(255); // driver receiver
        let handlers = Arc::new(RwLock::new(HashMap::new()));

        tokio::spawn(pipe_handlers(rx1, handlers.clone()));

        let driver = Self {
            tx,
            num_serv,
            handlers,
        };
        (driver, rx, tx1)
    }
    pub fn handler_cnt(&self) -> usize {
        self.handlers.read().unwrap().len()
    }
}

impl Driver for StubDriver {
    type Error = std::convert::Infallible;

    fn publish(
        &self,
        chan: String,
        val: Vec<u8>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        self.tx.try_send((chan, val)).unwrap();
        async move { Ok(()) }
    }

    async fn subscribe(
        &self,
        pat: String,
        _: usize,
    ) -> Result<MessageStream<ChanItem>, Self::Error> {
        let (tx, rx) = mpsc::channel(255);
        self.handlers.write().unwrap().insert(pat, tx);
        Ok(MessageStream::new(rx))
    }

    async fn unsubscribe(&self, pat: String) -> Result<(), Self::Error> {
        self.handlers.write().unwrap().remove(&pat);
        Ok(())
    }

    async fn num_serv(&self, _chan: &str) -> Result<u16, Self::Error> {
        Ok(self.num_serv)
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
