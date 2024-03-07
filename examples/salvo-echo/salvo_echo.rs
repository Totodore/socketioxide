use salvo::prelude::*;
use socketioxide::{
    extract::{AckSender, Data, SocketRef},
    PayloadValue, SocketIo,
};

use tower::ServiceBuilder;
use tower_http::cors::CorsLayer;
use tracing::info;
use tracing_subscriber::FmtSubscriber;

fn on_connect(socket: SocketRef, Data(data): Data<PayloadValue>) {
    info!("Socket.IO connected: {:?} {:?}", socket.ns(), socket.id);
    socket.emit("auth", data).ok();

    socket.on(
        "message",
        |socket: SocketRef, Data::<PayloadValue>(data)| {
            info!("Received event: {:?}", data);
            socket.emit("message-back", data).ok();
        },
    );

    socket.on(
        "message-with-ack",
        |Data::<PayloadValue>(data), ack: AckSender| {
            info!("Received event: {:?}", data);
            ack.send(data).ok();
        },
    );
}

#[handler]
async fn hello() -> &'static str {
    "Hello World"
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::new();
    tracing::subscriber::set_global_default(subscriber)?;

    let (layer, io) = SocketIo::new_layer();

    // This code is used to integrates other tower layers before or after Socket.IO such as CORS
    // Beware that classic salvo request won't pass through these layers
    let layer = ServiceBuilder::new()
        .layer(CorsLayer::permissive()) // Enable CORS policy
        .layer(layer); // Mount Socket.IO

    io.ns("/", on_connect);
    io.ns("/custom", on_connect);

    let layer = layer.compat();
    let router = Router::with_path("/socket.io").hoop(layer).goal(hello);
    let acceptor = TcpListener::new("127.0.0.1:3000").bind().await;
    Server::new(acceptor).serve(router).await;

    Ok(())
}
