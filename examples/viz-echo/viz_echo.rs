use socketioxide::{
    extract::{AckSender, Data, SocketRef},
    PayloadValue, SocketIo,
};
use tracing::{error, info};
use tracing_subscriber::FmtSubscriber;
use viz::{handler::ServiceHandler, serve, Result, Router};

fn on_connect(socket: SocketRef, Data(data): Data<PayloadValue>) {
    info!("Socket.IO connected: {:?} {:?}", socket.ns(), socket.id);
    socket.emit("auth", data).ok();

    socket.on(
        "message",
        |socket: SocketRef, Data::<PayloadValue>(data)| {
            info!("Received event: {:?}", data);
            socket.emit("message-back", data).ok();
        },
    );

    socket.on(
        "message-with-ack",
        |Data::<PayloadValue>(data), ack: AckSender| {
            info!("Received event: {:?}", data);
            ack.send(data).ok();
        },
    );
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing::subscriber::set_global_default(FmtSubscriber::default())?;

    let (svc, io) = SocketIo::new_svc();

    io.ns("/", on_connect);
    io.ns("/custom", on_connect);

    let app = Router::new()
        .get("/", |_| async { Ok("Hello, World!") })
        .any("/*", ServiceHandler::new(svc));

    info!("Starting server");

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();

    if let Err(e) = serve(listener, app).await {
        error!("{}", e);
    }

    Ok(())
}
