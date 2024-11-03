# Makes all sockets selected with the previous operators leave the given room(s).

### Example
```
# use socketioxide::{SocketIo, extract::*};
let (_, io) = SocketIo::new_svc();
io.ns("/", |socket: SocketRef| {
socket.on("test", |socket: SocketRef| async move {
    // Remove all sockets that are in the room1 and room3 from the room4 and room5
    socket.within("room1").within("room3").leave(["room4", "room5"]).unwrap();
  });
});
