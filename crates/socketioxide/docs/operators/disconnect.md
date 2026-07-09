# Disconnect all sockets selected with the previous operators.

This will return a `Future` that must be awaited because socket.io may communicate with remote instances
if you use horizontal scaling through remote adapters.

# Example from a socket
```rust
# use socketioxide::{SocketIo, extract::*};
async fn handler(socket: SocketRef) {
    // Disconnect all sockets in the room1 and room3 rooms, except for room2.
    socket.within("room1").within("room3").except("room2").disconnect().await.unwrap();
}
let (_, io) = SocketIo::new_svc();
io.ns("/", async |s: SocketRef| s.on("test", handler));
```

# Example from the io struct
```rust
# use socketioxide::{SocketIo, extract::*};
async fn handler(socket: SocketRef, io: SocketIo) {
    // Disconnect all sockets in the room1 and room3 rooms, except for room2.
    io.within("room1").within("room3").except("room2").disconnect().await.unwrap();
}
let (_, io) = SocketIo::new_svc();
io.ns("/", async |s: SocketRef| s.on("test", handler));
```
