//! This a end to end test server used with this [test suite](https://github.com/socketio/socket.io-protocol)

use std::time::Duration;

use hyper::server::conn::http1;
use hyper_util::rt::TokioIo;
use serde_json::Value;
use socketioxide::{
    extract::{AckSender, Bin, Data, SocketRef},
    SocketIo,
};
use tokio::net::TcpListener;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

fn on_connect(socket: SocketRef, Data(data): Data<Value>) {
    info!("Socket.IO connected: {:?} {:?}", socket.ns(), socket.id);
    socket.emit("auth", data).ok();

    socket.on(
        "message",
        |socket: SocketRef, Data::<Value>(data), Bin(bin)| {
            info!("Received event: {:?} {:?}", data, bin);
            socket.bin(bin).emit("message-back", data).ok();
        },
    );

    // keep this handler async to test async message handlers
    socket.on(
        "message-with-ack",
        |Data::<Value>(data), ack: AckSender, Bin(bin)| async move {
            info!("Received event: {:?} {:?}", data, bin);
            ack.bin(bin).send(data).ok();
        },
    );
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::builder()
        .with_line_number(true)
        .with_max_level(Level::DEBUG)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    let (svc, io) = SocketIo::builder()
        .ping_interval(Duration::from_millis(300))
        .ping_timeout(Duration::from_millis(200))
        .connect_timeout(Duration::from_millis(1000))
        .max_payload(1e6 as u64)
        .build_svc();

    io.ns("/", on_connect);
    io.ns("/custom", on_connect);

    #[cfg(feature = "v5")]
    info!("Starting server with v5 protocol");
    #[cfg(feature = "v4")]
    info!("Starting server with v4 protocol");
    let listener = TcpListener::bind("127.0.0.1:3000").await?;

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
