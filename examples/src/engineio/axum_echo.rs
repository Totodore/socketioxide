use std::{sync::Arc, time::Duration};

use axum::routing::get;
use axum::Server;
use engineioxide::utils::SnowflakeGenerator;
use engineioxide::{
    layer::{EngineIoConfig, EngineIoHandler, EngineIoLayer},
    socket::Socket,
};
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

#[derive(Clone)]
struct MyHandler;

#[engineioxide::async_trait]
impl EngineIoHandler for MyHandler {
    type Data = ();

    fn on_connect(self: Arc<Self>, socket: &Socket<Self, i64>) {
        println!("socket connect {}", socket.sid);
    }
    fn on_disconnect(self: Arc<Self>, socket: &Socket<Self, i64>) {
        println!("socket disconnect {}", socket.sid);
    }

    async fn on_message(self: Arc<Self>, msg: String, socket: &Socket<Self, i64>) {
        println!("Ping pong message {:?}", msg);
        socket.emit(msg).ok();
    }

    async fn on_binary(self: Arc<Self>, data: Vec<u8>, socket: &Socket<Self, i64>) {
        println!("Ping pong binary message {:?}", data);
        socket.emit_binary(data).ok();
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::builder()
        .with_line_number(true)
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    let config = EngineIoConfig::builder()
        .ping_interval(Duration::from_millis(300))
        .ping_timeout(Duration::from_millis(200))
        .max_payload(1e6 as u64)
        .build();
    info!("Starting server");
    let g = SnowflakeGenerator::default();
    let app = axum::Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .layer(EngineIoLayer::from_config(MyHandler, config, g));

    Server::bind(&"0.0.0.0:3000".parse().unwrap())
        .serve(app.into_make_service())
        .await?;

    Ok(())
}
