use std::sync::Arc;

use bytes::Bytes;
use engineioxide::handler::EngineIoHandler;
use engineioxide::{DisconnectReason, service::EngineIoService};
use engineioxide::{Socket, Str};
use engineioxide_client::HttpClient;
use engineioxide_core::{Packet, Sid};
use futures_util::TryFutureExt;
use tokio::sync::mpsc;

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
    let (handler, mut rx) = Handler::new();
    let svc = EngineIoService::new(Arc::new(handler));
    let packet = HttpClient::new(svc).handshake().await.unwrap();
    assert_eq!(rx.recv().await.unwrap(), Event::Connect(packet.sid));
}
