use serde_json::Value;
use socketioxide::{
    extract::{AckSender, Bin, Data, SocketRef},
    SocketIo,
};
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::FmtSubscriber;
use viz::{handler::ServiceHandler, serve_with_upgrades, Result, Router, Tree};

fn on_connect(socket: SocketRef, Data(data): Data<Value>) {
    info!("Socket.IO connected: {:?} {:?}", socket.ns(), socket.id);
    socket.emit("auth", data).ok();

    socket.on(
        "message",
        |socket: SocketRef, Data::<Value>(data), Bin(bin)| {
            info!("Received event: {:?} {:?}", data, bin);
            socket.bin(bin).emit("message-back", data).ok();
        },
    );

    socket.on(
        "message-with-ack",
        |Data::<Value>(data), ack: AckSender, Bin(bin)| {
            info!("Received event: {:?} {:?}", data, bin);
            ack.bin(bin).send(data).ok();
        },
    );
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing::subscriber::set_global_default(FmtSubscriber::default())?;

    let (svc, io) = SocketIo::new_svc();

    io.ns("/", on_connect);
    io.ns("/custom", on_connect);

    let app = Router::new()
        .get("/", |_| async { Ok("Hello, World!") })
        .any("/*", ServiceHandler::new(svc));
    let tree = Arc::new(Tree::from(app));

    info!("Starting server");

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();

    loop {
        let (stream, addr) = listener.accept().await?;
        let tree = tree.clone();
        tokio::task::spawn(serve_with_upgrades(stream, tree, Some(addr)));
    }
}
