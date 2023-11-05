use std::sync::Arc;

use salvo::prelude::*;
use serde_json::Value;
use socketioxide::{Socket, SocketIo};

use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

#[handler]
async fn hello() -> &'static str {
    "Hello World"
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::builder()
        .with_line_number(true)
        .with_max_level(Level::TRACE)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    let (layer, io) = SocketIo::new_layer();

    io.ns("/", |socket: Arc<Socket>, auth: Value| async move {
        info!("Socket.IO connected: {:?} {:?}", socket.ns(), socket.id);
        socket.emit("auth", auth).ok();

        socket.on("message", |socket, data: Value, bin, _| async move {
            info!("Received event: {:?} {:?}", data, bin);
            socket.bin(bin).emit("message-back", data).ok();
        });

        socket.on("message-with-ack", |_, data: Value, bin, ack| async move {
            info!("Received event: {:?} {:?}", data, bin);
            ack.bin(bin).send(data).ok();
        });

        socket.on_disconnect(|socket, reason| async move {
            info!("Socket.IO disconnected: {} {}", socket.id, reason);
        });
    });

    io.ns("/custom", |socket: Arc<Socket>, auth: Value| async move {
        info!("Socket.IO connected on: {:?} {:?}", socket.ns(), socket.id);
        socket.emit("auth", auth).ok();
    });

    let layer = layer.with_hyper_v1().compat();
    let router = Router::with_path("/socket.io").hoop(layer).goal(hello);
    let acceptor = TcpListener::new("127.0.0.1:3000").bind().await;
    Server::new(acceptor).serve(router).await;

    Ok(())
}
