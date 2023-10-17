# [`Socketioxide`](https://github.com/totodore/socketioxide) 🚀🦀

A [***`socket.io`***](https://socket.io) server implementation in Rust that integrates with the [***`Tower`***](https://tokio.rs/#tk-lib-tower) ecosystem and the [***`Tokio stack`***](https://tokio.rs). It integrates with any server framework based on tower like [***`Axum`***](https://docs.rs/axum/latest/axum/), [***`Warp`***](https://docs.rs/warp/latest/warp/) or [***`Hyper`***](https://docs.rs/hyper/latest/hyper/). Add any other tower based middleware on top of socketioxide such as CORS, authorization, compression, etc with [***`tower-http`***](https://docs.rs/tower-http/latest/tower_http/).

> ⚠️ This crate is under active development and the API is not yet stable.



[![Crates.io](https://img.shields.io/crates/v/socketioxide.svg)](https://crates.io/crates/socketioxide)
[![Documentation](https://docs.rs/socketioxide/badge.svg)](https://docs.rs/socketioxide)
[![CI](https://github.com/Totodore/socketioxide/actions/workflows/github-ci.yml/badge.svg)](https://github.com/Totodore/socketioxide/actions/workflows/github-ci.yml)

<img src="https://raw.githubusercontent.com/andreasbm/readme/master/assets/lines/solar.png">

## Features :
* Integrates with :
  * [Axum](https://docs.rs/axum/latest/axum/): [🏓echo example](./examples/src/socketio-echo/axum_echo.rs)
  * [Warp](https://docs.rs/warp/latest/warp/): [🏓echo example](./examples/src/socketio-echo/warp_echo.rs)
  * [Hyper](https://docs.rs/hyper/latest/hyper/): [🏓echo example](./examples/src/socketio-echo/hyper_echo.rs)
* Out of the box support for any other middleware based on tower :
  * [🔓CORS](https://docs.rs/tower-http/latest/tower_http/cors)
  * [📁Compression](https://docs.rs/tower-http/latest/tower_http/compression)
  * [🔐Authorization](https://docs.rs/tower-http/latest/tower_http/auth)
* Namespaces
* Rooms
* Ack and emit with ack
* Binary packets
* Polling & Websocket transports
* Extensions to add custom data to sockets
* Memory efficient http payload parsing with streams
* Api that mimics the [socket.io](https://socket.io/docs/v4/server-api/) javascript api as much as possible
* Well tested with the official [end to end test-suite](https://github.com/totodore/socketioxide/actions) 
* Socket.io versions supported :
  * [🔌protocol v5](https://socket.io/docs/v4/) : based on engine.io v4, feature flag `v5` (default)
  * [🔌protocol v4](https://github.com/socketio/socket.io-protocol/tree/v4) : based on engine.io v3, feature flag `v4`

<img src="https://raw.githubusercontent.com/andreasbm/readme/master/assets/lines/solar.png">

## Planned features :
* Other adapter to share state between server instances (like redis adapter), currently only the in memory adapter is implemented
* State recovery when a socket reconnects

<img src="https://raw.githubusercontent.com/andreasbm/readme/master/assets/lines/solar.png">

## Examples :
<details> <summary><code>Chat app 💬 (see full example <a href="./examples/src/chat">here</a>)</code></summary>

```rust
use std::sync::Arc;

use serde::Deserialize;
use socketioxide::{adapter::LocalAdapter, Socket};
use tracing::info;

#[derive(Deserialize, Clone, Debug)]
pub struct Nickname(String);

#[derive(Deserialize)]
pub struct Auth {
    pub nickname: Nickname,
}

pub async fn handler(socket: Arc<Socket<LocalAdapter>>, data: Option<Auth>) {
    info!("Socket connected on / with id: {}", socket.id);
    if let Some(data) = data {
        info!("Nickname: {:?}", data.nickname);
        socket.extensions.insert(data.nickname);
        socket.emit("message", "Welcome to the chat!").ok();
        socket.join("default").unwrap();
    } else {
        info!("No nickname provided, disconnecting...");
        socket.disconnect().ok();
        return;
    }

    socket.on(
        "message",
        |socket, (room, message): (String, String), _, _| async move {
            let Nickname(ref nickname) = *socket.extensions.get().unwrap();
            info!("transfering message from {nickname} to {room}: {message}");
            info!("Sockets in room: {:?}", socket.local().sockets().unwrap());
            if let Some(dest) = socket.to("default").sockets().unwrap().iter().find(|s| {
                s.extensions
                    .get::<Nickname>()
                    .map(|n| n.0 == room)
                    .unwrap_or_default()
            }) {
                info!("Sending message to {}", room);
                dest.emit("message", format!("{}: {}", nickname, message))
                    .ok();
            }

            socket
                .to(room)
                .emit("message", format!("{}: {}", nickname, message))
                .ok();
        },
    );

    socket.on("join", |socket, room: String, _, _| async move {
        info!("Joining room {}", room);
        socket.join(room).unwrap();
    });

    socket.on("leave", |socket, room: String, _, _| async move {
        info!("Leaving room {}", room);
        socket.leave(room).unwrap();
    });

    socket.on("list", |socket, room: Option<String>, _, _| async move {
        if let Some(room) = room {
            info!("Listing sockets in room {}", room);
            let sockets = socket
                .within(room)
                .sockets()
                .unwrap()
                .iter()
                .filter_map(|s| s.extensions.get::<Nickname>())
                .fold("".to_string(), |a, b| a + &b.0 + ", ")
                .trim_end_matches(", ")
                .to_string();
            socket.emit("message", sockets).ok();
        } else {
            let rooms = socket.rooms().unwrap();
            info!("Listing rooms: {:?}", &rooms);
            socket.emit("message", rooms).ok();
        }
    });

    socket.on("nickname", |socket, nickname: Nickname, _, _| async move {
        let previous = socket.extensions.insert(nickname.clone());
        info!("Nickname changed from {:?} to {:?}", &previous, &nickname);
        let msg = format!(
            "{} changed his nickname to {}",
            previous.map(|n| n.0).unwrap_or_default(),
            nickname.0
        );
        socket.to("default").emit("message", msg).ok();
    });

    socket.on_disconnect(|socket, reason| async move {
        info!("Socket disconnected: {} {}", socket.id, reason);
        let Nickname(ref nickname) = *socket.extensions.get().unwrap();
        let msg = format!("{} left the chat", nickname);
        socket.to("default").emit("message", msg).ok();
    });
}

```

</details>
<details> <summary><code>Echo implementation with Axum 🏓</code></summary>

```rust
use axum::routing::get;
use axum::Server;
use serde_json::Value;
use socketioxide::SocketIo;
use tracing::info;
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing::subscriber::set_global_default(FmtSubscriber::default())?;

    let (layer, io) = SocketIo::new_layer();
    io.ns("/", |socket, auth: Value| async move {
        info!("Socket.IO connected: {:?} {:?}", socket.ns(), socket.sid);
        socket.emit("auth", auth).ok();

        socket.on("message", |socket, data: Value, bin, _| async move {
            info!("Received event: {:?} {:?}", data, bin);
            socket.bin(bin).emit("message-back", data).ok();
        });

        socket.on("message-with-ack", |_, data: Value, bin, ack| async move {
            info!("Received event: {:?} {:?}", data, bin);
            ack.bin(bin).send(data).ok();
        });

        socket.on_disconnect(|socket, reason| async move {
            info!("Socket.IO disconnected: {} {}", socket.sid, reason);
        });
    });

    io.ns("/custom", |socket, auth: Value| async move {
        info!("Socket.IO connected on: {:?} {:?}", socket.ns(), socket.sid);
        socket.emit("auth", auth).ok();
    });

    let app = axum::Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .layer(layer);

    info!("Starting server");

    Server::bind(&"127.0.0.1:3000".parse().unwrap())
        .serve(app.into_make_service())
        .await?;

    Ok(())
}
```
</details>

<img src="https://raw.githubusercontent.com/andreasbm/readme/master/assets/lines/solar.png">

## Contributions and Feedback / Questions :
Any contribution is welcome, feel free to open an issue or a PR. If you want to contribute but don't know where to start, you can check the [issues](https://github.com/totodore/socketioxide/issues).

If you have any question or feedback, please open a thread on the [discussions](https://github.com/totodore/socketioxide/discussions) page.

## License 🔐
This project is licensed under the [MIT license](./LICENSE).
