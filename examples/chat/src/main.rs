use std::sync::atomic::AtomicUsize;

use serde::{Deserialize, Serialize};
use socketioxide::{
    extract::{Data, SocketRef, State},
    SocketIo,
};
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

struct UserCnt(AtomicUsize);
impl UserCnt {
    fn new() -> Self {
        Self(AtomicUsize::new(0))
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
        s.on("new message", |s: SocketRef, Data::<String>(msg)| {
            let username = s.extensions.get::<Username>().unwrap().clone();
            let msg = Res::Message {
                username,
                message: msg,
            };
            s.broadcast().emit("new message", msg).ok();
        });

        s.on(
            "add user",
            |s: SocketRef, Data::<String>(username), user_cnt: State<UserCnt>| {
                if s.extensions.get::<Username>().is_some() {
                    return;
                }
                let num_users = user_cnt.add_user();
                s.extensions.insert(Username(username.clone()));
                s.emit("login", Res::Login { num_users }).ok();

                let res = Res::UserEvent {
                    num_users,
                    username: Username(username),
                };
                s.broadcast().emit("user joined", res).ok();
            },
        );

        s.on("typing", |s: SocketRef| {
            let username = s.extensions.get::<Username>().unwrap().clone();
            s.broadcast()
                .emit("typing", Res::Username { username })
                .ok();
        });

        s.on("stop typing", |s: SocketRef| {
            let username = s.extensions.get::<Username>().unwrap().clone();
            s.broadcast()
                .emit("stop typing", Res::Username { username })
                .ok();
        });

        s.on_disconnect(|s: SocketRef, user_cnt: State<UserCnt>| {
            if let Some(username) = s.extensions.get::<Username>() {
                let num_users = user_cnt.remove_user();
                let res = Res::UserEvent {
                    num_users,
                    username: username.clone(),
                };
                s.broadcast().emit("user left", res).ok();
            }
        });
    });

    let app = axum::Router::new()
        .nest_service("/", ServeDir::new("dist"))
        .layer(
            ServiceBuilder::new()
                .layer(CorsLayer::permissive()) // Enable CORS policy
                .layer(layer),
        );

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();

    Ok(())
}
