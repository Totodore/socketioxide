use hyper::server::conn::http1;
use hyper_util::rt::TokioIo;
use socketioxide::SocketIo;
use socketioxide_mongodb::drivers::mongodb::mongodb_client as mongodb;
use socketioxide_mongodb::{
    MessageExpirationStrategy, MongoDbAdapter, MongoDbAdapterConfig, MongoDbAdapterCtr,
};
use std::time::Duration;
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
    let variant = std::env::args().next().unwrap();
    let variant = variant.split("/").last().unwrap();

    const URI: &str = "mongodb://127.0.0.1:27017/?replicaSet=rs0&directConnection=true";
    let client = mongodb::Client::with_uri_str(URI).await?;
    let strategy = MessageExpirationStrategy::TtlIndex(Duration::from_secs(1));
    let collection = format!("socket.io-ttl-{variant}");
    let config = MongoDbAdapterConfig::new()
        .with_expiration_strategy(strategy)
        .with_collection(collection);
    let adapter =
        MongoDbAdapterCtr::new_with_mongodb_config(client.database("test"), config).await?;
    let (svc, io) = SocketIo::builder()
        .with_adapter::<MongoDbAdapter<_>>(adapter)
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
