mod handlers;

use axum::response::{Html, IntoResponse};
use axum::routing::get;
use axum::Server;
use socketioxide::{Namespace, SocketIoLayer};
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
    let ns = Namespace::builder().add("/", handlers::handler).build();

    let app = axum::Router::new()
        .route("/", get(serve_html))
        .layer(SocketIoLayer::new(ns));

    Server::bind(&"0.0.0.0:3000".parse().unwrap())
        .serve(app.into_make_service())
        .await?;

    Ok(())
}
