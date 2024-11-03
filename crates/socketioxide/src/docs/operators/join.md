# Makes all sockets selected with the previous operators join the given room(s).

### Example
```
# use socketioxide::{SocketIo, extract::*};
let (_, io) = SocketIo::new_svc();
io.ns("/", |socket: SocketRef| {
  socket.on("test", |socket: SocketRef| async move {
    // Add all sockets that are in the room1 and room3 to the room4 and room5
    socket.within("room1").within("room3").join(["room4", "room5"]).unwrap();
  });
});
