use serde::{Deserialize, Serialize};
use socketioxide::{
    adapter::Adapter,
    extract::{Data, Extension, SocketRef, State},
    SocketIo,
};
use socketioxide_postgres::{drivers::sqlx::sqlx_client as sqlx, PostgresAdapterCtr, SqlxAdapter};
use sqlx::PgPool;
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
struct RemoteUserCnt(PgPool);
impl RemoteUserCnt {
    fn new(pool: PgPool) -> Self {
        Self(pool)
    }

    async fn init(&self) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS socket_io_chat_users (
                id INT PRIMARY KEY,
                count BIGINT NOT NULL DEFAULT 0
            )"#,
        )
        .execute(&self.0)
        .await?;

        sqlx::query(
            "INSERT INTO socket_io_chat_users (id, count) VALUES (1, 0) ON CONFLICT DO NOTHING",
        )
        .execute(&self.0)
        .await?;

        Ok(())
    }

    async fn add_user(&self) -> Result<usize, sqlx::Error> {
        let count: i64 = sqlx::query_scalar(
            "UPDATE socket_io_chat_users SET count = count + 1 WHERE id = 1 RETURNING count",
        )
        .fetch_one(&self.0)
        .await?;
        Ok(count as usize)
    }

    async fn remove_user(&self) -> Result<usize, sqlx::Error> {
        let count: i64 = sqlx::query_scalar(
            "UPDATE socket_io_chat_users SET count = count - 1 WHERE id = 1 RETURNING count",
        )
        .fetch_one(&self.0)
        .await?;
        Ok(count as usize)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::new();

    tracing::subscriber::set_global_default(subscriber)?;

    info!("Starting server");

    let pool = sqlx::Pool::connect("postgres://socketio:socketio@localhost/socketio").await?;
    let user_cnt = RemoteUserCnt::new(pool.clone());
    user_cnt.init().await?;
    let adapter = PostgresAdapterCtr::new_with_sqlx(pool);

    let (layer, io) = SocketIo::builder()
        .with_state(user_cnt)
        .with_adapter::<SqlxAdapter<_>>(adapter)
        .build_layer();

    io.ns("/", on_connect).await?;

    let app = axum::Router::new()
        .fallback_service(ServeDir::new("dist"))
        .layer(
            ServiceBuilder::new()
                .layer(CorsLayer::permissive()) // Enable CORS policy
                .layer(layer),
        );

    let port = std::env::var("PORT")
        .map(|s| s.parse().unwrap())
        .unwrap_or(3000);

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port))
        .await
        .unwrap();
    axum::serve(listener, app).await.unwrap();

    Ok(())
}

async fn on_connect<A: Adapter>(socket: SocketRef<A>) {
    socket.on("new message", on_msg);
    socket.on("add user", on_add_user);
    socket.on("typing", on_typing);
    socket.on("stop typing", on_stop_typing);
    socket.on_disconnect(on_disconnect);
}
async fn on_msg<A: Adapter>(
    s: SocketRef<A>,
    Data(msg): Data<String>,
    Extension(username): Extension<Username>,
) {
    let msg = &Res::Message {
        username,
        message: msg,
    };
    s.broadcast().emit("new message", msg).await.ok();
}
async fn on_add_user<A: Adapter>(
    s: SocketRef<A>,
    Data(username): Data<String>,
    user_cnt: State<RemoteUserCnt>,
) {
    if s.extensions.get::<Username>().is_some() {
        return;
    }
    let num_users = user_cnt.add_user().await.unwrap_or(0);
    s.extensions.insert(Username(username.clone()));
    s.emit("login", &Res::Login { num_users }).ok();

    let res = &Res::UserEvent {
        num_users,
        username: Username(username),
    };
    s.broadcast().emit("user joined", res).await.ok();
}
async fn on_typing<A: Adapter>(s: SocketRef<A>, Extension(username): Extension<Username>) {
    s.broadcast()
        .emit("typing", &Res::Username { username })
        .await
        .ok();
}
async fn on_stop_typing<A: Adapter>(s: SocketRef<A>, Extension(username): Extension<Username>) {
    s.broadcast()
        .emit("stop typing", &Res::Username { username })
        .await
        .ok();
}
async fn on_disconnect<A: Adapter>(
    s: SocketRef<A>,
    user_cnt: State<RemoteUserCnt>,
    Extension(username): Extension<Username>,
) {
    let num_users = user_cnt.remove_user().await.unwrap_or(0);
    let res = &Res::UserEvent {
        num_users,
        username,
    };
    s.broadcast().emit("user left", res).await.ok();
}
