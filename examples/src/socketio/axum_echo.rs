use std::time::Duration;

use axum::routing::get;
use axum::Server;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use socketio_server::{config::SocketIoConfig, layer::SocketIoLayer, ns::Namespace};
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

#[derive(Serialize, Deserialize)]
struct TestMessage {
    test: String,
}

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
        .add("/", |socket| async move {
            info!("Socket.IO connected: {:?} {:?}", socket.ns, socket.sid);

            socket
                .emit("auth", socket.handshake.auth.clone())
                .await
                .ok();

            socket.on_event("message", |socket, data: Value, bin| async move {
                if let Some(bin) = bin {
                    info!("Received event binary: {:?} {:?}", data, bin);
                    socket.emit_bin("message-back", data, bin).await.ok();
                } else {
                    info!("Received event: {:?}", data);
                    socket.emit("message-back", data).await.ok();
                }
                Ok(())
            });

            socket.on_event("message-with-ack", |_, data: Value, _| async move {
                info!("Received event with ack: {:?}", data);
                Ok(data)
            });
        })
        .add("/custom", |socket| async move {
            info!("Socket.IO connected on: {:?} {:?}", socket.ns, socket.sid);
            socket
                .emit("auth", socket.handshake.auth.clone())
                .await
                .ok();
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
