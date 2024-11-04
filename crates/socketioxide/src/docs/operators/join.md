# Add all sockets selected with the previous operators from the given room(s).

# Example
```
# use socketioxide::{SocketIo, extract::*};
fn handler(socket: SocketRef) {
    // Add all sockets that are in the room1 and room3 to the room4 and room5
    socket.within("room1").within("room3").join(["room4", "room5"]).unwrap();
    let sockets = socket.within("room4").within("room5").sockets().unwrap();
}

let (_, io) = SocketIo::new_svc();
io.ns("/", |s: SocketRef| s.on("test", handler));
```
