use std::sync::Arc;

use bytes::Bytes;
use engineioxide::config::EngineIoConfig;
use engineioxide::handler::EngineIoHandler;
use engineioxide::service::EngineIoService;
use engineioxide::{DisconnectReason, Socket};
use engineioxide_client::flavors::testing::TestingFlavor;
use engineioxide_core::{Sid, Str};
use tokio::sync::mpsc;
use tracing_subscriber::EnvFilter;

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
}

impl EchoHandler {
    fn new() -> (Self, mpsc::UnboundedReceiver<Event>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Self { tx }, rx)
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
) -> (TestingFlavor<EchoHandler>, mpsc::UnboundedReceiver<Event>) {
    init_tracing();
    let (handler, rx) = EchoHandler::new();
    let svc = EngineIoService::with_config(Arc::new(handler), config);
    (svc.into(), rx)
}

#[allow(unused)]
pub fn service() -> (TestingFlavor<EchoHandler>, mpsc::UnboundedReceiver<Event>) {
    service_with_config(EngineIoConfig::default())
}

impl EngineIoHandler for EchoHandler {
    type Data = ();
    fn on_connect(self: Arc<Self>, socket: Arc<Socket<Self::Data>>) {
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
