# Broadcast to all sockets without any filtering (except the current socket).
If you want to include the current socket use emit operators from the [`io`] global context.

[`io`]: crate::SocketIo

# Example
```
# use socketioxide::{SocketIo, extract::*};
# use serde_json::Value;
fn handler(socket: SocketRef, ) {
    // This message will be broadcast to all sockets in this namespace excepted this one.
    socket.broadcast().emit("test", &data);
    // This message will be broadcast to all sockets in this namespace including this one.
    io.emit("test", &data);
}
let (_, io) = SocketIo::new_svc();
io.ns("/", |s: SocketRef| s.on("test", handler));
```
