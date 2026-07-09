# [`Socketioxide-Postgres`](https://github.com/totodore/socketioxide) 🚀🦀

A [***`socket.io`***](https://socket.io) adapter for [***`Socketioxide`***](https://github.com/totodore/socketioxide), using [PostgreSQL LISTEN/NOTIFY](https://www.postgresql.org/docs/current/sql-notify.html) for event broadcasting. This adapter enables **horizontal scaling** of your Socketioxide servers across distributed deployments by leveraging PostgreSQL as a message bus.

[![Crates.io](https://img.shields.io/crates/v/socketioxide-postgres.svg)](https://crates.io/crates/socketioxide-postgres)
[![Documentation](https://docs.rs/socketioxide-postgres/badge.svg)](https://docs.rs/socketioxide-postgres)
[![CI](https://github.com/Totodore/socketioxide/actions/workflows/github-ci.yml/badge.svg)](https://github.com/Totodore/socketioxide/actions/workflows/github-ci.yml)

<img src="https://raw.githubusercontent.com/andreasbm/readme/master/assets/lines/solar.png">

## Features

- **PostgreSQL LISTEN/NOTIFY-based adapter**
- **Support for any PostgreSQL client** via the [`Driver`] abstraction
- Built-in driver for the [sqlx](https://docs.rs/sqlx) crate: [`SqlxDriver`](https://docs.rs/socketioxide-postgres/latest/socketioxide_postgres/drivers/sqlx/struct.SqlxDriver.html)
- **Heartbeat-based liveness detection** for tracking active server nodes
- Fully compatible with the asynchronous Rust ecosystem
- Implement your own custom driver by implementing the `Driver` trait

> [!WARNING]
> This adapter is **not compatible** with [`@socket.io/postgres-adapter`](https://github.com/socketio/socket.io-postgres-adapter).
> These projects use entirely different protocols and cannot interoperate.
> **Do not mix Socket.IO JavaScript servers with Socketioxide Rust servers**.

<img src="https://raw.githubusercontent.com/andreasbm/readme/master/assets/lines/solar.png">

## Example: Using the PostgreSQL Adapter with Axum

```rust
use serde::{Deserialize, Serialize};
use socketioxide::{
    adapter::Adapter,
    extract::{Data, Extension, SocketRef},
    SocketIo,
};
use socketioxide_postgres::{
    drivers::sqlx::sqlx_client::{self as sqlx, PgPool},
    SqlxAdapter, PostgresAdapterCtr, PostgresAdapterConfig,
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
    Message {
        username: Username,
        message: String,
    },
    Username {
        username: Username,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::new();

    tracing::subscriber::set_global_default(subscriber)?;

    info!("Starting server");

    let pool = PgPool::connect("postgres://user:password@localhost/socketio").await?;
    let adapter = PostgresAdapterCtr::new_with_sqlx(pool);

    let (layer, io) = SocketIo::builder()
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
    socket.on("typing", on_typing);
    socket.on("stop typing", on_stop_typing);
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

```

<img src="https://raw.githubusercontent.com/andreasbm/readme/master/assets/lines/solar.png">

## Contributions and Feedback / Questions

Contributions are very welcome! Feel free to open an issue or a PR. If you're unsure where to start, check the [issues](https://github.com/totodore/socketioxide/issues).

For feedback or questions, join the discussion on the [discussions](https://github.com/totodore/socketioxide/discussions) page.

## License 🔐

This project is licensed under the [MIT license](./LICENSE).
