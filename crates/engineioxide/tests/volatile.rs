use std::sync::Arc;

use bytes::Bytes;
use engineioxide::{
    Str,
    handler::EngineIoHandler,
    socket::{DisconnectReason, Socket},
};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

mod fixture;
use fixture::{create_polling_connection, create_server, create_ws_connection, send_req};

#[derive(Debug, Clone)]
struct VolatileHandler {
    socket_tx: mpsc::Sender<Arc<Socket<()>>>,
}
impl EngineIoHandler for VolatileHandler {
    type Data = ();
    fn on_connect(self: Arc<Self>, socket: Arc<Socket<()>>) {
        self.socket_tx.try_send(socket).ok();
    }
    fn on_disconnect(&self, _socket: Arc<Socket<()>>, _reason: DisconnectReason) {}
    fn on_message(self: &Arc<Self>, msg: Str, socket: Arc<Socket<()>>) {
        if msg == "trigger_volatile" {
            socket.emit_volatile("volatile_response");
        } else if msg == "echo" {
            socket.emit(msg).ok();
        }
    }
    fn on_binary(self: &Arc<Self>, data: Bytes, socket: Arc<Socket<()>>) {
        socket.emit_binary(data).ok();
    }
}

#[tokio::test]
async fn volatile_message_arrives_via_polling() {
    let (socket_tx, _socket_rx) = mpsc::channel(10);
    let mut svc = create_server(VolatileHandler { socket_tx }).await;
    let sid = create_polling_connection(&mut svc).await;

    send_req(
        &mut svc,
        format!("transport=polling&sid={sid}"),
        http::Method::POST,
        Some("4trigger_volatile".into()),
    )
    .await;

    let response = send_req(
        &mut svc,
        format!("transport=polling&sid={sid}"),
        http::Method::GET,
        None,
    )
    .await;
    // send_req skips the first character (packet type '4')
    assert_eq!(response, "volatile_response");
}

#[tokio::test]
async fn mixed_volatile_and_normal_via_polling() {
    let (socket_tx, mut socket_rx) = mpsc::channel(10);
    let mut svc = create_server(VolatileHandler { socket_tx }).await;
    let sid = create_polling_connection(&mut svc).await;

    let socket = socket_rx
        .recv()
        .await
        .expect("socket not received from on_connect");

    // Send a normal message AND a volatile
    socket.emit("normal_msg").ok();
    assert!(socket.emit_volatile("volatile_msg"));

    let response = send_req(
        &mut svc,
        format!("transport=polling&sid={sid}"),
        http::Method::GET,
        None,
    )
    .await;
    // Volatile should have priority and appear first, before normal.
    // send_req skips only the first character (the leading '4' of the volatile).
    assert_eq!(response, "volatile_msg\x1e4normal_msg");
}

#[tokio::test]
async fn volatile_overwrite_only_latest_survives() {
    let (socket_tx, mut socket_rx) = mpsc::channel(10);
    let mut svc = create_server(VolatileHandler { socket_tx }).await;
    let sid = create_polling_connection(&mut svc).await;

    let socket = socket_rx
        .recv()
        .await
        .expect("socket not received from on_connect");

    assert!(socket.emit_volatile("dropped"));
    assert!(socket.emit_volatile("kept"));
    assert!(socket.emit("normal").is_ok());

    let response = send_req(
        &mut svc,
        format!("transport=polling&sid={sid}"),
        http::Method::GET,
        None,
    )
    .await;
    // After send_req's skip(1): "kept" then \x1e separator then "4normal"
    assert_eq!(response, "kept\x1e4normal");
}

#[tokio::test]
async fn volatile_only_arrives_via_polling() {
    let (socket_tx, mut socket_rx) = mpsc::channel(10);
    let mut svc = create_server(VolatileHandler { socket_tx }).await;
    let sid = create_polling_connection(&mut svc).await;

    let socket = socket_rx
        .recv()
        .await
        .expect("socket not received from on_connect");

    assert!(socket.emit_volatile("volatile_only"));

    let response = send_req(
        &mut svc,
        format!("transport=polling&sid={sid}"),
        http::Method::GET,
        None,
    )
    .await;
    assert_eq!(response, "volatile_only");
}

#[tokio::test]
async fn volatile_only_arrives_via_ws() {
    use std::time::Duration;

    use futures_util::StreamExt;

    let (socket_tx, mut socket_rx) = mpsc::channel(10);
    let mut svc = create_server(VolatileHandler { socket_tx }).await;
    let mut stream = create_ws_connection(&mut svc).await;

    let socket = socket_rx
        .recv()
        .await
        .expect("socket not received from on_connect");

    let _open_packet = stream.next().await.unwrap().unwrap();

    let ping = stream.next().await.unwrap().unwrap();
    assert_eq!(ping, Message::Text("2".into()));

    assert!(socket.emit_volatile("volatile_only"));

    let volatile = tokio::time::timeout(Duration::from_millis(100), stream.next()).await;

    let msg = volatile
        .expect("timeout: volatile-only packet was never flushed over websocket")
        .unwrap()
        .unwrap();
    assert_eq!(msg, Message::Text("4volatile_only".into()));
}
