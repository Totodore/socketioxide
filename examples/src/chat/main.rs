mod handlers;

use axum::response::{Html, IntoResponse};
use axum::routing::get;
use axum::Server;
use socketioxide::SocketIo;
use tower::ServiceBuilder;
use tower_http::cors::CorsLayer;
use tracing::info;
use tracing_subscriber::FmtSubscriber;

async fn serve_html() -> impl IntoResponse {
    Html(include_str!("chat.html"))
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::builder().finish();
    tracing::subscriber::set_global_default(subscriber)?;

    info!("Starting server");

    let (io_layer, io) = SocketIo::builder().ns("/", handlers::handler).build_layer();

    let app = axum::Router::new()
        .route("/", get(serve_html))
        .layer(
            ServiceBuilder::new()
                .layer(CorsLayer::permissive()) // Enable CORS policy
                .layer(io_layer),
        )
        .with_state(io);

    Server::bind(&"0.0.0.0:3000".parse().unwrap())
        .serve(app.into_make_service())
        .await?;

    Ok(())
}
