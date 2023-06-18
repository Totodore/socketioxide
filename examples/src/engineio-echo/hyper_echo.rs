use std::sync::Arc;

use engineioxide::{handler::EngineIoHandler, service::EngineIoService, socket::Socket};
use hyper::Server;
use tracing::info;
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
    tracing::subscriber::set_global_default(FmtSubscriber::default())?;

    // We'll bind to 127.0.0.1:3000
    let addr = &"127.0.0.1:3000".parse().unwrap();
    let handler = Arc::new(MyHandler);
    let svc = EngineIoService::new(handler);

    let server = Server::bind(addr).serve(svc.into_make_service());

    info!("Starting server");

    server.await?;

    Ok(())
}
