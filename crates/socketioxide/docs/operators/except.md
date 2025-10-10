# Filter out all sockets selected with the previous operators that are in the specified rooms.

# Example
```rust
# use socketioxide::{SocketIo, extract::*};
# use serde_json::Value;
async fn handler(socket: SocketRef, Data(data): Data::<Value>) {
    // This message will be broadcast to all sockets in the namespace,
    // except for those in room1 and the current socket
    socket.broadcast().except("room1").emit("test", &data).await;
}

let (_, io) = SocketIo::new_svc();
io.ns("/", async |socket: SocketRef| {
    socket.on("register1", async |s: SocketRef| s.join("room1"));
    socket.on("register2", async |s: SocketRef| s.join("room2"));
    socket.on("test", handler);
});
```
