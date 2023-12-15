use serde_json::Value;
use socketioxide::{
    extract::{Data, SocketRef},
    SocketIo,
};
use tracing::info;
use tracing_subscriber::FmtSubscriber;

/// The `background_task` function is a simple example of a task taking a io handle
/// to manipulate all the connected sockets
async fn background_task(io: SocketIo) {
    let mut i = 0;
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        info!("Background task");
        let cnt = io.of("/").unwrap().sockets().unwrap().len();
        let msg = format!("{}s, {} socket connected", i, cnt);
        io.emit("tic tac !", msg).unwrap();

        i += 1;
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::new();

    tracing::subscriber::set_global_default(subscriber)?;

    info!("Starting server");

    let (layer, io) = SocketIo::new_layer();

    io.ns("/", |s: SocketRef, Data::<Value>(data)| {
        info!("Received: {:?}", data);
        s.emit("welcome", data).ok();
    });

    tokio::spawn(background_task(io));

    let app = axum::Router::new().layer(layer);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();

    Ok(())
}
