mod fixture;
mod utils;

use fixture::{create_server, create_ws_connection};
use futures::{FutureExt, SinkExt};
use socketioxide::extract::SocketRef;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use crate::fixture::{socketio_client, socketio_client_with_handler};

#[tokio::test]
pub async fn ack_emit_single_with_ack() {
    const PORT: u16 = 2100;
    let io = create_server(PORT).await;
    let (tx, mut rx) = mpsc::channel::<String>(4);

    io.ns("/", move |socket: SocketRef| async move {
        let res = assert_ok!(socket.emit_with_ack::<String>("test", "test"));
        let ack = assert_ok!(res.await);
        tx.try_send(ack.data).unwrap();
    });

    let handler = |_, _| Box::pin(async move {});
    let client = socketio_client_with_handler(PORT, "test", handler, ()).await;
    let client = assert_ok!(client);

    assert_eq!(rx.recv().await.unwrap(), "test");
    assert_eq!(rx.recv().await.unwrap(), "test");
    assert_ok!(client.disconnect().await);
}
