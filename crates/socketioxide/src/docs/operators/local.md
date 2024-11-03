## Broadcasts to all sockets only connected on this node (when using multiple nodes).
When using the default in-memory adapter, this operator is a no-op.

#### Example
```
# use socketioxide::{SocketIo, extract::*};
# use serde_json::Value;
let (_, io) = SocketIo::new_svc();
io.ns("/", |socket: SocketRef| {
    socket.on("test", |socket: SocketRef, Data::<Value>(data)| async move {
        // This message will be broadcast to all sockets in this namespace and connected on this node
        socket.local().emit("test", &data);
    });
});
