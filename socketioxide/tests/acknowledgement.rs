mod fixture;
use fixture::{create_server, create_ws_connection};
use socketioxide::extract::SocketRef;
use tokio::sync::mpsc;

#[tokio::test]
pub async fn data_extractor() {
    let io = create_server(2001).await;
    let (tx, mut rx) = mpsc::channel::<i32>(4);

    io.ns("/", move |socket: SocketRef| async move {
        println!("Socket connected on / namespace with id: {}", socket.id);
        let ack = socket
            .emit_with_ack::<String>("test", "test")
            .await
            .unwrap();
    });

    let mut stream = create_ws_connection(2001).await;
    stream
        .send(Message::Text("42[\"test\", 1]".to_string()))
        .await
        .unwrap();
    stream
        .send(Message::Text("42[\"async_test\", 2]".to_string()))
        .await
        .unwrap();
    stream
        .send(Message::Text("42[\"ko_test\", 2]".to_string()))
        .await
        .unwrap();
    assert_eq!(rx.recv().await.unwrap(), 1);
    assert_eq!(rx.recv().await.unwrap(), 2);
    stream.close(None).await.unwrap();
}
