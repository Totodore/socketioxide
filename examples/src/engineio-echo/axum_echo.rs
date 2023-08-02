use axum::routing::get;
use axum::Server;
use engineioxide::{handler::EngineIoHandler, layer::EngineIoLayer, socket::Socket};
use tower::ServiceBuilder;
use tower_http::cors::CorsLayer;
use tracing::{info, Level};
use tracing_subscriber::{EnvFilter, FmtSubscriber};

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
    tracing::subscriber::set_global_default(
        FmtSubscriber::builder()
            .with_line_number(true)
            .with_max_level(Level::DEBUG)
            .with_env_filter(EnvFilter::from_default_env())
            .finish(),
    )?;

    info!("Starting server");
    let app = axum::Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .layer(
            ServiceBuilder::new()
                .layer(CorsLayer::very_permissive()) // Enable CORS policy
                .layer(EngineIoLayer::new(MyHandler)),
        );

    Server::bind(&"127.0.0.1:3000".parse().unwrap())
        .serve(app.into_make_service())
        .await?;

    Ok(())
}
