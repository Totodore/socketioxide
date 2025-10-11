use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use engineioxide::handler::EngineIoHandler;
use engineioxide::{DisconnectReason, service::EngineIoService};
use engineioxide::{Socket, Str};
use engineioxide_client::{Client, HttpClient};
use engineioxide_core::{Packet, Sid};
use futures_util::{SinkExt, StreamExt, TryFutureExt};
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
    tx: mpsc::Sender<Event>,
}

impl Handler {
    fn new() -> (Self, mpsc::Receiver<Event>) {
        let (tx, rx) = mpsc::channel(100);
        (Self { tx }, rx)
    }
}

impl EngineIoHandler for Handler {
    type Data = ();
    fn on_connect(self: Arc<Self>, socket: Arc<Socket<Self::Data>>) {
        self.tx.try_send(Event::Connect(socket.id)).unwrap();
    }

    fn on_disconnect(&self, socket: Arc<Socket<Self::Data>>, reason: DisconnectReason) {
        self.tx
            .try_send(Event::Disconnect(socket.id, reason))
            .unwrap();
    }

    fn on_message(self: &Arc<Self>, msg: Str, socket: Arc<Socket<Self::Data>>) {
        self.tx.try_send(Event::Message(socket.id, msg)).unwrap();
    }

    fn on_binary(self: &Arc<Self>, data: Bytes, socket: Arc<Socket<Self::Data>>) {
        self.tx.try_send(Event::Binary(socket.id, data)).unwrap();
    }
}

#[tokio::test]
async fn handshake() {
    tracing_subscriber::fmt::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init()
        .ok();
    let (handler, mut rx) = Handler::new();
    let svc = EngineIoService::new(Arc::new(handler));
    let packet = HttpClient::new(svc).handshake().await.unwrap();
    assert_eq!(rx.recv().await.unwrap(), Event::Connect(packet.sid));
}

#[tokio::test]
async fn connect() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init()
        .ok();
    let (handler, mut rx) = Handler::new();
    let svc = EngineIoService::new(Arc::new(handler));
    let client = Client::connect(svc).await.unwrap();
    assert_eq!(rx.recv().await.unwrap(), Event::Connect(client.sid));
    let (ctx, mut crx) = client.split::<Packet>();

    while let Some(event) = crx.next().await {
        match event {
            Ok(event) => {
                dbg!(event);
            }
            Err(e) => panic!("Error: {e}"),
        }
    }
}

#[tokio::test]
async fn spaaam() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init()
        .ok();
    let (handler, mut rx) = Handler::new();
    let svc = EngineIoService::new(Arc::new(handler));
    let client = Client::connect(svc).await.unwrap();
    assert_eq!(rx.recv().await.unwrap(), Event::Connect(client.sid));
    let (mut ctx, mut crx) = client.split::<Packet>();

    tokio::task::LocalSet::new()
        .run_until(async move {
            tokio::task::spawn_local(async move {
                loop {
                    ctx.send(Packet::Pong).await.unwrap();
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            });

            while let Some(event) = crx.next().await {
                match event {
                    Ok(event) => {
                        dbg!(event);
                    }
                    Err(e) => panic!("Error: {e}"),
                }
            }
        })
        .await;
}
