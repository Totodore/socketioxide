use axum::routing::get;
use axum::Server;
use engineio_server::{layer::{EngineIoLayer, EngineIoHandler}, socket::Socket};


#[derive(Clone)]
struct MyHandler;

#[engineio_server::async_trait]
impl EngineIoHandler for MyHandler {
    //TODO: Fix this generic
    async fn handle<EngineIoHandler>(&self, msg: String, socket: &mut Socket) -> Result<(), engineio_server::errors::Error> {
        //Ping pong message
        println!("Ping pong message {:?}", msg);
        socket.emit(msg).await
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = axum::Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .layer(EngineIoLayer::new(MyHandler));

    Server::bind(&"0.0.0.0:3000".parse().unwrap())
        .serve(app.into_make_service())
        .await?;

    Ok(())
}
