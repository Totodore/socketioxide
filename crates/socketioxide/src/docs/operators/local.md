# Broadcast to all sockets only connected on this node.
When using the default in-memory adapter, this operator is a no-op.

# Example
```
# use socketioxide::{SocketIo, extract::*};
# use serde_json::Value;
fn handler(socket: SocketRef) {
    // This message will be broadcast to all sockets in this
    // namespace and connected on this node
    socket.local().emit("test", &data);
}

let (_, io) = SocketIo::new_svc();
io.ns("/", |s: SocketRef| s.on("test", handler));
```
