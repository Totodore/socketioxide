//! Tests for extractors
use std::time::Duration;

use serde_json::json;
use socketioxide::extract::{Data, SocketRef, State, TryData};
use tokio::sync::mpsc;

use fixture::{create_server, create_server_with_state};

use crate::fixture::socketio_client;

mod fixture;
mod utils;

#[tokio::test]
pub async fn state_extractor() {
    const PORT: u16 = 2000;
    const TIMEOUT: Duration = Duration::from_millis(200);
    let state = 1112i32;
    let io = create_server_with_state(PORT, state).await;
    let (tx, mut rx) = mpsc::channel::<i32>(4);
    io.ns("/", move |socket: SocketRef, state: State<i32>| {
        assert_ok!(tx.try_send(*state));
        socket.on("test", move |State(state): State<i32>| {
            assert_ok!(tx.try_send(*state))
        });
    });
    let client = assert_ok!(socketio_client(PORT, ()).await);
    assert_eq!(
        tokio::time::timeout(TIMEOUT, rx.recv())
            .await
            .unwrap()
            .unwrap(),
        state
    );

    assert_ok!(client.emit("test", json!("foo")).await);
    assert_eq!(
        tokio::time::timeout(TIMEOUT, rx.recv())
            .await
            .unwrap()
            .unwrap(),
        state
    );

    assert_ok!(client.disconnect().await);
}

#[tokio::test]
pub async fn data_extractor() {
    const PORT: u16 = 2001;
    let io = create_server(PORT).await;
    let (tx, mut rx) = mpsc::channel::<String>(4);
    let tx1 = tx.clone();
    io.ns("/", move |socket: SocketRef, Data(data): Data<String>| {
        assert_ok!(tx.try_send(data));
        socket.on("test", move |Data(data): Data<String>| {
            assert_ok!(tx.try_send(data));
        });
    });

    assert_ok!(socketio_client(PORT, ()).await);
    assert_ok!(socketio_client(PORT, 1321).await);

    // Capacity should be the same as the handler should not be called
    assert_eq!(tx1.capacity(), 4);

    let client = assert_ok!(socketio_client(PORT, "foo").await);
    assert_eq!(rx.recv().await.unwrap(), "foo");

    assert_ok!(client.emit("test", json!("oof")).await);
    assert_eq!(rx.recv().await.unwrap(), "oof");

    assert_ok!(client.emit("test", json!({ "test": 132 })).await);
    // Capacity should be the same as the handler should not be called
    assert_eq!(tx1.capacity(), 4);

    assert_ok!(client.disconnect().await);
}

#[tokio::test]
pub async fn try_data_extractor() {
    const PORT: u16 = 2002;
    let io = create_server(PORT).await;
    let (tx, mut rx) = mpsc::channel::<Result<String, serde_json::Error>>(4);
    io.ns("/", move |s: SocketRef, TryData(data): TryData<String>| {
        assert_ok!(tx.try_send(data));
        s.on("test", move |TryData(data): TryData<String>| {
            assert_ok!(tx.try_send(data));
        });
    });

    // Non deserializable data
    assert_ok!(socketio_client(PORT, ()).await);
    assert_err!(rx.recv().await.unwrap());

    // Non deserializable data
    assert_ok!(socketio_client(PORT, 1321).await);
    assert_err!(rx.recv().await.unwrap());

    let client = assert_ok!(socketio_client(PORT, "foo").await);
    let res = assert_ok!(rx.recv().await.unwrap());
    assert_eq!(res, "foo");

    assert_ok!(client.emit("test", json!("oof")).await);
    let res = assert_ok!(rx.recv().await.unwrap());
    assert_eq!(res, "oof");

    // Non deserializable data
    assert_ok!(client.emit("test", json!({ "test": 132 })).await);
    assert_err!(rx.recv().await.unwrap());

    assert_ok!(client.disconnect().await);
}
