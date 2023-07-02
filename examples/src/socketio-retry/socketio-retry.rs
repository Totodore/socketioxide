//! This an example how to manually handle failed sent Packets

use std::time::Duration;

use hyper::Server;
use serde_json::Value;
use socketioxide::{
    AckSenderError, BroadcastError, Namespace, SendError, SocketIoConfig, SocketIoService,
    TransportError,
};
use tokio::time::sleep;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::builder()
        .with_line_number(true)
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    let config = SocketIoConfig::builder()
        // set buffer size, it will force TransportError
        .max_buffer_size(1usize)
        .build();
    info!("Starting server");

    let ns = Namespace::builder()
        .add("/", |socket| async move {
            info!("Socket.IO connected: {:?} {:?}", socket.ns(), socket.sid);
            let data: Value = socket.handshake.data().unwrap();
            socket.emit("auth", data).ok();

            socket.on("message", |socket, data: Value, bin, _| async move {
                info!("Received event: {:?} {:?}", data, bin);
                if let Err(BroadcastError::SendError(mut errors)) =
                    socket.bin(bin).emit("message-back", data)
                {
                    let err = errors.pop().unwrap();
                    if let SendError::TransportError(TransportError::SendFailedBinPayloads(_)) = err
                    {
                        while let Err(TransportError::SendFailedBinPayloads(_)) =
                            socket.retry_failed()
                        {
                            sleep(Duration::from_millis(10)).await;
                            info!("retry");
                        }
                    }
                }
            });

            socket.on("message-with-ack", |_, data: Value, bin, ack| async move {
                info!("Received event: {:?} {:?}", data, bin);
                if let Err(AckSenderError::SendError {
                    send_error: SendError::TransportError(TransportError::SendFailedBinPayloads(_)),
                    socket,
                }) = ack.bin(bin).send(data)
                {
                    while let Err(TransportError::SendFailedBinPayloads(_)) = socket.retry_failed()
                    {
                        sleep(Duration::from_millis(10)).await;
                        info!("retry");
                    }
                }
            });
        })
        .add("/custom", |socket| async move {
            info!("Socket.IO connected on: {:?} {:?}", socket.ns(), socket.sid);
            let data: Value = socket.handshake.data().unwrap();
            socket.emit("auth", data).ok();
        })
        .build();

    let svc = SocketIoService::with_config(ns, config);
    Server::bind(&"127.0.0.1:3000".parse().unwrap())
        .serve(svc.into_make_service())
        .await?;

    Ok(())
}
