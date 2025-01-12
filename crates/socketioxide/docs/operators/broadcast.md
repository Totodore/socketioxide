# Broadcast to all sockets without any filtering (except the current socket).
If you want to include the current socket use the broadcast operators from the [`io`] global context.

[`io`]: crate::SocketIo

# Example
```rust
# use socketioxide::{SocketIo, extract::*};
# use serde_json::Value;
async fn handler(io: SocketIo, socket: SocketRef, Data(data): Data::<Value>) {
    // This message will be broadcast to all sockets in this namespace except this one.
    socket.broadcast().emit("test", &data).await;
    // This message will be broadcast to all sockets in this namespace, including this one.
    io.emit("test", &data).await;
}

let (_, io) = SocketIo::new_svc();
io.ns("/", |s: SocketRef| s.on("test", handler));
```
