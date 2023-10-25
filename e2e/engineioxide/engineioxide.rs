//! This a end to end test server used with this [test suite](https://github.com/socketio/engine.io-protocol)

use std::{sync::Arc, time::Duration};

use engineioxide::{
    config::EngineIoConfig,
    handler::EngineIoHandler,
    service::EngineIoService,
    socket::{DisconnectReason, Socket},
};
use hyper::Server;
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::builder()
        .with_line_number(true)
        .with_max_level(Level::DEBUG)
        .finish();

    let config = EngineIoConfig::builder()
        .ping_interval(Duration::from_millis(300))
        .ping_timeout(Duration::from_millis(200))
        .max_payload(1e6 as u64)
        .build();

    let addr = &"127.0.0.1:3000".parse().unwrap();
    let svc = EngineIoService::with_config(MyHandler, config);

    let server = Server::bind(addr).serve(svc.into_make_service());
    tracing::subscriber::set_global_default(subscriber)?;

    #[cfg(feature = "v3")]
    tracing::info!("Starting server with v3 protocol");
    #[cfg(feature = "v4")]
    tracing::info!("Starting server with v4 protocol");

    server.await?;

    Ok(())
}
