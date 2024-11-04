# Filter out all sockets selected with the previous operators which are in the given rooms.

# Example
```
# use socketioxide::{SocketIo, extract::*};
# use serde_json::Value;
fn handler(socket: SocketRef, Data::<Value>(data)) {
    // This message will be broadcast to all sockets in the Namespace
    // except for ones in room1 and the current socket
    socket.broadcast().except("room1").emit("test", &data);
}

let (_, io) = SocketIo::new_svc();
io.ns("/", |socket: SocketRef| {
    socket.on("register1", |s: SocketRef| s.join("room1"));
    socket.on("register2", |s: SocketRef| s.join("room2"));
    socket.on("test", handler);
});
```
