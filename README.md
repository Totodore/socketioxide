<img src="https://socket.io/images/logo-dark.svg" align="right" width=200 />

# [`Socketioxide`](https://github.com/totodore/socketioxide) 🚀🦀

A [***`socket.io`***](https://socket.io) server implementation in Rust that integrates with the [***`Tower`***](https://tokio.rs/#tk-lib-tower) ecosystem and the [***`Tokio stack`***](https://tokio.rs). Integrates with any server framework based on tower like [***`Axum`***](https://docs.rs/axum/latest/axum/), [***`Warp`***](https://docs.rs/warp/latest/warp/) or [***`Hyper`***](https://docs.rs/hyper/latest/hyper/). Add any other tower based middleware on top of socketioxide such as CORS, SSL, compression, etc with [***`tower-http`***](https://docs.rs/tower-http/latest/tower_http/).

> ⚠️ This crate is under active development and the API is not yet stable.



[![Crates.io](https://img.shields.io/crates/v/socketioxide.svg)](https://crates.io/crates/socketioxide)
[![Documentation](https://docs.rs/socketioxide/badge.svg)](https://docs.rs/socketioxide)
[![SocketIO CI](https://github.com/Totodore/socketioxide/actions/workflows/socketio-ci.yml/badge.svg)](https://github.com/Totodore/socketioxide/actions/workflows/socketio-ci.yml)
[![EngineIO CI](https://github.com/Totodore/socketioxide/actions/workflows/engineio-ci.yml/badge.svg)](https://github.com/Totodore/socketioxide/actions/workflows/engineio-ci.yml)

<img src="https://raw.githubusercontent.com/andreasbm/readme/master/assets/lines/solar.png">

## Features :
* Integrates with :
  * [Axum](https://docs.rs/axum/latest/axum/): [🏓echo example](./examples/src/socketio-echo/axum_echo.rs)
  * [Warp](https://docs.rs/warp/latest/warp/): [🏓echo example](./examples/src/socketio-echo/warp_echo.rs)
  * [Hyper](https://docs.rs/hyper/latest/hyper/): [🏓echo example](./examples/src/socketio-echo/hyper_echo.rs)
* Out of the box support for any other middleware based on tower :
  * [🔓CORS](https://docs.rs/tower-http/latest/tower_http/struct.CorsLayer.html)
  * [🔐SSL](https://docs.rs/tower-http/latest/tower_http/struct.SslLayer.html)
  * [📁Compression](https://docs.rs/tower-http/latest/tower_http/struct.CompressionLayer.html)
  * [🕐Timeout](https://docs.rs/tower-http/latest/tower_http/struct.TimeoutLayer.html)
* Namespaces
* Rooms
* Ack and emit with ack
* Binary packets
* Polling & Websocket transports
* Extensions to add custom data to sockets
* Memory efficient http payload parsing with streams

<img src="https://raw.githubusercontent.com/andreasbm/readme/master/assets/lines/solar.png">

## Planned features :
* Other adapter to share state between server instances (like redis adapter), currently only the in memory adapter is implemented
* Better API ergonomics
* State recovery when a socket reconnects
* SocketIo v3 support (currently only v4 is supported)

<img src="https://raw.githubusercontent.com/andreasbm/readme/master/assets/lines/solar.png">

## Examples :
<details> <summary><code>Chat app 💬</code></summary>

```rust

#[derive(Deserialize, Clone, Debug)]
struct Nickname(String);

#[derive(Deserialize)]
struct Auth {
    pub nickname: Nickname,
}

pub async fn handler(socket: Arc<Socket<LocalAdapter>>) {
    info!("Socket connected on / with id: {}", socket.sid);
    if let Ok(data) = socket.handshake.data::<Auth>() {
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
        info!("Socket disconnected: {} {}", socket.sid, reason);
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
use serde::{Serialize, Deserialize};
use socketioxide::{Namespace, SocketIoLayer};
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize)]
struct MyData {
  pub name: String,
  pub age: u8,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {

    println!("Starting server");

    let ns = Namespace::builder()
        .add("/", |socket| async move {
            println!("Socket connected on / namespace with id: {}", socket.sid);

            // Add a callback triggered when the socket receives an 'abc' event
            // The json data will be deserialized to MyData
            socket.on("abc", |socket, data: MyData, bin, _| async move {
                println!("Received abc event: {:?} {:?}", data, bin);
                socket.bin(bin).emit("abc", data).ok();
            });

            // Add a callback triggered when the socket receives an 'acb' event
            // Ackknowledge the message with the ack callback
            socket.on("acb", |_, data: Value, bin, ack| async move {
                println!("Received acb event: {:?} {:?}", data, bin);
                ack.bin(bin).send(data).ok();
            });
            // Add a callback triggered when the socket disconnects
            // The reason of the disconnection will be passed to the callback
            socket.on_disconnect(|socket, reason| async move {
                println!("Socket.IO disconnected: {} {}", socket.sid, reason);
            });
        })
        .add("/custom", |socket| async move {
            println!("Socket connected on /custom namespace with id: {}", socket.sid);
        })
        .build();

    let app = axum::Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .layer(SocketIoLayer::new(ns));

    Server::bind(&"127.0.0.1:3000".parse().unwrap())
        .serve(app.into_make_service())
        .await?;

    Ok(())
}
```
</details>
