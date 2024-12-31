use std::time::Duration;

use hyper::server::conn::http1;
use hyper_util::rt::TokioIo;
use socketioxide::SocketIo;
use socketioxide_redis::{RedisAdapter, RedisAdapterCtr};
use tokio::net::TcpListener;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::builder()
        .with_line_number(true)
        .with_max_level(Level::TRACE)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    let client = redis::Client::open("redis://127.0.0.1:6379?protocol=resp3")?;
    let adapter = RedisAdapterCtr::new(&client).await?;

    let (svc, io) = SocketIo::builder()
        .with_adapter::<RedisAdapter<_>>(adapter)
        .build_svc();

    io.ns("/", || ());

    tokio::task::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let server = io.config().server_id;
            let sockets = io.fetch_sockets().await.unwrap();
            for socket in sockets {
                socket
                    .emit("test", &format!("Hello from {server}"))
                    .await
                    .unwrap();
            }
        }
    });

    #[cfg(feature = "v5")]
    info!("Starting server with v5 protocol");
    #[cfg(feature = "v4")]
    info!("Starting server with v4 protocol");
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
