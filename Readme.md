# Socketioxide: SocketIO & EngineIO Server in Rust
[![SocketIO CI](https://github.com/Totodore/socketioxide/actions/workflows/socketio-ci.yml/badge.svg)](https://github.com/Totodore/socketioxide/actions/workflows/socketio-ci.yml)
### Socket.IO server implementation as a [tower layer](https://docs.rs/tower/latest/tower/) in Rust
It integrates with any framework based on tower/hyper, such as:
* [axum](https://docs.rs/axum/latest/axum/)
* [warp](https://docs.rs/warp/latest/warp/)
* [hyper](https://docs.rs/hyper/latest/hyper/)

It takes full advantage of the [tower](https://docs.rs/tower/latest/tower/) and [tower-http](https://docs.rs/tower-http/latest/tower_http/) ecosystem of middleware, services, and utilities.


> ⚠️ This crate is under active development and the API is not yet stable.

### Features :
* Namespaces
* Rooms
* Handshake data
* Ack and emit with ack
* Binary
* Polling & Websocket transport
* Extensions on socket to add custom data to sockets

### Planned features :
* Improving the documentation
* Adding more tests & benchmars 
* Other adapter to share state between server instances (like redis adapter), currently only the in memory adapter is implemented
* Better error handling
* State recovery when a socket reconnects

### Examples :
* [Chat app with Axum](./examples/src/chat)
* Echo implementation with Axum :
```rust
use axum::routing::get;
use axum::Server;
use serde::{Serialize, Deserialize};
use socketioxide::{Namespace, SocketIoLayer};
use tracing::info;

#[derive(Debug, Serialize, Deserialize)]
struct MyData {
  pub name: String,
  pub age: u8,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {

    info!("Starting server");

    let ns = Namespace::builder()
        .add("/", |socket| async move {
            info!("Socket connected on / namespace with id: {}", socket.sid);

            // Add a callback triggered when the socket receive an 'abc' event
            // The json data will be deserialized to MyData
            socket.on("abc", |socket, data: MyData, bin, _| async move {
                info!("Received abc event: {:?} {:?}", data, bin);
                socket.bin(bin).emit("abc", data).ok();
            });

            // Add a callback triggered when the socket receive an 'acb' event
            // Ackknowledge the message with the ack callback
            socket.on("acb", |_, data: Value, bin, ack| async move {
                info!("Received acb event: {:?} {:?}", data, bin);
                ack.bin(bin).send(data).ok();
            });
        })
        .add("/custom", |socket| async move {
            info!("Socket connected on /custom namespace with id: {}", socket.sid);
        })
        .build();

    let app = axum::Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .layer(SocketIoLayer::new(ns));

    Server::bind(&"0.0.0.0:3000".parse().unwrap())
        .serve(app.into_make_service())
        .await?;

    Ok(())
}
```