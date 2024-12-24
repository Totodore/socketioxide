# Remove all sockets selected with the previous operators from the specified room(s).

This will return a `Future` that must be awaited because socket.io may communicate with remote instances
if you use horizontal scaling through remote adapters.

# Example
```rust
# use socketioxide::{SocketIo, extract::*};
async fn handler(socket: SocketRef) {
    // Remove all sockets that are in room1 and room3 from room4 and room5
    socket.within("room1").within("room3").leave(["room4", "room5"]).await.unwrap();
}

let (_, io) = SocketIo::new_svc();
io.ns("/", |s: SocketRef| s.on("test", handler));
```
