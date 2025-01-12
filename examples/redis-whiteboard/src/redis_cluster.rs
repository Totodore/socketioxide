//! A simple whiteboard example using Redis as the adapter.
//! It uses the redis crate to connect to a Redis server.
use std::str::FromStr;

use rmpv::Value;
use socketioxide::{
    adapter::Adapter,
    extract::{Data, SocketRef},
    ParserConfig, SocketIo,
};
use socketioxide_redis::drivers::redis::redis_client as redis;
use socketioxide_redis::{ClusterAdapter, RedisAdapterCtr};
use tower::ServiceBuilder;
use tower_http::{cors::CorsLayer, services::ServeDir};
use tracing::info;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    info!("connecting to redis");
    // single node cluster. In a real world scenario, you would have multiple nodes.
    let builder = redis::cluster::ClusterClient::builder(std::iter::once(
        "redis://127.0.0.1:6379?protocol=resp3",
    ));
    let adapter = RedisAdapterCtr::new_with_cluster(builder).await?;
    info!("starting server");

    let (layer, io) = SocketIo::builder()
        .with_parser(ParserConfig::msgpack())
        .with_adapter::<ClusterAdapter<_>>(adapter)
        .build_layer();

    // It is heavily recommended to use generic fns instead of closures for handlers.
    // This allows to be generic over the adapter you want to use.
    async fn on_drawing<A: Adapter>(s: SocketRef<A>, Data(data): Data<Value>) {
        s.broadcast().emit("drawing", &data).await.ok();
    }
    fn on_connect<A: Adapter>(s: SocketRef<A>) {
        s.on("drawing", on_drawing);
    }
    io.ns("/", on_connect).await?;

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
    axum::serve(listener, app).await?;

    Ok(())
}
