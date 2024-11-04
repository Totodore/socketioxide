# Set a custom timeout when sending a message with an acknowledgement.

* See [`SocketIoBuilder::ack_timeout`](crate::SocketIoBuilder) for the default timeout.
* See [`emit_with_ack()`](#method.emit_with_ack) for more details on acknowledgements.

# Example
```
# use socketioxide::{SocketIo, extract::*};
# use serde_json::Value;
# use futures_util::stream::StreamExt;
# use std::time::Duration;
fn handler(socket: SocketRef, Data::<Value>(data)) {
    // Emit a test message in the room1 and room3 rooms, except for the room2
    // room with the binary payload received, wait for 5 seconds for an acknowledgement
    socket.to("room1")
          .to("room3")
          .except("room2")
          .timeout(Duration::from_secs(5))
          .emit_with_ack::<_, Value>("message-back", &data)
          .unwrap()
          .for_each(|(id, ack)| async move {
            match ack {
                Ok(ack) => println!("Ack received, socket {} {:?}", id, ack),
                Err(err) => println!("Ack error, socket {} {:?}", id, err),
            }
          }).await;
}

let (_, io) = SocketIo::new_svc();
io.ns("/", |s: SocketRef| s.on("test", handler));
```
