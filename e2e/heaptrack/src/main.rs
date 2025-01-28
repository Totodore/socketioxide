use hyper::server::conn::http1;
use hyper_util::rt::TokioIo;
use socketioxide::{extract::SocketRef, SocketIo};
use std::{net::SocketAddr, time::Duration};
use tokio::net::TcpListener;

fn on_connect(socket: SocketRef) {
    socket.on("ping", |s: SocketRef| {
        s.emit("pong", &()).ok();
    });
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    let (svc, io) = SocketIo::new_svc();

    io.ns("/", on_connect);

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    let listener = TcpListener::bind(addr).await?;

    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(60)).await;
        std::process::exit(0);
    });

    loop {
        let (stream, _) = listener.accept().await?;

        let io = TokioIo::new(stream);
        let svc = svc.clone();

        tokio::task::spawn(async move {
            http1::Builder::new()
                .serve_connection(io, svc)
                .with_upgrades()
                .await
                .ok()
        });
    }
}
