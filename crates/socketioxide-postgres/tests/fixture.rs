#![allow(dead_code)]

use futures_core::Stream;
use socketioxide_core::{
    Uid,
    adapter::remote_packet::{RequestOut, RequestTypeOut},
};
use socketioxide_postgres::{
    CustomPostgresAdapter, PostgresAdapterConfig, PostgresAdapterCtr,
    drivers::{Driver, Notification},
};
use std::{
    collections::{HashMap, HashSet},
    pin::Pin,
    str::FromStr,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, AtomicI64},
    },
    task,
    time::{Duration, Instant},
};
use tokio::sync::mpsc;

use socketioxide::{SocketIo, SocketIoConfig, adapter::Emitter};

/// Spawns a number of servers with a stub driver for testing.
/// Every server will be connected to every other server.
pub fn spawn_servers<const N: usize>() -> [SocketIo<CustomPostgresAdapter<Emitter, StubDriver>>; N]
{
    let sync_buff = Arc::new(RwLock::new(Vec::with_capacity(N)));
    let (ios, _handles) = spawn_inner(sync_buff, PostgresAdapterConfig::default());
    ios
}

/// Spawns `N` servers with a custom [`PostgresAdapterConfig`].
pub fn spawn_servers_with_config<const N: usize>(
    config: PostgresAdapterConfig,
) -> [SocketIo<CustomPostgresAdapter<Emitter, StubDriver>>; N] {
    let sync_buff = Arc::new(RwLock::new(Vec::with_capacity(N)));
    let (ios, _handles) = spawn_inner(sync_buff, config);
    ios
}

/// Spawns `N` servers with a custom config AND returns handles to each stub driver so tests
/// can assert on attachment-store state (how many rows pushed, which ids were fetched, etc.).
pub fn spawn_servers_with_handles<const N: usize>(
    config: PostgresAdapterConfig,
) -> (
    [SocketIo<CustomPostgresAdapter<Emitter, StubDriver>>; N],
    [StubDriver; N],
) {
    let sync_buff = Arc::new(RwLock::new(Vec::with_capacity(N)));
    spawn_inner(sync_buff, config)
}

/// Serialize a [`RequestOut`] in the same wire envelope the adapter emits for inline requests:
/// `{"Request": {"node_id": <uid>, "payload": <request>}}`.
///
/// `node_id` is the emitter id used by the receiver's loopback filter — pass an id distinct
/// from every real server spawned in the test, otherwise the packet will be dropped as a
/// loopback.
pub fn wrap_request(node_id: Uid, req: &RequestOut<'_>) -> String {
    let payload = serde_json::to_value(req).unwrap();
    serde_json::to_string(&serde_json::json!({
        "Request": { "node_id": node_id, "payload": payload },
    }))
    .unwrap()
}

pub fn spawn_buggy_servers<const N: usize>(
    timeout: Duration,
) -> [SocketIo<CustomPostgresAdapter<Emitter, StubDriver>>; N] {
    let sync_buff = Arc::new(RwLock::new(Vec::with_capacity(N)));
    let config = PostgresAdapterConfig::default().with_request_timeout(timeout);
    let (res, _handles) = spawn_inner(sync_buff.clone(), config);

    // Reinject a false heartbeat request to simulate a bad number of servers.
    // This will trigger timeouts when expecting responses from all servers.
    let uid: Uid = Uid::from_str("PHHq01ObWy7Godqx").unwrap();
    let heartbeat = RequestOut::new_empty(uid, RequestTypeOut::Heartbeat);
    let payload = wrap_request(uid, &heartbeat);

    for (_, tx) in sync_buff.read().unwrap().iter() {
        let hash = xxhash_rust::xxh3::xxh3_64("socket.io#/".as_bytes());
        let channel = format!("ch_{:x}", hash);
        // Send the heartbeat to the global channel of the "/" namespace
        tx.try_send(StubNotification {
            channel,
            payload: payload.clone(),
        })
        .unwrap();
    }

    res
}

