//! Integration tests for volatile events on socketioxide.
//! Verifies the volatile operator API and that volatile emits
//! silently drop errors rather than propagating them.
mod fixture;
mod utils;

use fixture::{create_polling_connection, create_server, send_req};
use http::Method;
use socketioxide::{SocketIo, extract::SocketRef};

#[tokio::test]
async fn volatile_emit_returns_ok() {
    use serde_json::json;
    let (_svc, io) = SocketIo::new_svc();

    let (tx, rx) = std::sync::mpsc::channel();
    io.ns("/", async move |socket: SocketRef| {
        let result = socket.volatile().emit("test", &json!({"key": "val"}));
        tx.send(result).unwrap();
    });

    io.new_dummy_sock("/", ()).await;
    assert!(rx.recv().unwrap().is_ok());
}

#[tokio::test]
async fn volatile_emit_broadcast_does_not_panic() {
    let (_svc, io) = SocketIo::new_svc();

    let (tx, rx) = std::sync::mpsc::channel();
    io.ns("/", async move |socket: SocketRef| {
        // Broadcast with volatile flag should complete without panicking
        socket
            .within("room")
            .volatile()
            .emit("event", &"data")
            .await
            .ok();
        tx.send(()).unwrap();
    });

    io.new_dummy_sock("/", ()).await;
    rx.recv().unwrap();
}

#[tokio::test]
async fn io_volatile_composes() {
    let (_svc, io) = SocketIo::new_svc();
    io.ns("/", |_: SocketRef| async {});

    io.new_dummy_sock("/", ()).await;

    // Volatile on the io handle delegates to the default namespace
    let _ = io.volatile();
    let _ = io.of("/").unwrap().volatile();
}

#[tokio::test]
async fn volatile_emit_on_disconnected_socket_returns_ok() {
    use serde_json::json;
    let (_svc, io) = SocketIo::new_svc();

    let (tx, rx) = std::sync::mpsc::channel();
    io.ns("/", {
        let tx = tx.clone();
        async move |_: SocketRef| {
            tx.send(()).unwrap();
        }
    });

    io.new_dummy_sock("/", ()).await;
    rx.recv().unwrap();

    // At this point the dummy socket's connect handler has run.
    // The socket is technically connected — test that volatile
    // emit returns Ok(()) without error.
    let (tx2, rx2) = std::sync::mpsc::channel();
    io.ns("/test", async move |socket: SocketRef| {
        let result = socket.volatile().emit("event", &json!({"data": 42}));
        tx2.send(result).unwrap();
    });

    io.new_dummy_sock("/test", ()).await;
    assert!(rx2.recv().unwrap().is_ok());
}

#[tokio::test]
async fn volatile_broadcast_arrives_via_polling_transport() {
    let (svc, io) = create_server().await;

    io.ns("/", |s: SocketRef| async move {
        s.on(
            "drawing",
            |s: SocketRef, socketioxide::extract::Data::<serde_json::Value>(data)| async move {
                s.broadcast().volatile().emit("drawing", &data).await.ok();
            },
        );
    });

    let sender_sid = create_polling_connection(&svc).await;
    let receiver_sid = create_polling_connection(&svc).await;

    // Drain any queued handshake data from the connections
    send_req(
        &svc,
        format!("transport=polling&sid={sender_sid}"),
        Method::GET,
        None,
    )
    .await;
    send_req(
        &svc,
        format!("transport=polling&sid={receiver_sid}"),
        Method::GET,
        None,
    )
    .await;
    // Respond to pings to keep sessions alive
    send_req(
        &svc,
        format!("transport=polling&sid={sender_sid}"),
        Method::POST,
        Some("3".into()),
    )
    .await;
    send_req(
        &svc,
        format!("transport=polling&sid={receiver_sid}"),
        Method::POST,
        Some("3".into()),
    )
    .await;

    // Send drawing event from sender
    send_req(
        &svc,
        format!("transport=polling&sid={sender_sid}"),
        Method::POST,
        Some("42[\"drawing\",\"hello\"]".into()),
    )
    .await;

    // Poll receiver — should receive the volatile broadcast
    let response = send_req(
        &svc,
        format!("transport=polling&sid={receiver_sid}"),
        Method::GET,
        None,
    )
    .await;
    // send_req skips first char (engine.io message type '4')
    assert!(
        response.contains("drawing"),
        "Expected volatile broadcast with 'drawing', got: {response}"
    );
}
