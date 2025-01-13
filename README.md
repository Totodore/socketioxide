# [`Socketioxide`](https://github.com/totodore/socketioxide) 🚀🦀

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://github.com/Totodore/socketioxide/blob/main/.github/logo_dark.svg">
  <img width=150 height=150 align=left alt="Shows Anuraghazra's GitHub Stats." src="https://github.com/Totodore/socketioxide/blob/main/.github/logo_light.svg">
</picture>

A [***`socket.io`***](https://socket.io) server implementation in Rust that integrates with the [***`Tower`***](https://tokio.rs/#tk-lib-tower) ecosystem and the [***`Tokio stack`***](https://tokio.rs). It integrates with any server framework based on tower like [***`Axum`***](https://docs.rs/axum/latest/axum/), [***`Warp`***](https://docs.rs/warp/latest/warp/), [***`Salvo`***](https://salvo.rs), [***`Viz`***](https://viz.rs) or [***`Hyper`***](https://docs.rs/hyper/latest/hyper/). Add any other tower based middleware on top of socketioxide such as CORS, authorization, compression, etc with [***`tower-http`***](https://docs.rs/tower-http/latest/tower_http/).

 ⚠️ This crate is under active development and the API is not yet stable.

<img src="https://raw.githubusercontent.com/andreasbm/readme/master/assets/lines/solar.png">

[![Crates.io](https://img.shields.io/crates/v/socketioxide.svg)](https://crates.io/crates/socketioxide)
[![Documentation](https://docs.rs/socketioxide/badge.svg)](https://docs.rs/socketioxide)
[![CI](https://github.com/Totodore/socketioxide/actions/workflows/github-ci.yml/badge.svg)](https://github.com/Totodore/socketioxide/actions/workflows/github-ci.yml)

## Features
* Integrates with:
  * [Axum](https://docs.rs/axum/latest/axum/): [🏓echo example](./examples/axum-echo/axum_echo.rs)
  * [Warp](https://docs.rs/warp/latest/warp/): [🏓echo example](https://github.com/Totodore/socketioxide/blob/v0.8.0/examples/warp-echo/warp_echo.rs) (Not supported with `socketioxide >= 0.9.0` as long as warp doesn't migrate to hyper v1)
  * [Hyper](https://docs.rs/hyper/latest/hyper/): [🏓echo example](./examples/hyper-echo/hyper_echo.rs)
  * [Salvo](https://salvo.rs): [🏓echo example](./examples/salvo-echo/salvo_echo.rs)
  * [Viz](https://viz.rs): [🏓echo example](./examples/viz-echo/viz_echo.rs)
* Out of the box support for any other middleware based on tower:
  * [🔓CORS](https://docs.rs/tower-http/latest/tower_http/cors)
  * [📁Compression](https://docs.rs/tower-http/latest/tower_http/compression)
  * [🔐Authorization](https://docs.rs/tower-http/latest/tower_http/auth)
* Effortless horizontal scaling with plugable adapters:
  * [Redis / Valkey](https://docs.rs/socketioxide-redis/latest/socketioxide-redis)
  * More to come...
* Namespaces and Dynamic Namespaces
* Rooms
* Ack and emit with ack
* Binary packets
* Polling & Websocket transports
* Common (default) & Msgpack parsers
* Extensions to add custom data to sockets
* Memory efficient http payload parsing with streams
* Flexible axum-like API to handle events. With extractors to extract data from your handlers
* Well tested with the official [end to end test-suite](https://github.com/totodore/socketioxide/actions)
* All Socket.io versions supported :
  * [🔌protocol v5](https://socket.io/docs/v4/) : socket.io js from v3.0.0..latest, it is enabled by default
  * [🔌protocol v4](https://github.com/socketio/socket.io-protocol/tree/v4) : based on engine.io v3, under the feature flag `v4`, (socket.io js from v1.0.3..latest)

<img src="https://raw.githubusercontent.com/andreasbm/readme/master/assets/lines/solar.png">

## Examples
<details> <summary><code>Chat app 💬 (see full example <a href="./examples/chat">here</a>)</code></summary>

```rust
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

```

</details>
<details> <summary><code>Echo implementation with Axum 🏓</code></summary>

```rust
use axum::routing::get;
use serde_json::Value;
use socketioxide::{
    extract::{AckSender, Bin, Data, SocketRef},
    SocketIo,
};
use tracing::info;
use tracing_subscriber::FmtSubscriber;

fn on_connect(socket: SocketRef, Data(data): Data<Value>) {
    info!("Socket.IO connected: {:?} {:?}", socket.ns(), socket.id);
    socket.emit("auth", data).ok();

    socket.on(
        "message",
        |socket: SocketRef, Data::<Value>(data), Bin(bin)| {
            info!("Received event: {:?} {:?}", data, bin);
            socket.bin(bin).emit("message-back", data).ok();
        },
    );

    socket.on(
        "message-with-ack",
        |Data::<Value>(data), ack: AckSender, Bin(bin)| {
            info!("Received event: {:?} {:?}", data, bin);
            ack.bin(bin).send(data).ok();
        },
    );
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing::subscriber::set_global_default(FmtSubscriber::default())?;

    let (layer, io) = SocketIo::new_layer();

    io.ns("/", on_connect);
    io.ns("/custom", on_connect);

    let app = axum::Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .layer(layer);

    info!("Starting server");

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();

    Ok(())
}
```
</details>
<code>Other examples are available in the <a href="./examples">example folder</a></code>

<img src="https://raw.githubusercontent.com/andreasbm/readme/master/assets/lines/solar.png">

## Contributions and Feedback / Questions
Any contribution is welcome, feel free to open an issue or a PR. If you want to contribute but don't know where to start, you can check the [issues](https://github.com/totodore/socketioxide/issues).

If you have any question or feedback, please open a thread on the [discussions](https://github.com/totodore/socketioxide/discussions) page.

## License 🔐
This project is licensed under the [MIT license](./LICENSE).
