use axum::Server;

use serde_json::Value;
use socketioxide::SocketIo;
use tower::ServiceBuilder;
use tower_http::{cors::CorsLayer, services::ServeDir};
use tracing::{error, info};
use tracing_subscriber::FmtSubscriber;

/// The `background_task` function is a simple example of a task taking a io handle
/// to manipulate all the connected sockets
async fn background_task(io: SocketIo) {
    let mut i = 0;
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        info!("Background task");
        let cnt = io.sockets().unwrap().len();
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

    io.ns("/", |s, data: Value| async move {
        info!("Received: {:?}", data);
        s.emit("welcome", data).ok();
    });

    tokio::spawn(background_task(io));

    let app = axum::Router::new()
        .nest_service("/", ServeDir::new("dist"))
        .layer(
            ServiceBuilder::new()
                .layer(CorsLayer::permissive()) // Enable CORS policy
                .layer(layer),
        );

    let server = Server::bind(&"0.0.0.0:3000".parse().unwrap()).serve(app.into_make_service());

    if let Err(e) = server.await {
        error!("server error: {}", e);
    }

    Ok(())
}
