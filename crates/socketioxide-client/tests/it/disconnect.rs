use socketioxide::{SocketIo, extract::SocketRef, socket::DisconnectReason};
use socketioxide_client::IoManager;
use tokio::sync::mpsc;

#[tokio::test]
async fn client_ns_disconnect() {
    let (svc, server) = SocketIo::new_svc();
    let (tx, mut rx) = mpsc::channel(1);
    server.ns("/", async move |s: SocketRef| {
        s.on_disconnect({
            let tx = tx.clone();
            async move |reason: DisconnectReason| tx.try_send(reason).unwrap()
        });
    });

    let manager = IoManager::new();
    let socket = manager.connect(svc).await;

    socket.disconnect().await.unwrap();

    assert_eq!(rx.recv().await, Some(DisconnectReason::ClientNSDisconnect));
}
