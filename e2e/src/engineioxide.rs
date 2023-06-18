//! This a end to end test server used with this [test suite](https://github.com/socketio/engine.io-protocol)

use std::{sync::Arc, time::Duration};

use engineioxide::{
    config::EngineIoConfig, handler::EngineIoHandler, service::EngineIoService, socket::Socket,
};
use hyper::Server;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

#[derive(Debug, Clone)]
struct MyHandler;

#[engineioxide::async_trait]
impl EngineIoHandler for MyHandler {
    type Data = ();

    fn on_connect(&self, socket: &Socket<Self>) {
        println!("socket connect {}", socket.sid);
    }
    fn on_disconnect(&self, socket: &Socket<Self>) {
        println!("socket disconnect {}", socket.sid);
    }

    fn on_message(&self, msg: String, socket: &Socket<Self>) {
        println!("Ping pong message {:?}", msg);
        socket.emit(msg).ok();
    }

    fn on_binary(&self, data: Vec<u8>, socket: &Socket<Self>) {
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
    let handler = Arc::new(MyHandler);
    let svc = EngineIoService::with_config(handler, config);

    let server = Server::bind(addr).serve(svc.into_make_service());
    tracing::subscriber::set_global_default(subscriber)?;

    info!("Starting server");

    server.await?;

    Ok(())
}