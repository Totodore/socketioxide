pub(crate) mod simulation;
pub(crate) mod utils;

use anyhow::Result;
use axum::routing::get;
use serde_json::Value;
use socketioxide::{
    extract::{AckSender, Bin, Data, SocketRef},
    SocketIo,
};
use tokio::sync::mpsc;
use tracing::info;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, FmtSubscriber};

use crate::simulation::ThreadEvent;

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
async fn main() -> Result<()> {
    let console_layer = console_subscriber::spawn();

    // build a `Subscriber` by combining layers with a
    // `tracing_subscriber::Registry`:
    tracing_subscriber::registry()
        .with(console_layer)
        .with(tracing_subscriber::fmt::layer().with_line_number(true))
        .init();

    let (layer, io) = SocketIo::new_layer();

    io.ns("/", on_connect);
    io.ns("/custom", on_connect);

    let app = axum::Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .layer(layer);

    info!("Starting simulation.");

    let (simulation_sender, simulation_receiver) = mpsc::unbounded_channel();

    let socket_sender = simulation_sender.clone();
    io.ns(
        "/simulation",
        |socket: SocketRef, Data(data): Data<Value>| {
            info!("Socket.IO connected: {:?} {:?}", socket.ns(), socket.id);
            socket.emit("auth", data).ok();

            socket.on("message", move |socket: SocketRef, Data::<Value>(data)| {
                if !data.is_u64() {
                    socket.emit("message-back", "Please send an u64").unwrap();
                    return;
                }

                socket_sender
                    .send(ThreadEvent::ChangeTargetTPS(data.as_u64().unwrap() as usize))
                    .unwrap();
                socket.emit("message-back", "Changing tick speed").unwrap();
            });
        },
    );

    let simulation_thread = tokio::spawn(simulation::runner::new(io, simulation_receiver));

    info!("Starting server");

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();

    simulation_sender.send(ThreadEvent::Shutdown)?;
    _ = tokio::join!(simulation_thread);

    Ok(())
}
