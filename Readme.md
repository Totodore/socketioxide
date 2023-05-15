# SocketIO & EngineIO Server in Rust
[![SocketIO CI](https://github.com/Totodore/rust-socketio-server/actions/workflows/socketio-ci.yml/badge.svg)](https://github.com/Totodore/rust-socketio-server/actions/workflows/socketio-ci.yml)

### Rust implementation for a socket.io server, it is based on :
* hyper
* tokio
* tokio-tungstenite
* tower

> ⚠️ This crate is under active development and the API is not yet stable.

### Socket.IO example echo implementation with Axum :
```rust
use std::time::Duration;

use axum::routing::get;
use axum::Server;
use serde_json::Value;
use socketio_server::{config::SocketIoConfig, layer::SocketIoLayer, ns::Namespace, socket::Ack};
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::builder()
        .with_line_number(true)
        .with_max_level(Level::DEBUG)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    let config = SocketIoConfig::builder()
        .ping_interval(Duration::from_millis(300))
        .ping_timeout(Duration::from_millis(200))
        .max_payload(1e6 as u64)
        .build();
    info!("Starting server");

    let ns = Namespace::builder()
        .add("/", |socket| async move {
            info!("Socket.IO connected: {:?} {:?}", socket.ns, socket.sid);

            socket.emit("auth", socket.handshake.auth.clone()).ok();

            socket.on("message", |socket, data: Value, bin| async move {
                if let Some(bin) = bin {
                    info!("Received event binary: {:?} {:?}", data, bin);
                    socket.emit_bin("message-back", data, bin).ok();
                } else {
                    info!("Received event: {:?}", data);
                    socket.emit("message-back", data).ok();
                }
                Ok(Ack::<()>::None)
            });

            socket.on("message-with-ack", |_, data: Value, bin| async move {
                if let Some(bin) = bin {
                    info!("Received event binary: {:?} {:?}", data, bin);
                    return Ok(Ack::DataBin(data, bin));
                } else {
                    info!("Received event: {:?}", data);
                    return Ok(Ack::Data(data));
                }
            });
        })
        .add("/custom", |socket| async move {
            info!("Socket.IO connected on: {:?} {:?}", socket.ns, socket.sid);
            socket.emit("auth", socket.handshake.auth.clone()).ok();
        })
        .build();

    let app = axum::Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .layer(SocketIoLayer::from_config(config, ns));

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
use engineio_server::{
    errors::Error,
    layer::{EngineIoConfig, EngineIoHandler, EngineIoLayer},
    socket::Socket,
};
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

#[derive(Clone)]
struct MyHandler;

#[engineio_server::async_trait]
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
