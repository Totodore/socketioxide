#![allow(dead_code)]

use futures_core::Stream;
use socketioxide_core::Uid;
use socketioxide_postgres::{
    CustomPostgresAdapter, PostgresAdapterConfig, PostgresAdapterCtr,
    drivers::{Driver, Notification},
};
use std::{
    convert::Infallible,
    pin::Pin,
    str::FromStr,
    sync::{Arc, RwLock},
    task,
    time::Duration,
};
use tokio::sync::mpsc;

use socketioxide::{SocketIo, SocketIoConfig, adapter::Emitter};

/// Spawns a number of servers with a stub driver for testing.
/// Every server will be connected to every other server.
pub fn spawn_servers<const N: usize>() -> [SocketIo<CustomPostgresAdapter<Emitter, StubDriver>>; N]
{
    let sync_buff = Arc::new(RwLock::new(Vec::with_capacity(N)));
    spawn_inner(sync_buff, PostgresAdapterConfig::default())
}

pub fn spawn_buggy_servers<const N: usize>(
    timeout: Duration,
) -> [SocketIo<CustomPostgresAdapter<Emitter, StubDriver>>; N] {
    let sync_buff = Arc::new(RwLock::new(Vec::with_capacity(N)));
    let config = PostgresAdapterConfig::default().with_request_timeout(timeout);
    let res = spawn_inner(sync_buff.clone(), config);

    // Reinject a false heartbeat request to simulate a bad number of servers.
    // This will trigger timeouts when expecting responses from all servers.
    // The heartbeat type is 20 (RequestTypeOut::Heartbeat) in the wire format.
    let uid: Uid = Uid::from_str("PHHq01ObWy7Godqx").unwrap();
    let heartbeat_json = serde_json::json!({
        "node_id": uid.to_string(),
        "id": "ZG9K1r7xSLBiJYWD",
        "type": 20,
        "opts": null,
    });
    let payload = serde_json::to_string(&heartbeat_json).unwrap();

    for (_, tx) in sync_buff.read().unwrap().iter() {
        // Send the heartbeat to the global channel of the "/" namespace
        tx.try_send(StubNotification {
            channel: "socket.io#/".to_string(),
            payload: payload.clone(),
        })
        .unwrap();
    }

    res
}

fn spawn_inner<const N: usize>(
    sync_buff: Arc<RwLock<NotifyHandlers>>,
    config: PostgresAdapterConfig,
) -> [SocketIo<CustomPostgresAdapter<Emitter, StubDriver>>; N] {
    [0; N].map(|_| {
        let server_id = Uid::new();
        let (driver, mut rx, tx) = StubDriver::new(server_id);

        // pipe messages to all other servers
        sync_buff.write().unwrap().push((server_id, tx));
        let sync_buff = sync_buff.clone();
        tokio::spawn(async move {
            while let Some(notif) = rx.recv().await {
                tracing::debug!("received notify on channel {:?}", notif.channel);
                for (sid, tx) in sync_buff.read().unwrap().iter() {
                    if *sid != server_id {
                        tracing::debug!("forwarding notify to server {:?}", sid);
                        tx.try_send(notif.clone()).unwrap();
                    }
                }
            }
        });

        let adapter = PostgresAdapterCtr::new_with_driver(driver, config.clone());
        let mut config = SocketIoConfig::default();
        config.server_id = server_id;
        let (_svc, io) = SocketIo::builder()
            .with_config(config)
            .with_adapter::<CustomPostgresAdapter<_, _>>(adapter)
            .build_svc();
        io
    })
}

type NotifyHandlers = Vec<(Uid, mpsc::Sender<StubNotification>)>;

#[derive(Debug, Clone)]
pub struct StubNotification {
    channel: String,
    payload: String,
}

impl Notification for StubNotification {
    fn channel(&self) -> &str {
        &self.channel
    }

    fn payload(&self) -> &str {
        &self.payload
    }
}

#[derive(Debug, Clone)]
pub struct StubDriver {
    server_id: Uid,
    /// Sender to emit outgoing NOTIFY messages (to be broadcast to other servers).
    tx: mpsc::Sender<StubNotification>,
    /// Handlers for incoming notifications per listened channel.
    handlers: Arc<RwLock<Vec<(String, mpsc::Sender<StubNotification>)>>>,
}

impl StubDriver {
    pub fn new(
        server_id: Uid,
    ) -> (Self, mpsc::Receiver<StubNotification>, mpsc::Sender<StubNotification>) {
        let (tx, rx) = mpsc::channel(255); // outgoing notifies
        let (tx1, rx1) = mpsc::channel(255); // incoming notifies
        let handlers: Arc<RwLock<Vec<(String, mpsc::Sender<StubNotification>)>>> =
            Arc::new(RwLock::new(Vec::new()));

        tokio::spawn(pipe_handlers(rx1, handlers.clone()));

        let driver = Self {
            server_id,
            tx,
            handlers,
        };
        (driver, rx, tx1)
    }
}

/// Pipe incoming notifications to the matching channel handlers.
async fn pipe_handlers(
    mut rx: mpsc::Receiver<StubNotification>,
    handlers: Arc<RwLock<Vec<(String, mpsc::Sender<StubNotification>)>>>,
) {
    while let Some(notif) = rx.recv().await {
        let handlers = handlers.read().unwrap();
        for (chan, handler) in &*handlers {
            if *chan == notif.channel {
                handler.try_send(notif.clone()).unwrap();
            }
        }
    }
}

pin_project_lite::pin_project! {
    pub struct NotificationStream {
        #[pin]
        rx: mpsc::Receiver<StubNotification>,
    }
}

impl Stream for NotificationStream {
    type Item = StubNotification;

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
    ) -> task::Poll<Option<Self::Item>> {
        self.project().rx.poll_recv(cx)
    }
}

impl Driver for StubDriver {
    type Error = Infallible;
    type Notification = StubNotification;
    type NotificationStream = NotificationStream;

    async fn init(&self, _table: &str) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn listen(&self, channels: &[&str]) -> Result<Self::NotificationStream, Self::Error> {
        let (tx, rx) = mpsc::channel(255);
        let mut handlers = self.handlers.write().unwrap();
        for chan in channels {
            handlers.push((chan.to_string(), tx.clone()));
        }
        Ok(NotificationStream { rx })
    }

    async fn notify(&self, channel: &str, message: &str) -> Result<(), Self::Error> {
        // Also deliver to local handlers (self-delivery, like real PG NOTIFY).
        {
            let handlers = self.handlers.read().unwrap();
            for (chan, handler) in &*handlers {
                if *chan == channel {
                    handler
                        .try_send(StubNotification {
                            channel: channel.to_string(),
                            payload: message.to_string(),
                        })
                        .unwrap();
                }
            }
        }
        // Send to the broadcast pipe for delivery to other servers.
        self.tx
            .try_send(StubNotification {
                channel: channel.to_string(),
                payload: message.to_string(),
            })
            .unwrap();
        Ok(())
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
