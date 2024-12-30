# Get all the local and remote sockets selected with the previous operators.

<div class="warning">
    Use <code>sockets()</code> if you only have a single node.
</div>

Avoid using this method if you want to immediately perform actions on the sockets.
Instead, directly apply the actions using operators:

## Correct Approach
```rust
# use socketioxide::{SocketIo, extract::*};
# async fn doc_main() {
# let (_, io) = SocketIo::new_svc();
io.within("room1").emit("foo", "bar").await.unwrap();
io.within("room1").disconnect().await.unwrap();
# }
```

## Incorrect Approach
```rust
# use socketioxide::{SocketIo, extract::*};
# async fn doc_main() {
# let (_, io) = SocketIo::new_svc();
let sockets = io.within("room1").fetch_sockets().await.unwrap();
for socket in sockets {
    println!("Socket id: {:?}", socket.id());
    socket.emit("test", &"Hello").await.unwrap();
    socket.leave("room1").await.unwrap();
}
# }
```

# Example
```rust
# use socketioxide::{SocketIo, extract::*};
# async fn doc_main() {
let (_, io) = SocketIo::new_svc();
let sockets = io.within("room1").fetch_sockets().await.unwrap();
for socket in sockets {
    println!("Socket ID: {:?}", socket.data().id);
}
# }
```
