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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::new();

    tracing::subscriber::set_global_default(subscriber)?;

    info!("Starting server");

    let (layer, io) = SocketIo::builder().with_state(UserCnt::new()).build_layer();

    io.ns("/", |s: SocketRef| {
        s.on(
            "new message",
            async |s: SocketRef, Data::<String>(msg), Extension::<Username>(username)| {
                let msg = &Res::Message {
                    username,
                    message: msg,
                };
                s.broadcast().emit("new message", msg).await.ok();
            },
        );

        s.on(
            "add user",
            async |s: SocketRef, Data::<String>(username), user_cnt: State<UserCnt>| {
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
            },
        );

        s.on(
            "typing",
            async |s: SocketRef, Extension::<Username>(username)| {
                s.broadcast()
                    .emit("typing", &Res::Username { username })
                    .await
                    .ok();
            },
        );

        s.on(
            "stop typing",
            async |s: SocketRef, Extension::<Username>(username)| {
                s.broadcast()
                    .emit("stop typing", &Res::Username { username })
                    .await
                    .ok();
            },
        );

        s.on_disconnect(
            async |s: SocketRef, user_cnt: State<UserCnt>, Extension::<Username>(username)| {
                let num_users = user_cnt.remove_user();
                let res = &Res::UserEvent {
                    num_users,
                    username,
                };
                s.broadcast().emit("user left", res).await.ok();
            },
        );
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
