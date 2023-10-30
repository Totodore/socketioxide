use salvo::prelude::*;

use std::{sync::Arc, time::Duration};

use engineioxide::{
    config::EngineIoConfig,
    handler::EngineIoHandler,
    layer::EngineIoLayer,
    socket::{DisconnectReason, Socket},
};
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

#[derive(Debug, Clone)]
struct MyHandler;

#[engineioxide::async_trait]
impl EngineIoHandler for MyHandler {
    type Data = ();

    fn on_connect(&self, socket: Arc<Socket<Self::Data>>) {
        println!("socket connect {}", socket.id);
    }
    fn on_disconnect(&self, socket: Arc<Socket<Self::Data>>, reason: DisconnectReason) {
        println!("socket disconnect {}: {:?}", socket.id, reason);
    }

    fn on_message(&self, msg: String, socket: Arc<Socket<Self::Data>>) {
        println!("Ping pong message {:?}", msg);
        socket.emit(msg).ok();
    }

    fn on_binary(&self, data: Vec<u8>, socket: Arc<Socket<Self::Data>>) {
        println!("Ping pong binary message {:?}", data);
        socket.emit_binary(data).ok();
    }
}

#[handler]
async fn hello() -> &'static str {
    "Hello World"
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::builder()
        .with_line_number(true)
        .with_max_level(Level::TRACE)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    let config = EngineIoConfig::builder()
        .ping_interval(Duration::from_millis(300))
        .ping_timeout(Duration::from_millis(200))
        .max_payload(1e6 as u64)
        .build();

    let layer = EngineIoLayer::from_config(MyHandler, config)
        .with_hyper_v1()
        .compat();

    let router = Router::with_path("/engine.io").hoop(layer).goal(hello);
    let acceptor = TcpListener::new("127.0.0.1:3000").bind().await;
    Server::new(acceptor).serve(router).await;

    Ok(())
}
