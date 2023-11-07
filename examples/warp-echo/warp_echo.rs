use hyper::Server;
use serde_json::Value;
use socketioxide::{
    extract::{AckSender, Bin, Data, SocketRef},
    SocketIo,
};
use tracing::info;
use tracing_subscriber::FmtSubscriber;
use warp::Filter;

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
    tracing::subscriber::set_global_default(FmtSubscriber::default())?;

    let filter = warp::any().map(|| "Hello From Warp!");
    let warp_svc = warp::service(filter);

    let (service, io) = SocketIo::new_inner_svc(warp_svc);

    io.ns("/", on_connect);
    io.ns("/custom", on_connect);

    info!("Starting server");

    Server::bind(&"127.0.0.1:3000".parse().unwrap())
        .serve(service.into_make_service())
        .await?;

    Ok(())
}
