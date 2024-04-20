### Engineioxide does the heavy lifting for [socketioxide](https://docs.rs/socketioxide/latest/socketioxide/), a socket.io server implementation in Rust which integrates with the [tower](https://docs.rs/tower/latest/tower/) stack.

You can still use engineioxide as a standalone crate to talk with an engine.io client.

### Supported Protocols
You can enable support for other engine.io protocol implementations through feature flags. 
The latest protocol version (v4) is enabled by default. 

To add support for the `v3` protocol version, adjust your dependency configuration accordingly:

```toml
[dependencies]
# Enables the `v3` protocol (`v4` is always enabled, as it's the default).
engineioxide = { version = "0.3.0", features = ["v3"] }
```

## Feature flags : 
* `v3`: Enable the engine.io v3 protocol
* `tracing`: Enable tracing logs with the `tracing` crate

## Basic example with axum :
```rust
use bytes::Bytes;
use engineioxide::layer::EngineIoLayer;
use engineioxide::handler::EngineIoHandler;
use engineioxide::{Socket, DisconnectReason, Str};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use axum::routing::get;
// Global state, with axum it must be clonable
#[derive(Debug, Default, Clone)]
struct MyHandler {
    user_cnt: Arc<AtomicUsize>,
}

// Socket state
#[derive(Debug, Default)]
struct SocketState {
    id: Mutex<String>,
}

impl EngineIoHandler for MyHandler {
    type Data = SocketState;

    fn on_connect(&self, socket: Arc<Socket<SocketState>>) { 
        let cnt = self.user_cnt.fetch_add(1, Ordering::Relaxed) + 1;
        socket.emit(cnt.to_string()).ok();
    }
    fn on_disconnect(&self, socket: Arc<Socket<SocketState>>, reason: DisconnectReason) { 
        let cnt = self.user_cnt.fetch_sub(1, Ordering::Relaxed) - 1;
        socket.emit(cnt.to_string()).ok();
    }
    fn on_message(&self, msg: Str, socket: Arc<Socket<SocketState>>) { 
        *socket.data.id.lock().unwrap() = msg.into(); // bind a provided user id to a socket
    }
    fn on_binary(&self, data: Bytes, socket: Arc<Socket<SocketState>>) { }
}

// Create a new engineio layer
let layer = EngineIoLayer::new(MyHandler::default());

let app = axum::Router::<()>::new()
    .route("/", get(|| async { "Hello, World!" }))
    .layer(layer);

// Spawn the axum server

```
