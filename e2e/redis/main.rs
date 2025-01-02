use std::time::Duration;

use fred::types::RespVersion;
use hyper::server::conn::http1;
use hyper_util::rt::TokioIo;
use socketioxide::SocketIo;
use socketioxide_redis::RedisAdapterCtr;
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

    #[cfg(feature = "fred")]
    let (_svc, _io) = {
        let mut config = fred::prelude::Config::from_url("redis://127.0.0.1:6379?protocol=resp3")?;
        config.version = RespVersion::RESP3;
        let client = fred::prelude::Builder::from_config(config).build_subscriber_client()?;
        let adapter = RedisAdapterCtr::new_with_fred(client).await?;

        SocketIo::builder()
            .with_adapter::<socketioxide_redis::FredAdapter<_>>(adapter)
            .build_svc()
    };

    #[cfg(feature = "redis")]
    let (_svc, _io) = {
        let client = redis::Client::open("redis://127.0.0.1:6379?protocol=resp3")?;
        let adapter = RedisAdapterCtr::new_with_redis(&client).await?;
        SocketIo::builder()
            .with_adapter::<socketioxide_redis::RedisAdapter<_>>(adapter)
            .build_svc()
    };

    _io.ns("/", || ());

    tokio::task::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let server = _io.config().server_id;
            let sockets = _io.fetch_sockets().await.unwrap();
            dbg!(&sockets);
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
        let svc = _svc.clone();

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
