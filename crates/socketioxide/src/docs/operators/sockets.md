# Gets all sockets selected with the previous operators.

It can be used to retrieve any extension data (with the `extensions` feature enabled) from the sockets or to make some sockets join other rooms.

## Example
```
# use socketioxide::{SocketIo, extract::*};
let (_, io) = SocketIo::new_svc();
io.ns("/", |socket: SocketRef| {
  socket.on("test", |socket: SocketRef| async move {
    // Find an extension data in each sockets in the room1 and room3 rooms, except for the room2
    let sockets = socket.within("room1").within("room3").except("room2").sockets().unwrap();
    for socket in sockets {
        println!("Socket custom string: {:?}", socket.extensions.get::<String>());
    }
  });
});
