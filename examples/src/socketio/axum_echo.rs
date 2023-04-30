use std::time::Duration;

use axum::routing::get;
use axum::Server;
use serde_json::Value;
use socketio_server::{Ack, Namespace, SocketIoConfig, SocketIoLayer};
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
        .add("/", |socket| async move {
            info!("Socket.IO connected: {:?} {:?}", socket.ns(), socket.sid);
            let data: Value = socket.handshake.data().unwrap();
            socket.emit("auth", data).ok();

            socket.on_event("message", |socket, data: Value, bin| async move {
                if let Some(bin) = bin {
                    info!("Received event binary: {:?} {:?}", data, bin);
                    socket.bin(bin).emit("message-back", data).ok();
                } else {
                    info!("Received event: {:?}", data);
                    socket.emit("message-back", data).ok();
                }
                Ok(Ack::<()>::None)
            });

            socket.on_event("message-with-ack", |_, data: Value, bin| async move {
                if let Some(bin) = bin {
                    info!("Received event binary: {:?} {:?}", data, bin);
                    return Ok(Ack::DataBin(data, bin));
                } else {
                    info!("Received event: {:?}", data);
                    return Ok(Ack::Data(data));
                }
            });
        })
        .add("/custom", |socket| async move {
            info!("Socket.IO connected on: {:?} {:?}", socket.ns(), socket.sid);
            let data: Value = socket.handshake.data().unwrap();
            socket.emit("auth", data).ok();
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
