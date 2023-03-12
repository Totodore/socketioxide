use axum::routing::get;
use axum::Server;
use engineio_server::layer::EngineIoLayer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = axum::Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .layer(EngineIoLayer::new());

    Server::bind(&"0.0.0.0:3000".parse().unwrap())
        .serve(app.into_make_service())
        .await?;

    Ok(())
}
