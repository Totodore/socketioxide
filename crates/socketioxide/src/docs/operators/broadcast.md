## Broadcasts to all sockets without any filtering (except the current socket).

#### Example
```
# use socketioxide::{SocketIo, extract::*};
# use serde_json::Value;
let (_, io) = SocketIo::new_svc();
io.ns("/", |socket: SocketRef| {
    socket.on("test", |socket: SocketRef, Data::<Value>(data)| async move {
        // This message will be broadcast to all sockets in this namespace
        socket.broadcast().emit("test", &data);
    });
});
