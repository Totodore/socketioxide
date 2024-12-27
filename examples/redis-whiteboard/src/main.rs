use std::str::FromStr;

use rmpv::Value;
use socketioxide::{
    ParserConfig, SocketIo,
    extract::{Data, SocketRef},
};
use socketioxide_redis::{RedisAdapter, RedisAdapterCtr};
use tower::ServiceBuilder;
use tower_http::{cors::CorsLayer, services::ServeDir};
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    info!("connecting to redis");
    let client = redis::Client::open("redis://127.0.0.1:6379?protocol=resp3").unwrap();
    let adapter = RedisAdapterCtr::new(client).await.unwrap();
    info!("starting server");

    let (layer, io) = SocketIo::builder()
        .with_parser(ParserConfig::msgpack())
        .with_adapter::<RedisAdapter<_>>(adapter)
        .build_layer();

    io.ns("/", |s: SocketRef<RedisAdapter<_>>| {
        s.on(
            "drawing",
            |s: SocketRef<RedisAdapter<_>>, Data::<Value>(data)| async move {
                s.broadcast().emit("drawing", &data).await.unwrap();
            },
        );
    });

    let app = axum::Router::new()
        .nest_service("/", ServeDir::new("dist"))
        .layer(
            ServiceBuilder::new()
                .layer(CorsLayer::permissive()) // Enable CORS policy
                .layer(layer),
        );

    let port: u16 = std::env::var("PORT")
        .map(|s| u16::from_str(&s).unwrap())
        .unwrap_or(3000);
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port))
        .await
        .unwrap();
    axum::serve(listener, app).await.unwrap();

    Ok(())
}
