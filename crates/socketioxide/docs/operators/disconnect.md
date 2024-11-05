# Disconnect all sockets selected with the previous operators.

# Example
```rust
# use socketioxide::{SocketIo, extract::*};
fn handler(socket: SocketRef) {
    // Disconnect all sockets in the room1 and room3 rooms, except for room2.
    socket.within("room1").within("room3").except("room2").disconnect().unwrap();
}
let (_, io) = SocketIo::new_svc();
io.ns("/", |s: SocketRef| s.on("test", handler));
```
