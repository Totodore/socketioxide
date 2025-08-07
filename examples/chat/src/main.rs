use std::sync::atomic::AtomicUsize;

use serde::{Deserialize, Serialize};
use socketioxide::{
    extract::{Data, Extension, SocketRef, State},
    SocketIo,
};
use std::sync::Arc;
use tower::ServiceBuilder;
use tower_http::{cors::CorsLayer, services::ServeDir};
use tracing::info;
use tracing_subscriber::FmtSubscriber;

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(transparent)]
struct Username(String);

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase", untagged)]
enum Res {
    Login {
        #[serde(rename = "numUsers")]
        num_users: usize,
    },
    UserEvent {
        #[serde(rename = "numUsers")]
        num_users: usize,
        username: Username,
    },
    Message {
        username: Username,
        message: String,
    },
    Username {
        username: Username,
    },
}
#[derive(Clone)]
struct UserCnt(Arc<AtomicUsize>);
impl UserCnt {
    fn new() -> Self {
        Self(Arc::new(AtomicUsize::new(0)))
    }
    fn add_user(&self) -> usize {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1
    }
    fn remove_user(&self) -> usize {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::SeqCst) - 1
    }
}

async fn on_new_msg(
    s: SocketRef,
    Data(msg): Data<String>,
    Extension(username): Extension<Username>,
) {
    let msg = &Res::Message {
        username,
        message: msg,
    };
    s.broadcast().emit("new message", msg).await.ok();
}
async fn on_add_user(s: SocketRef, Data(username): Data<String>, user_cnt: State<UserCnt>) {
    if s.extensions.get::<Username>().is_some() {
        return;
    }
    let num_users = user_cnt.add_user();
    s.extensions.insert(Username(username.clone()));
    s.emit("login", &Res::Login { num_users }).ok();

    let res = &Res::UserEvent {
        num_users,
        username: Username(username),
    };
    s.broadcast().emit("user joined", res).await.ok();
}
async fn on_typing(s: SocketRef, Extension(username): Extension<Username>) {
    s.broadcast()
        .emit("typing", &Res::Username { username })
        .await
        .ok();
}
async fn stop_typing(s: SocketRef, Extension(username): Extension<Username>) {
    s.broadcast()
        .emit("stop typing", &Res::Username { username })
        .await
        .ok();
}
async fn on_disconnect(
    s: SocketRef,
    user_cnt: State<UserCnt>,
    Extension(username): Extension<Username>,
) {
    let num_users = user_cnt.remove_user();
    let res = &Res::UserEvent {
        num_users,
        username,
    };
    s.broadcast().emit("user left", res).await.ok();
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::new();

    tracing::subscriber::set_global_default(subscriber)?;

    info!("Starting server");

    let (layer, io) = SocketIo::builder().with_state(UserCnt::new()).build_layer();

    io.ns("/", async |s: SocketRef| {
        s.on("new message", on_new_msg);
        s.on("add user", on_add_user);
        s.on("typing", on_typing);
        s.on("stop typing", stop_typing);
        s.on_disconnect(on_disconnect);
    });

    let app = axum::Router::new()
        .fallback_service(ServeDir::new("dist"))
        .layer(
            ServiceBuilder::new()
                .layer(CorsLayer::permissive()) // Enable CORS policy
                .layer(layer),
        );

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();

    Ok(())
}
