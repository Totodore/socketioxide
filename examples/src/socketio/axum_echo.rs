use std::time::Duration;

use axum::routing::get;
use axum::Server;
use socketio_server::{config::SocketIoConfig, layer::SocketIoLayer, ns::Namespace};
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::builder()
        .with_line_number(true)
        .with_max_level(Level::DEBUG)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    let config = SocketIoConfig::builder()
        .ping_interval(Duration::from_millis(300))
        .ping_timeout(Duration::from_millis(200))
        .max_payload(1e6 as u64)
        .build();
    info!("Starting server");

    let ns = Namespace::builder()
        .add("/", |socket| {
            info!("Socket.IO connected: {:?} {:?}", socket.ns, socket.sid);
            socket.emit("auth", ());
            socket.on_message(|socket, e, data| {
                info!("Received event: {:?} {:?}", e, data);
                socket.emit(e, data);
            });
        })
        .build();

    let app = axum::Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .layer(SocketIoLayer::from_config(config, ns));

    Server::bind(&"0.0.0.0:3000".parse().unwrap())
        .serve(app.into_make_service())
        .await?;

    Ok(())
}
