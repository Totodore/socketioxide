# Get all sockets selected with the previous operators.

This can be used to retrieve any extension data (with the `extensions` feature enabled) from the sockets or to make certain sockets join other rooms.

This will return a `Future` that must be awaited because socket.io may communicate with remote instances
if you use horizontal scaling through remote adapters.

# Example
```rust
# use socketioxide::{SocketIo, extract::*};
async fn handler(socket: SocketRef) {
    // Find extension data in each socket in the room1 and room3 rooms, except for room2
    let sockets = socket.within("room1").within("room3").except("room2").sockets().await.unwrap();
    for socket in sockets {
        println!("Socket custom string: {:?}", socket.extensions.get::<String>());
    }
}

let (_, io) = SocketIo::new_svc();
io.ns("/", |s: SocketRef| s.on("test", handler));
```
