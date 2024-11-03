## Filters out all sockets selected with the previous operators which are in the given rooms.

#### Example
```
# use socketioxide::{SocketIo, extract::*};
# use serde_json::Value;
let (_, io) = SocketIo::new_svc();
io.ns("/", |socket: SocketRef| {
    socket.on("register1", |socket: SocketRef, Data::<Value>(data)| async move {
        socket.join("room1");
    });
    socket.on("register2", |socket: SocketRef, Data::<Value>(data)| async move {
        socket.join("room2");
    });
    socket.on("test", |socket: SocketRef, Data::<Value>(data)| async move {
        // This message will be broadcast to all sockets in the Namespace
        // except for ones in room1 and the current socket
        socket.broadcast().except("room1").emit("test", &data);
    });
});
