# Socket.io & Engine.io server Rust
[![SocketIO CI](https://github.com/Totodore/socketioxide/actions/workflows/socketio-ci.yml/badge.svg)](https://github.com/Totodore/socketioxide/actions/workflows/socketio-ci.yml)
[![EngineIO CI](https://github.com/Totodore/socketioxide/actions/workflows/engineio-ci.yml/badge.svg)](https://github.com/Totodore/socketioxide/actions/workflows/engineio-ci.yml)

### Socket.io server as a [tower layer](https://docs.rs/tower/latest/tower/) in Rust
It integrates with any framework based on tower/hyper, such as:
* [axum](https://docs.rs/axum/latest/axum/): [echo implementation](./examples/src/socketio-echo/axum_echo.rs)
* [warp](https://docs.rs/warp/latest/warp/): [echo implementation](./examples/src/socketio-echo/warp_echo.rs)
* [hyper](https://docs.rs/hyper/latest/hyper/): [echo implementation](./examples/src/socketio-echo/hyper_echo.rs)

It takes full advantage of the [tower](https://docs.rs/tower/latest/tower/) and [tower-http](https://docs.rs/tower-http/latest/tower_http/) ecosystem of middleware, services, and utilities.


> ⚠️ This crate is under active development and the API is not yet stable.

### Features :
* Namespaces
* Rooms
* Handshake data
* Ack and emit with ack
* Binary packets
* Polling & Websocket transport
* Extensions to add custom data to sockets
* Memory efficient payload parsing with streams

### Planned features :
* Other adapter to share state between server instances (like redis adapter), currently only the in memory adapter is implemented
* State recovery when a socket reconnects
* SocketIo v3 support (currently only v4 is supported)

### Examples :
* [Chat app with Axum](./examples/src/chat)
* Echo implementation with Axum :
```rust
use axum::routing::get;
use axum::Server;
use serde::{Serialize, Deserialize};
use socketioxide::{Namespace, SocketIoLayer};
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize)]
struct MyData {
  pub name: String,
  pub age: u8,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {

    println!("Starting server");

    let ns = Namespace::builder()
        .add("/", |socket| async move {
            println!("Socket connected on / namespace with id: {}", socket.sid);

            // Add a callback triggered when the socket receives an 'abc' event
            // The json data will be deserialized to MyData
            socket.on("abc", |socket, data: MyData, bin, _| async move {
                println!("Received abc event: {:?} {:?}", data, bin);
                socket.bin(bin).emit("abc", data).ok();
            });

            // Add a callback triggered when the socket receives an 'acb' event
            // Ackknowledge the message with the ack callback
            socket.on("acb", |_, data: Value, bin, ack| async move {
                println!("Received acb event: {:?} {:?}", data, bin);
                ack.bin(bin).send(data).ok();
            });
            // Add a callback triggered when the socket disconnects
            // The reason of the disconnection will be passed to the callback
            socket.on_disconnect(|socket, reason| async move {
                println!("Socket.IO disconnected: {} {}", socket.sid, reason);
            });
        })
        .add("/custom", |socket| async move {
            println!("Socket connected on /custom namespace with id: {}", socket.sid);
        })
        .build();

    let app = axum::Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .layer(SocketIoLayer::new(ns));

    Server::bind(&"127.0.0.1:3000".parse().unwrap())
        .serve(app.into_make_service())
        .await?;

    Ok(())
}
```
