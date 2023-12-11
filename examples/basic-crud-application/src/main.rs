use axum::Server;

use socketioxide::{extract::SocketRef, SocketIo};
use tower::ServiceBuilder;
use tower_http::{cors::CorsLayer, services::ServeDir};
use tracing::{error, info};
use tracing_subscriber::FmtSubscriber;

use crate::handlers::todo::Todos;

mod handlers;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::new();

    tracing::subscriber::set_global_default(subscriber)?;

    info!("Starting server");

    let (layer, io) = SocketIo::builder()
        .with_state(Todos::default())
        .build_layer();

    io.ns("/", |s: SocketRef| {
        s.on("todo:create", handlers::todo::create);
        s.on("todo:read", handlers::todo::read);
        s.on("todo:update", handlers::todo::update);
        s.on("todo:delete", handlers::todo::delete);
        s.on("todo:list", handlers::todo::list);
    });

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
