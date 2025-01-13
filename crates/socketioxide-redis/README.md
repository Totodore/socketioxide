# [`Socketioxide-Redis`](https://github.com/totodore/socketioxide) 🚀🦀

A [***`socket.io`***](https://socket.io) Redis adapter for [***`Socketioxide`***](https://github.com/totodore/socketioxide), enabling effortless horizontal scaling through Redis. This adapter supports multiple Redis client implementations and handles various Redis topologies, ensuring seamless scalability even on sharded, clustered setups.

> ⚠️ This crate is under active development, and the API is not yet stable.

[![Crates.io](https://img.shields.io/crates/v/socketioxide-redis.svg)](https://crates.io/crates/socketioxide-redis)
[![Documentation](https://docs.rs/socketioxide-redis/badge.svg)](https://docs.rs/socketioxide-redis)
[![CI](https://github.com/Totodore/socketioxide-redis/actions/workflows/github-ci.yml/badge.svg)](https://github.com/Totodore/socketioxide-redis/actions/workflows/github-ci.yml)

<img src="https://raw.githubusercontent.com/andreasbm/readme/master/assets/lines/solar.png">

## Features

- **Multiple Redis client support with the driver abstraction**:
  - [redis](https://docs.rs/redis/latest/redis/) crate
  - [fred](https://docs.rs/fred/latest/fred/) crate
  - Your custom Redis client implementation!
- **Flexible Redis topology support**:
  - Standalone
  - Sentinel
  - Clustered setups
- **Sharded Pub/Sub** for enhanced scalability in clustered Redis topologies.
- **Seamless integration with Socketioxide** for distributed event handling.
- **High performance** with minimal overhead (~1ms for event propagation on a local cluster).

<img src="https://raw.githubusercontent.com/andreasbm/readme/master/assets/lines/solar.png">

## Example: Multi-Node chat application

Here’s an example of how to use the Redis adapter with Socketioxide in an Axum-based chat application:

```rust
use serde::{Deserialize, Serialize};
use socketioxide::{
    adapter::Adapter,
    extract::{Data, Extension, SocketRef, State},
    SocketIo,
};
use socketioxide_redis::{drivers::redis::redis_client as redis, RedisAdapter, RedisAdapterCtr};
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
struct RemoteUserCnt(redis::aio::MultiplexedConnection);
impl RemoteUserCnt {
    fn new(conn: redis::aio::MultiplexedConnection) -> Self {
        Self(conn)
    }
    async fn add_user(&self) -> Result<usize, redis::RedisError> {
        let mut conn = self.0.clone();
        let num_users: usize = redis::cmd("INCR")
            .arg("num_users")
            .query_async(&mut conn)
            .await?;
        Ok(num_users)
    }
    async fn remove_user(&self) -> Result<usize, redis::RedisError> {
        let mut conn = self.0.clone();
        let num_users: usize = redis::cmd("DECR")
            .arg("num_users")
            .query_async(&mut conn)
            .await?;
        Ok(num_users)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::new();

    tracing::subscriber::set_global_default(subscriber)?;

    info!("Starting server");

    let client = redis::Client::open("redis://127.0.0.1:6379?protocol=resp3")?;
    let adapter = RedisAdapterCtr::new_with_redis(&client).await?;
    let conn = client.get_multiplexed_tokio_connection().await?;

    let (layer, io) = SocketIo::builder()
        .with_state(RemoteUserCnt::new(conn))
        .with_adapter::<RedisAdapter<_>>(adapter)
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

```

<img src="https://raw.githubusercontent.com/andreasbm/readme/master/assets/lines/solar.png">

## Contributions and Feedback / Questions

We welcome contributions! Feel free to open an issue or a PR. If you’re unsure where to start, check the [issues](https://github.com/totodore/socketioxide/issues).

For feedback or questions, join the discussion on the [discussions](https://github.com/totodore/socketioxide/discussions) page.

## License 🔐

This project is licensed under the [MIT license](./LICENSE).
