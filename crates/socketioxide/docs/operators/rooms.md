# Get all room names in the current namespace.

# Example
```rust
# use socketioxide::{SocketIo, extract::SocketRef};
fn handler(socket: SocketRef, io: SocketIo) {
    println!("Socket connected to the / namespace with id: {}", socket.id);
    let rooms = io.rooms().unwrap();
    println!("All rooms in the / namespace: {:?}", rooms);
}

let (_, io) = SocketIo::new_svc();
io.ns("/", handler);
```