fn spawn_inner<const N: usize>(
    sync_buff: Arc<RwLock<NotifyHandlers>>,
    config: PostgresAdapterConfig,
) -> (
    [SocketIo<CustomPostgresAdapter<Emitter, StubDriver>>; N],
    [StubDriver; N],
) {
    let attachments = Arc::new(RwLock::new(RemoteTable::default()));
    let (ios, handles) = [0; N]
        .map(|_| {
            let server_id = Uid::new();
            let (driver, mut rx, tx) = StubDriver::new(server_id, attachments.clone());

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

            let adapter = PostgresAdapterCtr::new_with_driver(driver.clone(), config.clone());
            let mut config = SocketIoConfig::default();
            config.server_id = server_id;
            let (_svc, io) = SocketIo::builder()
                .with_config(config)
                .with_adapter::<CustomPostgresAdapter<_, _>>(adapter)
                .build_svc();
            (io, driver)
        })
        .into_iter()
        .collect::<(Vec<_>, Vec<_>)>();

    (ios.try_into().unwrap(), handles.try_into().unwrap())
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

type Handlers = Vec<(String, mpsc::Sender<StubNotification>)>;

#[derive(Debug, Default)]
pub struct RemoteTable {
    table: HashMap<i64, Row>,
    idx: AtomicI64,
}
#[derive(Debug, Clone)]
pub struct Row {
    id: i64,
    data: Vec<u8>,
    created_at: Instant,
}
impl Row {
    fn new(id: i64, data: Vec<u8>) -> Self {
        Self {
            id,
            data,
            created_at: Instant::now(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct StubDriver {
    server_id: Uid,
    /// Sender to emit outgoing NOTIFY messages (to be broadcast to other servers).
    tx: mpsc::Sender<StubNotification>,
    /// Handlers for incoming notifications per listened channel.
    handlers: Arc<RwLock<Handlers>>,
    attachments: Arc<RwLock<RemoteTable>>,
    /// Ids passed to `get_attachment`, in call order. Used by tests to assert on fetch
    /// activity (e.g. that a loopback large NOTIFY does not trigger a fetch).
    fetched_ids: Arc<RwLock<Vec<i64>>>,
    /// Ids that, when fetched, return garbage bytes — triggers decode failure in the adapter.
    corrupt_ids: Arc<RwLock<HashSet<i64>>>,
    /// One-shot flag: if true, the next `get_attachment` returns a driver error.
    fail_next_get: Arc<AtomicBool>,
}

#[derive(Debug, thiserror::Error)]
pub enum StubError {
    #[error("injected stub driver failure")]
    Injected,
}

impl StubDriver {
    pub fn new(
        server_id: Uid,
        attachments: Arc<RwLock<RemoteTable>>,
    ) -> (
        Self,
        mpsc::Receiver<StubNotification>,
        mpsc::Sender<StubNotification>,
    ) {
        let (tx, rx) = mpsc::channel(255); // outgoing notifies
        let (tx1, rx1) = mpsc::channel(255); // incoming notifies
        let handlers: Arc<RwLock<Handlers>> = Arc::new(RwLock::new(Vec::new()));

        tokio::spawn(pipe_handlers(rx1, handlers.clone()));

        let driver = Self {
            server_id,
            tx,
            handlers,
            attachments,
            fetched_ids: Arc::new(RwLock::new(Vec::new())),
            corrupt_ids: Arc::new(RwLock::new(HashSet::new())),
            fail_next_get: Arc::new(AtomicBool::new(false)),
        };
        (driver, rx, tx1)
    }

    /// Number of attachment rows this server has written (i.e. sent via NOTIFY).
    pub fn push_count(&self) -> usize {
        self.attachments.read().unwrap().table.len()
    }
    /// Number of `get_attachment` calls this server has served.
    pub fn fetch_count(&self) -> usize {
        self.fetched_ids.read().unwrap().len()
    }
    /// All ids `get_attachment` was called with, in call order.
    pub fn fetched(&self) -> Vec<i64> {
        self.fetched_ids.read().unwrap().clone()
    }
    /// Mark an id as corrupt: next `get_attachment(id)` returns a garbage payload so the
    /// decoder fails. Exercises the silent-drop path.
    pub fn corrupt(&self, id: i64) {
        self.corrupt_ids.write().unwrap().insert(id);
    }
    /// One-shot: the next `get_attachment` call returns a driver error (triggers the
    /// attachment-resolution-failure path).
    pub fn fail_once(&self) {
        self.fail_next_get
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Pipe incoming notifications to the matching channel handlers.
async fn pipe_handlers(mut rx: mpsc::Receiver<StubNotification>, handlers: Arc<RwLock<Handlers>>) {
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
    type Error = StubError;
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

    async fn push_attachment(&self, _table: &str, attachment: &[u8]) -> Result<i64, Self::Error> {
        let id = self
            .attachments
            .read()
            .unwrap()
            .idx
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        self.attachments
            .write()
            .unwrap()
            .table
            .insert(id, Row::new(id, attachment.to_vec()));

        Ok(id)
    }

    async fn get_attachment(&self, _table: &str, id: i64) -> Result<Vec<u8>, Self::Error> {
        self.fetched_ids.write().unwrap().push(id);
        if self
            .fail_next_get
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            return Err(StubError::Injected);
        }
        if self.corrupt_ids.read().unwrap().contains(&id) {
            return Ok(b"not a valid request".to_vec());
        }
        Ok(self
            .attachments
            .read()
            .unwrap()
            .table
            .get(&id)
            .cloned()
            .map(|v| v.data)
            .unwrap_or_default())
    }

    async fn cleanup_attachments(
        &self,
        _table: &str,
        interval: Duration,
    ) -> Result<(), Self::Error> {
        self.attachments
            .write()
            .unwrap()
            .table
            .retain(|_, v| v.created_at.elapsed() < interval);

        Ok(())
    }

    async fn close(&self) -> Result<(), Self::Error> {
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
