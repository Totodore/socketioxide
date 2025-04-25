use fred::types::RespVersion;
use hyper::server::conn::http1;
use hyper_util::rt::TokioIo;
use socketioxide::SocketIo;
use socketioxide_redis::drivers::fred::fred_client as fred;
use socketioxide_redis::{RedisAdapterConfig, RedisAdapterCtr};
use tokio::net::TcpListener;
use tracing::{Level, info};
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::builder()
        .with_line_number(true)
        .with_max_level(Level::TRACE)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;
    let server_config = fred::prelude::ServerConfig::new_clustered(vec![
        ("127.0.0.1", 7000),
        ("127.0.0.1", 7001),
        ("127.0.0.1", 7002),
        ("127.0.0.1", 7003),
        ("127.0.0.1", 7004),
        ("127.0.0.1", 7005),
    ]);
    let mut config = fred::prelude::Config::default();
    config.server = server_config;
    config.version = RespVersion::RESP3;
    let client = fred::prelude::Builder::from_config(config).build_subscriber_client()?;
    let variant = std::env::args().next().unwrap();
    let variant = variant.split("/").last().unwrap();
    let config = RedisAdapterConfig::new().with_prefix(format!("socket.io-{variant}"));
    let adapter = RedisAdapterCtr::new_with_fred_config(client, config).await?;

    let (svc, io) = SocketIo::builder()
        .with_adapter::<socketioxide_redis::FredAdapter<_>>(adapter)
        .build_svc();

    io.ns("/", adapter_e2e::handler).await.unwrap();

    info!("Starting server with v5 protocol");
    let port: u16 = std::env::var("PORT")
        .expect("a PORT env var should be set")
        .parse()
        .unwrap();

    let listener = TcpListener::bind(("127.0.0.1", port)).await?;

    // We start a loop to continuously accept incoming connections
    loop {
        let (stream, _) = listener.accept().await?;

        // Use an adapter to access something implementing `tokio::io` traits as if they implement
        // `hyper::rt` IO traits.
        let io = TokioIo::new(stream);
        let svc = svc.clone();

        // Spawn a tokio task to serve multiple connections concurrently
        tokio::task::spawn(async move {
            // Finally, we bind the incoming connection to our `hello` service
            if let Err(err) = http1::Builder::new()
                .serve_connection(io, svc)
                .with_upgrades()
                .await
            {
                println!("Error serving connection: {:?}", err);
            }
        });
    }
}
