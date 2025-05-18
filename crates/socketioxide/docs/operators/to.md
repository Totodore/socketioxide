# Select all the sockets in the given rooms except for the current socket.

When called from a socket, if you want to also include it, use the [`within()`](#method.within) operator.

However, when called from the [`io`] (global context) level, there will be no difference.

[`io`]: crate::SocketIo

# Example
```rust
# use socketioxide::{SocketIo, extract::*};
# use serde_json::Value;
async fn handler(socket: SocketRef, io: SocketIo, Data(data): Data::<Value>) {
    // Emit a message to all sockets in room1, room2, room3, and room4, except the current socket
    socket
        .to("room1")
        .to(["room2", "room3"])
        .emit("test", &data)
        .await;

    // Emit a message to all sockets in room1, room2, room3, and room4, including the current socket
    io
        .to("room1")
        .to(["room2", "room3"])
        .emit("test", &data)
        .await;
}

let (_, io) = SocketIo::new_svc();
io.ns("/", async |s: SocketRef| s.on("test", handler));
```
