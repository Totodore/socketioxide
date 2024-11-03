# Disconnects all sockets selected with the previous operators.

### Example
```
# use socketioxide::{SocketIo, extract::*};
let (_, io) = SocketIo::new_svc();
io.ns("/", |socket: SocketRef| {
  socket.on("test", |socket: SocketRef| async move {
    // Disconnect all sockets in the room1 and room3 rooms, except for the room2
    socket.within("room1").within("room3").except("room2").disconnect().unwrap();
  });
});
