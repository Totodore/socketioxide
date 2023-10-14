use hyper::Server;
use serde_json::Value;
use socketioxide::SocketIo;
use tracing::info;
use tracing_subscriber::FmtSubscriber;
use warp::Filter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing::subscriber::set_global_default(FmtSubscriber::default())?;

    let filter = warp::any().map(|| "Hello From Warp!");
    let warp_svc = warp::service(filter);

    let (service, io) = SocketIo::new_inner_svc(warp_svc);
    io.ns("/", |socket, auth: Value| async move {
        info!("Socket.IO connected: {:?} {:?}", socket.ns(), socket.sid);
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
            info!("Socket.IO disconnected: {} {}", socket.sid, reason);
        });
    });

    io.ns("/custom", |socket, auth: Value| async move {
        info!("Socket.IO connected on: {:?} {:?}", socket.ns(), socket.sid);
        socket.emit("auth", auth).ok();
    });

    info!("Starting server");

    Server::bind(&"127.0.0.1:3000".parse().unwrap())
        .serve(service.into_make_service())
        .await?;

    Ok(())
}
