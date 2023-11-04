use axum::Server;

use socketioxide::SocketIo;
use tower::ServiceBuilder;
use tower_http::{cors::CorsLayer, services::ServeDir};
use tracing::{error, info};
use tracing_subscriber::FmtSubscriber;

mod handlers;
mod store;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::new();

    tracing::subscriber::set_global_default(subscriber)?;

    info!("Starting server");

    let (layer, io) = SocketIo::new_layer();

    io.ns("/", handlers::on_connection);

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
