# Select all the sockets in the given rooms except the current socket.

When called from a socket, if you want to also include it use the [`within()`](#method.within) operator.

However, when called from the [`io`] (global context) level there will be no difference.

[`io`]: crate::SocketIo

# Example
```
# use socketioxide::{SocketIo, extract::*};
# use serde_json::Value;
async fn handler(socket: SocketRef, io: SocketIo, Data::<Value>(data)) {
    let other_rooms = "room4".to_string();
    // In room1, room2, room3 and room4 except the current socket
    socket
        .to("room1")
        .to(["room2", "room3"])
        .to(vec![other_rooms])
        .emit("test", &data);
    // In room1, room2, room3 and room4 including the current socket
    io
        .to("room1")
        .to(["room2", "room3"])
        .to(vec![other_rooms])
        .emit("test", &data);
}

let (_, io) = SocketIo::new_svc();
io.ns("/", |s: SocketRef| s.on("test", handler));
```
