use axum::{
    extract::FromRequestParts,
    routing::get
};
use axum_session::{
    ReadOnlySession, Session, SessionConfig, SessionLayer, SessionNullPool, SessionStore,
};
use serde_json::Value;
use socketioxide::{
    extract::{Bin, Data, SocketRef},
    SocketIo,
};
use tracing::info;
use tracing_subscriber::FmtSubscriber;

async fn get_handler(session: Session<SessionNullPool>) -> &'static str {
    session.set("key", "value");

    "Hello, World!"
}

async fn on_connect(socket: SocketRef, Data(data): Data<Value>) {
    info!("Socket.IO connected: {:?} {:?}", socket.ns(), socket.id);
    socket.emit("auth", data).ok();

    let mut req_parts = socket.req_parts().clone();
    let session: ReadOnlySession<SessionNullPool> =
        ReadOnlySession::from_request_parts(&mut req_parts, &())
            .await
            .unwrap();
    let value = session.get::<String>("key");
    info!("Session value: {:?}", value);

    socket.on(
        "message",
        |socket: SocketRef, Data::<Value>(data), Bin(bin)| async move {
            info!("Received event: {:?} {:?}", data, bin);

            let mut data = data.clone();
            data["session_value"] = value.into();

            socket.bin(bin).emit("message-back", data).ok();
        },
    );
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing::subscriber::set_global_default(FmtSubscriber::default())?;

    let (layer, io) = SocketIo::new_layer();

    io.ns("/", on_connect);
    io.ns("/custom", on_connect);

    let session_store = SessionStore::<SessionNullPool>::new(None, SessionConfig::new())
        .await
        .unwrap();

    let app = axum::Router::new()
        .route("/", get(get_handler))
        .layer(layer)
        .layer(SessionLayer::new(session_store));

    info!("Starting server");

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();

    Ok(())
}
