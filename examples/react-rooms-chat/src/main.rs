mod state;

use axum::routing::get;
use socketioxide::{
    extract::{Data, SocketRef, State},
    SocketIo,
};
use tokio::net::TcpListener;
use tower::ServiceBuilder;
use tower_http::cors::CorsLayer;
use tracing::info;
use tracing_subscriber::FmtSubscriber;

#[derive(Debug, serde::Deserialize)]
struct MessageIn {
    room: String,
    text: String,
}

#[derive(serde::Serialize)]
struct Messages {
    messages: Vec<state::Message>,
}

async fn on_connect(socket: SocketRef) {
    info!("socket connected: {}", socket.id);

    socket.on(
        "join",
        |socket: SocketRef, Data::<String>(room), store: State<state::MessageStore>| async move {
            info!("Received join: {:?}", room);
            let _ = socket.leave_all();
            let _ = socket.join(room.clone());
            let messages = store.get(&room).await;
            let _ = socket.emit("messages", Messages { messages });
        },
    );

    socket.on(
        "message",
        |socket: SocketRef, Data::<MessageIn>(data), store: State<state::MessageStore>| async move {
            info!("Received message: {:?}", data);

            let response = state::Message {
                text: data.text,
                user: format!("anon-{}", socket.id),
                date: chrono::Utc::now(),
            };

            store.insert(&data.room, response.clone()).await;

            let _ = socket.within(data.room).emit("message", response);
        },
    )
}

async fn handler(axum::extract::State(io): axum::extract::State<SocketIo>) {
    info!("handler called");
    let _ = io.emit("hello", "world");
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing::subscriber::set_global_default(FmtSubscriber::default())?;

    let messages = state::MessageStore::default();

    let (layer, io) = SocketIo::builder().with_state(messages).build_layer();

    io.ns("/", on_connect);

    let app = axum::Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .route("/hello", get(handler))
        .with_state(io)
        .layer(
            ServiceBuilder::new()
                .layer(CorsLayer::permissive())
                .layer(layer),
        );

    info!("Starting server");

    let listener = TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();

    Ok(())
}
