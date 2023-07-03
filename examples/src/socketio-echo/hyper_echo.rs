use hyper::Server;
use serde_json::Value;
use socketioxide::{Namespace, SocketIoService};
use tracing::info;
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing::subscriber::set_global_default(FmtSubscriber::default())?;

    let ns = Namespace::builder()
        .add("/", |socket| async move {
            info!("Socket.IO connected: {:?} {:?}", socket.ns(), socket.sid);
            let data: Value = socket.handshake.data().unwrap().unwrap();
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
            let data: Value = socket.handshake.data().unwrap().unwrap();
            socket.emit("auth", data).ok();
        })
        .build();

    info!("Starting server");

    let svc = SocketIoService::new(ns);
    Server::bind(&"127.0.0.1:3000".parse().unwrap())
        .serve(svc.into_make_service())
        .await?;

    Ok(())
}
