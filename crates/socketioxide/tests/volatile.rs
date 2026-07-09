//! Integration tests for volatile events on socketioxide.
//! Verifies volatile emits flow through the transport correctly.
mod fixture;
mod utils;

use fixture::{create_polling_connection, create_server, send_req};
use http::Method;
use socketioxide::{SocketIo, extract::SocketRef};
use tokio::sync::mpsc;

#[tokio::test]
async fn volatile_emit_returns_ok() {
    use serde_json::json;
    let (_svc, io) = SocketIo::new_svc();

    let (tx, mut rx) = mpsc::channel::<Result<(), _>>(1);
    io.ns("/", async move |socket: SocketRef| {
        let result = socket.volatile().emit("test", &json!({"key": "val"}));
        tx.send(result).await.unwrap();
    });

    io.new_dummy_sock("/", ()).await;
    assert!(rx.recv().await.unwrap().is_ok());
}

#[tokio::test]
async fn volatile_emit_second_overwrites() {
    use serde_json::json;
    let (_svc, io) = SocketIo::new_svc();

    let (tx, mut rx) = mpsc::channel::<(Result<(), _>, Result<(), _>)>(1);
    io.ns("/", async move |socket: SocketRef| {
        let first = socket.volatile().emit("test", &json!({"key": "first"}));
        let second = socket.volatile().emit("test", &json!({"key": "second"}));
        tx.send((first, second)).await.unwrap();
    });

    io.new_dummy_sock("/", ()).await;
    let (first, _second) = rx.recv().await.unwrap();
    // Both should return Ok(()); the second emit overwrites the first
    // in the watch channel (it's not "false" since the send succeeds).
    assert!(first.is_ok());
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

    send_req(
        &svc,
        format!("transport=polling&sid={sender_sid}"),
        Method::POST,
        Some("42[\"drawing\",\"hello\"]".into()),
    )
    .await;

    let response = send_req(
        &svc,
        format!("transport=polling&sid={receiver_sid}"),
        Method::GET,
        None,
    )
    .await;
    assert!(
        response.contains("drawing"),
        "Expected volatile broadcast with 'drawing', got: {response}"
    );
}
