# Get all room names across all nodes in the current namespace.

This will return a `Future` that must be awaited because socket.io may communicate with remote instances
if you use horizontal scaling through remote adapters.

# Example
```rust
# use socketioxide::{SocketIo, extract::SocketRef};
async fn handler(socket: SocketRef, io: SocketIo) {
    println!("Socket connected to the / namespace with id: {}", socket.id);
    let rooms = io.rooms().await.unwrap();
    println!("All rooms in the / namespace: {:?}", rooms);
}

let (_, io) = SocketIo::new_svc();
io.ns("/", handler);
```
