# Set the volatile flag for the emit. When set, the event may be dropped
if the client is not ready to receive it (e.g. the connection is buffering
or not connected). This is useful for events that are not critical, such
as position updates in a game.

Because volatile events use a separate channel that bypasses the main
mpsc buffer, they may arrive **out of order** relative to regular events
emitted around the same time. Only use volatile when ordering relative to
regular events is not important.

See [socket.io volatile events](https://socket.io/docs/v4/emitting-events/#volatile-events).

# Example
```rust
# use socketioxide::{SocketIo, extract::*};
# use serde::Serialize;
#[derive(Serialize)]
struct GameState { x: f64, y: f64 }

let (_, io) = SocketIo::new_svc();
io.ns("/", async |socket: SocketRef| {
    // Direct volatile emit — may be dropped if the socket is not ready
    socket.volatile().emit("position", &GameState { x: 1.0, y: 2.0 }).ok();

    // Volatile broadcast to a room
    socket.volatile().to("game_room").emit("update", &42).await.ok();
});
```
