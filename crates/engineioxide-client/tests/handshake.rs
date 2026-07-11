use std::sync::Arc;

use bytes::Bytes;
use engineioxide::handler::EngineIoHandler;
use engineioxide::{DisconnectReason, service::EngineIoService};
use engineioxide::{Socket, Str};
use engineioxide_client::{Client, HttpClient};
use engineioxide_core::{Packet, Sid};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tracing_subscriber::EnvFilter;

#[derive(Debug, PartialEq, Eq)]
enum Event {
    Connect(Sid),
    Disconnect(Sid, DisconnectReason),
    Message(Sid, Str),
    Binary(Sid, Bytes),
}

#[derive(Debug)]
struct Handler {
    tx: mpsc::UnboundedSender<Event>,
}

impl Handler {
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

/// Build an [`EngineIoService`] with a short ping interval/timeout so the
/// heartbeat fires quickly during tests.
fn service() -> (EngineIoService<Handler>, mpsc::UnboundedReceiver<Event>) {
    init_tracing();
    let (handler, rx) = Handler::new();
    let svc = EngineIoService::new(Arc::new(handler));
    (svc, rx)
}

impl EngineIoHandler for Handler {
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

#[tokio::test]
async fn handshake() {
    let (svc, mut rx) = service();
    let (_, open) = HttpClient::connect(svc).await.unwrap();
    assert_eq!(rx.recv().await.unwrap(), Event::Connect(open.sid));
}

#[tokio::test]
async fn connect() {
    let (svc, mut rx) = service();
    let client = Client::connect(svc).await.unwrap();
    assert_eq!(rx.recv().await.unwrap(), Event::Connect(client.sid));
    let (mut ctx, mut crx) = client.split::<Packet>();

    ctx.send(Packet::Message("Hello".into())).await.unwrap();
    ctx.send(Packet::Binary(Bytes::from("Hello".to_string())))
        .await
        .unwrap();

    while let Some(event) = crx.next().await {
        match event {
            Ok(event) => {
                ctx.send(dbg!(event)).await.unwrap();
            }
            Err(e) => panic!("Error: {e}"),
        }
    }
}
