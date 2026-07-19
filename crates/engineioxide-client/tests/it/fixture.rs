#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use engineioxide::config::EngineIoConfig;
use engineioxide::handler::EngineIoHandler;
use engineioxide::service::EngineIoService;
use engineioxide::{DisconnectReason, Socket};
use engineioxide_client::flavors::testing::TestingFlavor;
use engineioxide_core::{Sid, Str};
use tokio::sync::mpsc;
use tracing_subscriber::EnvFilter;

/// Default deadline for every await in the tests: an enforcement test must
/// fail fast instead of hanging the whole suite.
pub const DEADLINE: Duration = Duration::from_secs(5);

/// Await `fut` with the global [`DEADLINE`], panicking with `what` on expiry.
pub async fn within<F: Future>(what: &str, fut: F) -> F::Output {
    match tokio::time::timeout(DEADLINE, fut).await {
        Ok(v) => v,
        Err(_) => panic!("timed out after {DEADLINE:?}: {what}"),
    }
}

/// Handle over the sockets currently connected to the test server, letting
/// tests drive server-side actions (e.g. closing a session).
pub type SocketRegistry = Arc<Mutex<HashMap<Sid, Arc<Socket<()>>>>>;

#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    Connect(Sid),
    Disconnect(Sid, DisconnectReason),
    Message(Sid, Str),
    Binary(Sid, Bytes),
}

#[derive(Debug)]
pub struct EchoHandler {
    tx: mpsc::UnboundedSender<Event>,
    sockets: SocketRegistry,
}

impl EchoHandler {
    fn new() -> (Self, mpsc::UnboundedReceiver<Event>, SocketRegistry) {
        let (tx, rx) = mpsc::unbounded_channel();
        let sockets = SocketRegistry::default();
        (
            Self {
                tx,
                sockets: sockets.clone(),
            },
            rx,
            sockets,
        )
    }
}

fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init()
        .ok();
}

pub fn service_with_config(
    config: EngineIoConfig,
) -> (
    TestingFlavor<EngineIoService<EchoHandler>>,
    mpsc::UnboundedReceiver<Event>,
) {
    let (svc, rx, _) = service_with_registry(config);
    (svc, rx)
}

/// Same as [`service_with_config`] but also returns the [`SocketRegistry`]
/// so tests can act on server-side sockets (e.g. close them).
pub fn service_with_registry(
    config: EngineIoConfig,
) -> (
    TestingFlavor<EngineIoService<EchoHandler>>,
    mpsc::UnboundedReceiver<Event>,
    SocketRegistry,
) {
    init_tracing();
    let (handler, rx, sockets) = EchoHandler::new();
    let svc = EngineIoService::with_config(Arc::new(handler), config);
    (svc.into(), rx, sockets)
}

pub fn service() -> (
    TestingFlavor<EngineIoService<EchoHandler>>,
    mpsc::UnboundedReceiver<Event>,
) {
    service_with_config(EngineIoConfig::default())
}

impl EngineIoHandler for EchoHandler {
    type Data = ();
    fn on_connect(self: Arc<Self>, socket: Arc<Socket<Self::Data>>) {
        self.sockets
            .lock()
            .unwrap()
            .insert(socket.id, socket.clone());
        self.tx.send(Event::Connect(socket.id)).unwrap();
    }

    fn on_disconnect(&self, socket: Arc<Socket<Self::Data>>, reason: DisconnectReason) {
        self.tx.send(Event::Disconnect(socket.id, reason)).unwrap();
    }

    fn on_message(self: &Arc<Self>, msg: Str, socket: Arc<Socket<Self::Data>>) {
        self.tx
            .send(Event::Message(socket.id, msg.clone()))
            .unwrap();
        socket.emit(msg).unwrap();
    }

    fn on_binary(self: &Arc<Self>, data: Bytes, socket: Arc<Socket<Self::Data>>) {
        self.tx
            .send(Event::Binary(socket.id, data.clone()))
            .unwrap();
        socket.emit_binary(data).unwrap();
    }
}
