use std::net::SocketAddr;

use hyper::server::conn::http1;
use hyper_util::rt::TokioIo;
use serde_json::Value;
use socketioxide::SocketIo;
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

    let (svc, io) = SocketIo::new_svc();

    io.ns("/", |socket, auth: Value| async move {
        info!("Socket.IO connected: {:?} {:?}", socket.ns(), socket.id);
        socket.emit("auth", auth).ok();

        socket.on("message", |socket, data: Value, bin, _| async move {
            info!("Received event: {:?} {:?}", data, bin);
            socket.bin(bin).emit("message-back", data).ok();
        });

        socket.on("message-with-ack", |_, data: Value, bin, ack| async move {
            info!("Received event: {:?} {:?}", data, bin);
            ack.bin(bin).send(data).ok();
        });

        socket.on_disconnect(|socket, reason| async move {
            info!("Socket.IO disconnected: {} {}", socket.id, reason);
        });
    });

    io.ns("/custom", |socket, auth: Value| async move {
        info!("Socket.IO connected on: {:?} {:?}", socket.ns(), socket.id);
        socket.emit("auth", auth).ok();
    });

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    let listener = TcpListener::bind(addr).await?;

    // Convert the `SocketIoService` so it works with hyper 1.0
    let svc = svc.with_hyper_v1();

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
