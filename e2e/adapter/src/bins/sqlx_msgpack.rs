use hyper::server::conn::http1;
use hyper_util::rt::TokioIo;
use socketioxide::{ParserConfig, SocketIo};

use socketioxide_postgres::{
    PostgresAdapterConfig, PostgresAdapterCtr, SqlxAdapter, drivers::sqlx::sqlx_client::PgPool,
};
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
    let config = PostgresAdapterConfig::new().with_prefix(format!("socket.io-{variant}"));

    let pg_pool = PgPool::connect("postgres://socketio:socketio@localhost:5432/socketio").await?;
    let adapter = PostgresAdapterCtr::new_with_sqlx_config(pg_pool, config);
    let (svc, io) = SocketIo::builder()
        .with_parser(ParserConfig::msgpack())
        .with_adapter::<SqlxAdapter<_>>(adapter)
        .build_svc();

    io.ns("/", adapter_e2e::handler).await?;

    info!("Starting server with v5 protocol");
    let port: u16 = std::env::var("PORT")
        .expect("a PORT env var should be set")
        .parse()?;

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
