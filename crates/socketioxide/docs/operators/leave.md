# Remove all sockets selected with the previous operators from the specified room(s).

# Example
```rust
# use socketioxide::{SocketIo, extract::*};
fn handler(socket: SocketRef) {
    // Remove all sockets that are in room1 and room3 from room4 and room5
    socket.within("room1").within("room3").leave(["room4", "room5"]).unwrap();
    let sockets = socket.within("room4").within("room5").sockets().unwrap();
}

let (_, io) = SocketIo::new_svc();
io.ns("/", |s: SocketRef| s.on("test", handler));
```
