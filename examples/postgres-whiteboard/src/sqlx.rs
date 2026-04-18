//! A simple whiteboard example using Postgres as the adapter.
//! It uses the sqlx crate to connect to a Postgres server.
use rmpv::Value;
use socketioxide::{
    adapter::Adapter,
    extract::{Data, SocketRef},
    ParserConfig, SocketIo,
};
use socketioxide_postgres::{drivers::sqlx::sqlx_client as sqlx, PostgresAdapterCtr, SqlxAdapter};
use std::str::FromStr;
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

    info!("connecting to postgres");
    let client = sqlx::Pool::connect("postgres://socketio:socketio@localhost/socketio").await?;
    let adapter = PostgresAdapterCtr::new_with_sqlx(client);
    info!("starting server");

    let (layer, io) = SocketIo::builder()
        .with_parser(ParserConfig::msgpack())
        .with_adapter::<SqlxAdapter<_>>(adapter)
        .build_layer();

    // It is heavily recommended to use generic fns instead of closures for handlers.
    // This allows to be generic over the adapter you want to use.
    async fn on_drawing<A: Adapter>(s: SocketRef<A>, Data(data): Data<Value>) {
        s.broadcast().emit("drawing", &data).await.ok();
    }
    async fn on_connect<A: Adapter>(s: SocketRef<A>) {
        s.on("drawing", on_drawing);
    }
    io.ns("/", on_connect).await?;

    let app = axum::Router::new()
        .fallback_service(ServeDir::new("dist"))
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
