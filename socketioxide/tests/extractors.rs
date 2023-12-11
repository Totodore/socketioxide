use futures::SinkExt;
use socketioxide::extract::{Data, SocketRef, State};
use tokio::sync::mpsc;

mod fixture;
use fixture::{create_server, create_server_with_state, create_ws_connection};
use tokio_tungstenite::tungstenite::Message;

#[tokio::test]
pub async fn state_extractor() {
    let state = 1112i32;
    let io = create_server_with_state(2000, state);
    let (tx, mut rx) = mpsc::channel::<i32>(4);
    io.ns("/", move |socket: SocketRef, state: State<i32>| {
        println!("Socket connected on / namespace with id: {}", socket.id);
        tx.try_send(*state).unwrap();

        let tx1 = tx.clone();
        let tx2 = tx.clone();
        let tx3 = tx.clone();
        socket.on("test", move |State(state): State<i32>| {
            println!("test event received");
            tx.try_send(*state).unwrap();
        });
        socket.on("async_test", move |State(state): State<i32>| async move {
            println!("async_test event received");
            tx2.try_send(*state).unwrap();
        });
        // This handler should not be called
        socket.on("ko_test", move |State(_): State<String>| {
            println!("ko_test event received");
            tx3.try_send(1213231).unwrap();
        });
        socket.on_disconnect(move |State(state): State<i32>| {
            println!("close event received");
            tx1.try_send(*state).unwrap();
        });
    });

    let mut stream = create_ws_connection(2000).await;
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
    assert_eq!(rx.recv().await.unwrap(), state);
    assert_eq!(rx.recv().await.unwrap(), state);
    assert_eq!(rx.recv().await.unwrap(), state);
    stream.close(None).await.unwrap();
    assert_eq!(rx.recv().await.unwrap(), state);
}

#[tokio::test]
pub async fn data_extractor() {
    let io = create_server(2001);
    let (tx, mut rx) = mpsc::channel::<i32>(4);
    io.ns("/", move |socket: SocketRef| {
        println!("Socket connected on / namespace with id: {}", socket.id);
        let tx1 = tx.clone();
        let tx2 = tx.clone();
        socket.on("test", move |Data(data): Data<i32>| {
            println!("test event received");
            tx1.try_send(data).unwrap();
        });
        socket.on("async_test", move |Data(data): Data<i32>| async move {
            println!("async_test event received");
            tx2.try_send(data).unwrap();
        });
        // This handler should not be called
        socket.on("ko_test", move |Data(_): Data<String>| {
            println!("ko_test event received");
            tx.try_send(1213231).unwrap();
        });
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
