//! This a end to end test server used with this [test suite](https://github.com/socketio/socket.io-protocol)

use std::time::Duration;

use hyper::Server;
use serde_json::Value;
use socketioxide::{
    extract::{AckSender, Bin, Data, SocketRef},
    SocketIo,
};
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

    socket.on(
        "message-with-ack",
        |Data::<Value>(data), ack: AckSender, Bin(bin)| {
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

    Server::bind(&"127.0.0.1:3000".parse().unwrap())
        .serve(svc.into_make_service())
        .await?;

    Ok(())
}
