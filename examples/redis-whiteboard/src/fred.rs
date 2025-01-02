//! A simple whiteboard example using Redis as the adapter.
//! It uses the fred crate to connect to a Redis server.
use std::str::FromStr;

use fred::{
    prelude::Config,
    types::{Builder, RespVersion},
};
use rmpv::Value;
use socketioxide::{
    adapter::Adapter,
    extract::{Data, SocketRef},
    ParserConfig, SocketIo,
};
use socketioxide_redis::{FredAdapter, RedisAdapterCtr};
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
    let mut config = Config::from_url("redis://127.0.0.1:6379")?;
    // We have to manually set the version to RESP3. Fred defaults to RESP2.
    config.version = RespVersion::RESP3;
    let client = Builder::default()
        .set_config(config)
        .build_subscriber_client()?;
    let adapter = RedisAdapterCtr::new_with_fred(client).await?;
    info!("starting server");

    let (layer, io) = SocketIo::builder()
        .with_parser(ParserConfig::msgpack())
        .with_adapter::<FredAdapter<_>>(adapter)
        .build_layer();

    // It is heavily recommended to use generic fns instead of closures for handlers.
    // This allows to be generic over the adapter you want to use.
    async fn on_drawing<A: Adapter>(s: SocketRef<A>, Data(data): Data<Value>) {
        s.broadcast().emit("drawing", &data).await.ok();
    }
    fn on_connect<A: Adapter>(s: SocketRef<A>) {
        s.on("drawing", on_drawing);
    }
    io.ns("/", on_connect);

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
