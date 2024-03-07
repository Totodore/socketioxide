use axum::routing::get;
use socketioxide::{
    extract::{AckSender, Data, SocketRef},
    PayloadValue, SocketIo,
};
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
        |ack: AckSender, Data::<PayloadValue>(data)| {
            info!("Received event: {:?}", data);
            ack.send(data).ok();
        },
    );
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing::subscriber::set_global_default(FmtSubscriber::default())?;

    let (layer, io) = SocketIo::new_layer();

    io.ns("/", on_connect);
    io.ns("/custom", on_connect);

    let app = axum::Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .layer(layer);

    info!("Starting server");

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();

    Ok(())
}
