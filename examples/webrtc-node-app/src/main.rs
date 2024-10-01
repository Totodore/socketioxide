use socketioxide::{
    extract::{Data, SocketRef},
    SocketIo,
};
use tower::ServiceBuilder;
use tower_http::{cors::CorsLayer, services::ServeDir};
use tracing::info;
use tracing_subscriber::FmtSubscriber;

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Event {
    room_id: String,
    sdp: rmpv::Value,
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct IceCandidate {
    room_id: String,
    #[serde(flatten)]
    data: rmpv::Value,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::new();

    tracing::subscriber::set_global_default(subscriber)?;

    info!("Starting server");

    let (layer, io) = SocketIo::new_layer();

    io.ns("/", |s: SocketRef| {
        s.on("join", |s: SocketRef, Data(room_id): Data<String>| {
            let socket_cnt = s.within(room_id.clone()).sockets().unwrap().len();
            if socket_cnt == 0 {
                tracing::info!("creating room {room_id} and emitting room_created socket event");
                s.join(room_id.clone()).unwrap();
                s.emit("room_created", &room_id).unwrap();
            } else if socket_cnt == 1 {
                tracing::info!("joining room {room_id} and emitting room_joined socket event");
                s.join(room_id.clone()).unwrap();
                s.emit("room_joined", &room_id).unwrap();
            } else {
                tracing::info!("Can't join room {room_id}, emitting full_room socket event");
                s.emit("full_room", &room_id).ok();
            }
        });

        s.on("start_call", |s: SocketRef, Data(room_id): Data<String>| {
            tracing::info!("broadcasting start_call event to peers in room {room_id}");
            s.to(room_id.clone()).emit("start_call", &room_id).ok();
        });
        s.on("webrtc_offer", |s: SocketRef, Data(event): Data<Event>| {
            tracing::info!(
                "broadcasting webrtc_offer event to peers in room {}",
                event.room_id
            );
            s.to(event.room_id).emit("webrtc_offer", &event.sdp).ok();
        });
        s.on("webrtc_answer", |s: SocketRef, Data(event): Data<Event>| {
            tracing::info!(
                "broadcasting webrtc_answer event to peers in room {}",
                event.room_id
            );
            s.to(event.room_id).emit("webrtc_answer", &event.sdp).ok();
        });
        s.on(
            "webrtc_ice_candidate",
            |s: SocketRef, Data(event): Data<IceCandidate>| {
                tracing::info!(
                    "broadcasting ice_candidate event to peers in room {}",
                    event.room_id
                );
                s.to(event.room_id.clone())
                    .emit("webrtc_ice_candidate", &event)
                    .ok();
            },
        );
    });

    let app = axum::Router::new()
        .nest_service("/", ServeDir::new("public"))
        .layer(
            ServiceBuilder::new()
                .layer(CorsLayer::permissive()) // Enable CORS policy
                .layer(layer),
        );

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();

    Ok(())
}
