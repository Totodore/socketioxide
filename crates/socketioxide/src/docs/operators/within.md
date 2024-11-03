## Selects all sockets in the given rooms.

It does include the current socket contrary to the `to()` operator.
If it is called from the `Namespace` level there will be no difference with the `to()` operator

#### Example
```
# use socketioxide::{SocketIo, extract::*};
# use serde_json::Value;
let (_, io) = SocketIo::new_svc();
io.ns("/", |socket: SocketRef| {
    socket.on("test", |socket: SocketRef, Data::<Value>(data)| async move {
        let other_rooms = "room4".to_string();
        // In room1, room2, room3 and room4 including the current socket
        socket
            .within("room1")
            .within(["room2", "room3"])
            .within(vec![other_rooms])
            .emit("test", &data);
    });
});
