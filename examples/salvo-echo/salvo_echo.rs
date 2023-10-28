use salvo::prelude::*;
use salvo::tower_compat::TowerServiceCompat;

use std::sync::Arc;

use engineioxide::{
    handler::EngineIoHandler,
    service::EngineIoService,
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
        .with_max_level(Level::DEBUG)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    let svc = EngineIoService::new(MyHandler).compat();

    let router = Router::new().get(hello);
    let acceptor = TcpListener::new("127.0.0.1:5800").bind().await;
    Server::new(acceptor).serve(router).await;

    Ok(())
}
