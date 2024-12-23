# Select all the sockets in the given rooms (current one included).

This includes the current socket, in contrast to the [`to()`](#method.to) operator.

However, when called from the [`io`] (global context) level, there will be no difference.

[`to()`]: crate::operators::BroadcastOperators#method.to
[`io`]: crate::SocketIo

# Example
```rust
# use socketioxide::{SocketIo, extract::*};
# use serde_json::Value;
async fn handler(socket: SocketRef, Data(data): Data::<Value>) {
    let other_rooms = "room4".to_string();
    // Emit a message to all sockets in room1, room2, room3, and room4, including the current socket
    socket
        .within("room1")
        .within(["room2", "room3"])
        .within(vec![other_rooms])
        .emit("test", &data)
        .await;
}

let (_, io) = SocketIo::new_svc();
io.ns("/", |s: SocketRef| s.on("test", handler));
```
