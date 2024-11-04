# Get all room names on the current namespace.

# Example
```
# use socketioxide::{SocketIo, extract::SocketRef};
fn handler(socket: SocketRef, io: SocketIo) {
    println!("Socket connected on /test namespace with id: {}", socket.id);
    let rooms = io.rooms().unwrap();
    println!("All rooms on / namespace: {:?}", rooms);
}

let (_, io) = SocketIo::new_svc();
io.ns("/", handler);
```
