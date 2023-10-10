mod handlers;

use axum::response::{Html, IntoResponse};
use axum::routing::get;
use axum::Server;
use futures::FutureExt;
use socketioxide::SocketIo;
use tokio::signal;
use tower::ServiceBuilder;
use tower_http::cors::CorsLayer;
use tracing::{error, info, Level};
use tracing_subscriber::FmtSubscriber;

async fn serve_html() -> impl IntoResponse {
    Html(include_str!("chat.html"))
}

fn set_shutdown_handler(io: SocketIo) -> tokio::sync::oneshot::Receiver<()> {
    let (close_tx, close_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        match signal::ctrl_c().await {
            Ok(()) => {}
            Err(err) => {
                error!("Unable to listen for shutdown signal: {}", err);
            }
        };

        io.close().await;
        close_tx.send(()).unwrap();
    });
    close_rx
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::builder()
        .with_line_number(true)
        .with_max_level(Level::DEBUG)
        .finish();

    tracing::subscriber::set_global_default(subscriber)?;

    info!("Starting server");

    let (io_layer, io) = SocketIo::builder().ns("/", handlers::handler).build_layer();

    // The shutdown handler is used to gracefully shutdown the server when a SIGINT or SIGTERM is emitted
    // The socket.io will then gracefully close all connections
    let on_shutdown = set_shutdown_handler(io.clone());

    let app = axum::Router::new()
        .route("/", get(serve_html))
        .layer(
            ServiceBuilder::new()
                .layer(CorsLayer::permissive()) // Enable CORS policy
                .layer(io_layer),
        )
        .with_state(io);

    let server = Server::bind(&"0.0.0.0:3000".parse().unwrap()).serve(app.into_make_service());

    let server = server.with_graceful_shutdown(on_shutdown.map(drop));
    if let Err(e) = server.await {
        error!("server error: {}", e);
    }

    Ok(())
}
