//! Polling mechanism tests.
//!
//! These tests exercise the [`Client`] read/write halves obtained via
//! [`Client::split`]: packets sent through the sink must reach the server and
//! the echoed packets must be surfaced back through the stream.

use std::sync::Arc;

use bytes::Bytes;
use engineioxide::handler::EngineIoHandler;
use engineioxide::{DisconnectReason, service::EngineIoService};
use engineioxide::{Socket, Str};
use engineioxide_client::{Client, EioEvent};
use engineioxide_core::Sid;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tracing_subscriber::EnvFilter;

mod fixture;

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

/// Packets sent through the write half must reach the server, and the packets
/// the handler echoes back must be surfaced in order through the read half.
#[tokio::test]
async fn round_trip() {
    let (svc, mut rx) = service();
    let client = Client::connect(svc).await.unwrap();
    let sid = client.sid;
    assert_eq!(rx.recv().await.unwrap(), Event::Connect(sid));
    let (mut ctx, mut crx) = client.split::<EioEvent>();

    ctx.send(EioEvent::Message("Hello".into())).await.unwrap();
    ctx.send(EioEvent::Binary(Bytes::from_static(b"Hello")))
        .await
        .unwrap();

    // The server observes both packets.
    assert_eq!(
        rx.recv().await.unwrap(),
        Event::Message(sid, "Hello".into())
    );
    assert_eq!(
        rx.recv().await.unwrap(),
        Event::Binary(sid, Bytes::from_static(b"Hello"))
    );

    // And echoes them back through the read half, in order.
    match crx.next().await {
        Some(Ok(EioEvent::Message(msg))) => assert_eq!(msg, "Hello"),
        other => panic!("expected echoed message, got {other:?}"),
    }
    match crx.next().await {
        Some(Ok(EioEvent::Binary(data))) => assert_eq!(data, Bytes::from_static(b"Hello")),
        other => panic!("expected echoed binary, got {other:?}"),
    }
}

/// Packets sent through the write half must reach the server, and the packets
/// the handler echoes back must be surfaced in order through the read half.
#[tokio::test]
async fn round_trip_ws() {
    let (svc, mut rx) = service();
    let client = fixture::client_ws_connect(svc).await;
    let sid = client.sid;
    assert_eq!(rx.recv().await.unwrap(), Event::Connect(sid));
    let (mut ctx, mut crx) = client.split::<EioEvent>();

    ctx.send(EioEvent::Message("Hello".into())).await.unwrap();
    ctx.send(EioEvent::Binary(Bytes::from_static(b"Hello")))
        .await
        .unwrap();

    // The server observes both packets.
    assert_eq!(
        rx.recv().await.unwrap(),
        Event::Message(sid, "Hello".into())
    );
    assert_eq!(
        rx.recv().await.unwrap(),
        Event::Binary(sid, Bytes::from_static(b"Hello"))
    );

    // And echoes them back through the read half, in order.
    match crx.next().await {
        Some(Ok(EioEvent::Message(msg))) => assert_eq!(msg, "Hello"),
        other => panic!("expected echoed message, got {other:?}"),
    }
    match crx.next().await {
        Some(Ok(EioEvent::Binary(data))) => assert_eq!(data, Bytes::from_static(b"Hello")),
        other => panic!("expected echoed binary, got {other:?}"),
    }
}
