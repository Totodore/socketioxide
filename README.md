<img src="https://socket.io/images/logo-dark.svg" align="right" width=150 />

# [`Socketioxide`](https://github.com/totodore/socketioxide) 🚀🦀

A [***`socket.io`***](https://socket.io) server implementation in Rust that integrates with the [***`Tower`***](https://tokio.rs/#tk-lib-tower) ecosystem and the [***`Tokio stack`***](https://tokio.rs). Integrates with any server framework based on tower like [***`Axum`***](https://docs.rs/axum/latest/axum/), [***`Warp`***](https://docs.rs/warp/latest/warp/) or [***`Hyper`***](https://docs.rs/hyper/latest/hyper/). Add any other tower based middleware on top of socketioxide such as CORS, SSL, compression, etc with [***`tower-http`***](https://docs.rs/tower-http/latest/tower_http/).

> ⚠️ This crate is under active development and the API is not yet stable.


[![Crates.io](https://img.shields.io/crates/v/socketioxide.svg)](https://crates.io/crates/socketioxide)
[![Documentation](https://docs.rs/socketioxide/badge.svg)](https://docs.rs/socketioxide)
[![SocketIO CI](https://github.com/Totodore/socketioxide/actions/workflows/socketio-ci.yml/badge.svg)](https://github.com/Totodore/socketioxide/actions/workflows/socketio-ci.yml)
[![EngineIO CI](https://github.com/Totodore/socketioxide/actions/workflows/engineio-ci.yml/badge.svg)](https://github.com/Totodore/socketioxide/actions/workflows/engineio-ci.yml)


## Features :
* Integrates with :
  * [Axum](https://docs.rs/axum/latest/axum/): [echo example](./examples/src/socketio-echo/axum_echo.rs)
  * [Warp](https://docs.rs/warp/latest/warp/): [echo example](./examples/src/socketio-echo/warp_echo.rs)
  * [Hyper](https://docs.rs/hyper/latest/hyper/): [echo example](./examples/src/socketio-echo/hyper_echo.rs)
* Out of the box support for any other middleware based on tower :
  * [CORS](https://docs.rs/tower-http/latest/tower_http/struct.CorsLayer.html)
  * [SSL](https://docs.rs/tower-http/latest/tower_http/struct.SslLayer.html)
  * [Compression](https://docs.rs/tower-http/latest/tower_http/struct.CompressionLayer.html)
  * [Timeout](https://docs.rs/tower-http/latest/tower_http/struct.TimeoutLayer.html)
* Namespaces
* Rooms
* Ack and emit with ack
* Binary packets
* Polling & Websocket transports
* Extensions to add custom data to sockets
* Memory efficient http payload parsing with streams


## Planned features :
* Other adapter to share state between server instances (like redis adapter), currently only the in memory adapter is implemented
* Better API ergonomics
* State recovery when a socket reconnects
* SocketIo v3 support (currently only v4 is supported)

## Examples :
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
