use std::sync::Arc;

use hyper::Server;
use serde_json::Value;
use socketioxide::{AckSender, Socket, SocketIo};
use tracing::info;
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing::subscriber::set_global_default(FmtSubscriber::default())?;

    let (service, io) = SocketIo::new_svc();
    io.ns("/", |socket: Arc<Socket>, auth: Value| async move {
        info!("Socket.IO connected: {:?} {:?}", socket.ns(), socket.id);
        socket.emit("auth", auth).ok();

        socket.on(
            "message",
            |socket: Arc<Socket>, data: Value, bin, _| async move {
                info!("Received event: {:?} {:?}", data, bin);
                socket.bin(bin).emit("message-back", data).ok();
            },
        );

        socket.on(
            "message-with-ack",
            |_: Arc<Socket>, data: Value, bin, ack: AckSender| async move {
                info!("Received event: {:?} {:?}", data, bin);
                ack.bin(bin).send(data).ok();
            },
        );

        socket.on_disconnect(|socket, reason| async move {
            info!("Socket.IO disconnected: {} {}", socket.id, reason);
        });
    });

    io.ns("/custom", |socket: Arc<Socket>, auth: Value| async move {
        info!("Socket.IO connected on: {:?} {:?}", socket.ns(), socket.id);
        socket.emit("auth", auth).ok();
    });
    info!("Starting server");

    Server::bind(&"127.0.0.1:3000".parse().unwrap())
        .serve(service.into_make_service())
        .await?;

    Ok(())
}
