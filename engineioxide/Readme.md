## Engineioxide does the heavy lifting for [Socketioxide](https://docs.rs/socketioxide/latest/socketioxide/), a SocketIO server implementation in Rust which integrates with the [tower](https://docs.rs/tower/latest/tower/) stack.

### It can also be used as a standalone server for [EngineIO clients](https://github.com/socketio/engine.io-client).

### Engine.IO example echo implementation with Axum :
```rust

#[derive(Clone)]
struct MyHandler;

impl EngineIoHandler for MyHandler {
    type Data = ();
    
    fn on_connect(&self, socket: &Socket<Self>) {
        println!("socket connect {}", socket.sid);
    }
    fn on_disconnect(&self, socket: &Socket<Self>) {
        println!("socket disconnect {}", socket.sid);
    }

    fn on_message(&self, msg: String, socket: &Socket<Self>) {
        println!("Ping pong message {:?}", msg);
        socket.emit(msg).ok();
    }

    fn on_binary(&self, data: Vec<u8>, socket: &Socket<Self>) {
        println!("Ping pong binary message {:?}", data);
        socket.emit_binary(data).ok();
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::builder().finish();
    tracing::subscriber::set_global_default(subscriber)?;

    info!("Starting server");
    let app = axum::Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .layer(EngineIoLayer::new(MyHandler));

    Server::bind(&"0.0.0.0:3000".parse().unwrap())
        .serve(app.into_make_service())
        .await?;

    Ok(())
}
```

### Supported Protocols
You can enable support for other EngineIO protocol implementations through feature flags. 
The latest supported protocol version is enabled by default. 

To add support for another protocol version, adjust your dependency configuration accordingly:

```toml
[dependencies]
# Enables the `v3` protocol (`v4` is also implicitly enabled, as it's the default).
engineioxide = { version = "0.3.0", features = ["v3"] }
```

To enable *a single protocol version only*, disable default features:

```toml
[dependencies]
# Enables the `v3` protocol only.
engineioxide = { version = "0.3.0", features = ["v3"], default-features = false }
```
