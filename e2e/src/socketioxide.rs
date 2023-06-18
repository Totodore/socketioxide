//! This a end to end test server used with this [test suite](https://github.com/socketio/socket.io-protocol)

use std::time::Duration;

use hyper::Server;
use serde_json::Value;
use socketioxide::{Namespace, SocketIoConfig, SocketIoService};
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

    let svc = SocketIoService::with_config(ns, config);
    Server::bind(&"127.0.0.1:3000".parse().unwrap())
        .serve(svc.into_make_service())
        .await?;

    Ok(())
}
