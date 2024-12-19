use std::sync::{Arc, RwLock};

use socketioxide::adapter::Emitter;
use socketioxide::SocketIo;
use socketioxide_adapter_redis::drivers::test::StubDriver;
use socketioxide_adapter_redis::{RedisAdapter, RedisAdapterConfig, RedisAdapterState};

/// Spawns a number of servers with a stub driver for testing.
/// Every server will be connected to every other server.
pub fn spawn_servers<const N: usize>() -> [SocketIo<RedisAdapter<Emitter, StubDriver>>; N] {
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

        let driver = Arc::new(driver);
        let adapter = RedisAdapterState::new(driver.clone(), RedisAdapterConfig::default());
        let (_svc, io) = SocketIo::builder()
            .with_adapter::<RedisAdapter<_, _>>(adapter)
            .build_svc();
        io
    })
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
