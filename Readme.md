# Socketioxide: SocketIO & EngineIO Server in Rust
[![SocketIO CI](https://github.com/Totodore/socketioxide/actions/workflows/socketio-ci.yml/badge.svg)](https://github.com/Totodore/socketioxide/actions/workflows/socketio-ci.yml)

### Rust implementation for a socket.io server, it is based on :
* hyper
* tokio
* tokio-tungstenite
* tower

> ⚠️ This crate is under active development and the API is not yet stable.
### Socket.IO is a tower Service, therefore it itegrates really well with frameworks based on tower like axum and warp.
### Features :
* Namespaces
* Rooms
* Handshake data
* Ack and emit with ack
* Binary

### Planned features :
* Improving the documentation
* Improving tests
* Other adapter to share state between server instances (like redis adapter), currently only the in memory adapter is implemented
* Better error handling
* Adding handshake options
* Socket extensions to share state between sockets
* State recovery when a socket reconnects


### Socket.IO example echo implementation with Axum :
```rust
use axum::routing::get;
use axum::Server;
use serde_json::Value;
use socketioxide::{Namespace, SocketIoLayer};
use tracing::info;
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::builder().finish();
    tracing::subscriber::set_global_default(subscriber)?;

    info!("Starting server");

    let ns = Namespace::builder()
        .add("/", |socket| async move {
            info!("Socket.IO connected: {:?} {:?}", socket.ns(), socket.sid);
            let data: Value = socket.handshake.data().unwrap();
            socket.emit("auth", data).ok();

            socket.on("message", |socket, data: Value, bin, _| async move {
                info!("Received event: {:?} {:?}", data, bin);
                socket.bin(bin).emit("message-back", data).ok();
            });

            socket.on("message-with-ack", |_, data: Value, bin, ack| async move {
                info!("Received event: {:?} {:?}", data, bin);
                ack.bin(bin).send(data).ok();
            });
        })
        .add("/custom", |socket| async move {
            info!("Socket.IO connected on: {:?} {:?}", socket.ns(), socket.sid);
            let data: Value = socket.handshake.data().unwrap();
            socket.emit("auth", data).ok();
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

### Engine.IO example echo implementation with Axum :
```rust
use std::time::Duration;

use axum::routing::get;
use axum::Server;
use engineioxide::{
    errors::Error,
    layer::{EngineIoConfig, EngineIoHandler, EngineIoLayer},
    socket::Socket,
};
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

#[derive(Clone)]
struct MyHandler;

#[engineioxide::async_trait]
impl EngineIoHandler for MyHandler {
    fn on_connect(&self, socket: &Socket<Self>) {
        println!("socket connect {}", socket.sid);
    }
    fn on_disconnect(&self, socket: &Socket<Self>) {
        println!("socket disconnect {}", socket.sid);
    }

    async fn on_message(&self, msg: String, socket: &Socket<Self>) -> Result<(), Error> {
        println!("Ping pong message {:?}", msg);
        socket.emit(msg).await
    }

    async fn on_binary(&self, data: Vec<u8>, socket: &Socket<Self>) -> Result<(), Error> {
        println!("Ping pong binary message {:?}", data);
        socket.emit_binary(data).await
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::builder()
        .with_line_number(true)
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    let config = EngineIoConfig::builder()
        .ping_interval(Duration::from_millis(300))
        .ping_timeout(Duration::from_millis(200))
        .max_payload(1e6 as u64)
        .build();
    info!("Starting server");
    let app = axum::Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .layer(EngineIoLayer::from_config(MyHandler, config));

    Server::bind(&"0.0.0.0:3000".parse().unwrap())
        .serve(app.into_make_service())
        .await?;

    Ok(())
}
```
