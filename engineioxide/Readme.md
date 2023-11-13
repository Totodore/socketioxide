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
* `hyper-v1`: Hyper v1 compatibility layer. For the moment most of the http crates use hyper v0

## Basic example with axum :
```rust
use engineioxide::layer::EngineIoLayer;
use engineioxide::handler::EngineIoHandler;
use engineioxide::{Socket, DisconnectReason};
// Global state
#[derive(Debug, Clone)]
struct MyHandler {
    user_cnt: AtomicUsize;
}

// Socket state
#[derive(Debug, Clone)]
struct SocketState {
    id: Mutex<String>;
}

impl EngineIoHandler for MyHandler {
    type Data = SocketState;

    fn on_connect(&self, socket: Arc<Socket<SocketState>>) { 
        let cnt = self.user_cnt.fetch_add(1) + 1;
        socket.emit(cnt.to_string()).ok();
    }
    fn on_disconnect(&self, socket: Arc<Socket<SocketState>>, reason: DisconnectReason) { 
        let cnt = self.user_cnt.fetch_sub(1) - 1;
        socket.emit(cnt.to_string()).ok();
    }
    fn on_message(&self, msg: String, socket: Arc<Socket<SocketState>>) { 
        *socket.data.id.lock().unwrap() = msg; // bind a provided user id to a socket
    }
    fn on_binary(&self, data: Vec<u8>, socket: Arc<Socket<SocketState>>) { }
}

// Create a new engineio layer
let layer = EngineIoLayer::new(MyHandler);

let app = axum::Router::new()
    .route("/", get(|| async { "Hello, World!" }))
    .layer(layer);

axum::Server::bind(&"127.0.0.1:3000".parse().unwrap())
    .serve(app.into_make_service())
```

#### Basic example with salvo (with the `hyper-v1` feature flag) : 
```rust
use engineioxide::layer::EngineIoLayer;
use engineioxide::handler::EngineIoHandler;
use engineioxide::{Socket, DisconnectReason};
// Global state
#[derive(Debug, Clone)]
struct MyHandler {
    user_cnt: AtomicUsize;
}

// Socket state
#[derive(Debug, Clone)]
struct SocketState {
    id: Mutex<String>;
}

impl EngineIoHandler for MyHandler {
    type Data = SocketState;

    fn on_connect(&self, socket: Arc<Socket<SocketState>>) { 
        let cnt = self.user_cnt.fetch_add(1) + 1;
        socket.emit(cnt.to_string()).ok();
    }
    fn on_disconnect(&self, socket: Arc<Socket<SocketState>>, reason: DisconnectReason) { 
        let cnt = self.user_cnt.fetch_sub(1) - 1;
        socket.emit(cnt.to_string()).ok();
    }
    fn on_message(&self, msg: String, socket: Arc<Socket<SocketState>>) { 
        *socket.data.id.lock().unwrap() = msg; // bind a provided user id to a socket
    }
    fn on_binary(&self, data: Vec<u8>, socket: Arc<Socket<SocketState>>) { }
}

// Create a new engineio layer
let layer = EngineIoLayer::new(MyHandler)
    .with_hyper_v1()   // Enable the hyper-v1 compatibility layer for salvo
    .compat()          // Enable the salvo compatibility layer

let router = Router::with_path("/engine.io").hoop(layer).goal(hello);
let acceptor = TcpListener::new("127.0.0.1:3000").bind().await;
Server::new(acceptor).serve(router);
```