use std::time::Duration;

use engineioxide_client::EngineIoClientConfig;
use socketioxide::{
    SocketIo, TransportType,
    extract::{Data, SocketRef},
    service::SocketIoService,
};
use socketioxide_client::io;
use tracing_subscriber::EnvFilter;

const fn main() {}

#[tokio::test]
async fn int() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let (svc, server_io) = SocketIo::new_svc();

    server_io.ns("/", async |sock: SocketRef| {
        dbg!(&sock);
        sock.on("event", async |Data(event): Data<String>| {
            assert_eq!(event, "yolo");
        });
    });

    let socket = io(svc).await;

    socket.emit("event", "yolo").await.unwrap();
    tokio::time::sleep(Duration::from_millis(10000)).await;
}
