use std::{sync::Arc, time::Duration};

use hyper::server::conn::http1;
use hyper_util::rt::TokioIo;
use rmpv::Value;
use socketioxide::{
    adapter::Adapter,
    extract::{AckSender, Data, SocketRef},
    SocketIo,
};
use socketioxide_adapter_redis::{
    drivers::redis::RedisDriver, RedisAdapter, RedisAdapterConfig, RedisAdapterState,
};
use tokio::net::TcpListener;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

async fn on_connect<A: Adapter>(socket: SocketRef<A>, Data(data): Data<Value>, io: SocketIo<A>) {
    info!(?data, ns = socket.ns(), ?socket.id, "Socket.IO connected:");
    socket.emit("auth", &data).ok();

    socket.on(
        "message",
        |socket: SocketRef<_>, Data::<[Value; 3]>(data)| {
            info!(?data, "Received event:");
            socket.emit("message-back", &data).unwrap();
        },
    );

    // keep this handler async to test async message handlers
    socket.on(
        "message-with-ack",
        |Data::<[Value; 3]>(data), ack: AckSender<_>| async move {
            info!(?data, "Received event:");
            ack.send(&data).unwrap();
        },
    );

    socket.on(
        "emit-with-ack",
        |s: SocketRef<_>, Data::<[Value; 3]>(data)| async move {
            info!(?data, "Received event:");
            let ack: [Value; 3] = s
                .emit_with_ack("emit-with-ack", &data)
                .unwrap()
                .await
                .unwrap();
            s.emit("emit-with-ack", &ack).unwrap();
        },
    );

    let mut int = tokio::time::interval(Duration::from_secs(1));
    loop {
        int.tick().await;
        io.broadcast().emit("test", &socket.id).await.unwrap();
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::builder()
        .with_line_number(true)
        .with_max_level(Level::TRACE)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    let client = redis::Client::open("redis://127.0.0.1:6379?protocol=resp3").unwrap();
    let driver = RedisDriver::new(client).await.unwrap();
    let adapter = RedisAdapterState::new(Arc::from(driver), RedisAdapterConfig::default());

    let (svc, io) = SocketIo::builder()
        .ping_interval(Duration::from_millis(300))
        .ping_timeout(Duration::from_millis(200))
        .ack_timeout(Duration::from_millis(200))
        .connect_timeout(Duration::from_millis(1000))
        .max_payload(1e6 as u64)
        .with_adapter::<RedisAdapter<_, RedisDriver>>(adapter)
        .build_svc();

    io.ns("/", on_connect);
    io.ns("/custom", on_connect);

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
