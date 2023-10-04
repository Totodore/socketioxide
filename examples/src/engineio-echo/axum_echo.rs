use axum::routing::get;
use axum::Server;
use engineioxide::{
    handler::EngineIoHandler,
    layer::EngineIoLayer,
    socket::{DisconnectReason, Socket},
};
use tracing::info;
use tracing_subscriber::FmtSubscriber;

#[derive(Debug, Clone)]
struct MyHandler;

#[engineioxide::async_trait]
impl EngineIoHandler for MyHandler {
    type Data = ();

    fn on_connect(&self, socket: &Socket<()>) {
        println!("socket connect {}", socket.sid);
    }
    fn on_disconnect(&self, socket: &Socket<()>, reason: DisconnectReason) {
        println!("socket disconnect {}: {:?}", socket.sid, reason);
    }

    fn on_message(&self, msg: String, socket: &Socket<()>) {
        println!("Ping pong message {:?}", msg);
        socket.emit(msg).ok();
    }

    fn on_binary(&self, data: Vec<u8>, socket: &Socket<()>) {
        println!("Ping pong binary message {:?}", data);
        socket.emit_binary(data).ok();
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing::subscriber::set_global_default(FmtSubscriber::default())?;

    info!("Starting server");
    let app = axum::Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .layer(EngineIoLayer::new(MyHandler));

    Server::bind(&"127.0.0.1:3000".parse().unwrap())
        .serve(app.into_make_service())
        .await?;

    Ok(())
}
