## Selects all sockets in the given rooms except the current socket.

If it is called from the `Namespace` level there will be no difference with the `within()` operator

If you want to include the current socket, use the `within()` operator.
#### Example
```
# use socketioxide::{SocketIo, extract::*};
# use serde_json::Value;
let (_, io) = SocketIo::new_svc();
io.ns("/", |socket: SocketRef| {
    socket.on("test", |socket: SocketRef, Data::<Value>(data)| async move {
        let other_rooms = "room4".to_string();
        // In room1, room2, room3 and room4 except the current
        socket
            .to("room1")
            .to(["room2", "room3"])
            .to(vec![other_rooms])
            .emit("test", &data);
    });
});
