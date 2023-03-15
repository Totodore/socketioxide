use axum::routing::get;
use axum::Server;
use engineio_server::{
    errors::Error,
    layer::{EngineIoHandler, EngineIoLayer},
    socket::Socket,
};
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

#[derive(Clone)]
struct MyHandler;

#[engineio_server::async_trait]
impl EngineIoHandler for MyHandler {
    //TODO: Fix this generic
    async fn handle<EngineIoHandler>(&self, msg: String, socket: &mut Socket) -> Result<(), Error> {
        //Ping pong message
        println!("Ping pong message {:?}", msg);
        socket.emit(msg).await
    }

    async fn handle_binary<H>(&self, data: Vec<u8>, socket: &mut Socket) -> Result<(), Error> {
        println!("Ping pong binary message {:?}", data);
        socket.emit_binary(data).await
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::builder()
        .with_line_number(true)
        .with_max_level(Level::DEBUG)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    info!("Starting server");
    let app = axum::Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .layer(EngineIoLayer::new(MyHandler));

    Server::bind(&"0.0.0.0:3000".parse().unwrap())
        .serve(app.into_make_service())
        .await?;

    Ok(())
}
