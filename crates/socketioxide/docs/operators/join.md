# Add all sockets selected with the previous operators to the specified room(s).

This will return a `Future` that must be awaited because socket.io may communicate with remote instances
if you use horizontal scaling through remote adapters.

# Example
```rust
# use socketioxide::{SocketIo, extract::*};
async fn handler(socket: SocketRef) {
    // Add all sockets that are in room1 and room3 to room4 and room5
    socket.within("room1").within("room3").join(["room4", "room5"]).await.unwrap();
    // We should retrieve all the local sockets that are in room3 and room5
    let sockets = socket.within("room4").within("room5").sockets();
}

let (_, io) = SocketIo::new_svc();
io.ns("/", async |s: SocketRef| s.on("test", handler));
```
