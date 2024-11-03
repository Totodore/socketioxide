## Emits a message to the client and wait for (an) acknowledgement(s).

See [`emit()`](#method.emit) for more details on emitting messages.

The acknowledgement has a timeout specified in the config (5s by default)
(see [`SocketIoBuilder::ack_timeout`]) or with the [`timeout()`] operator.

To get acknowledgements, an [`AckStream`] is returned.
It can be used in two ways:
* As a [`Stream`]: It will yield all the ack responses with their corresponding socket id
  received from the client. It can useful when broadcasting to multiple sockets and therefore expecting
  more than one acknowledgement. If you want to get the socket from this id, use [`io::get_socket()`].
* As a [`Future`]: It will yield the first ack response received from the client.
  Useful when expecting only one acknowledgement.

## Errors
If the packet encoding failed an [`EncodeError`] is **immediately** returned.

If the socket is full or if it has been closed before receiving the acknowledgement,
an [`SendError::Socket`] will be **immediately returned** and the value to send will be given back.

If the client didn't respond before the timeout, the [`AckStream`] will yield
an [`AckError::Timeout`]. If the data sent by the client is not deserializable as `V`,
an [`AckError::Decode`] will be yielded.

[`timeout()`]: crate::operators::ConfOperators#method.timeout
[`SocketIoBuilder::ack_timeout`]: crate::SocketIoBuilder#method.ack_timeout
[`Stream`]: futures_core::stream::Stream
[`Future`]: futures_core::future::Future
[`AckError`]: crate::AckError
[`AckError::Decode`]: crate::AckError::Decode
[`AckError::Timeout`]: crate::AckError::Timeout
[`AckError::Socket`]: crate::AckError::Socket
[`AckError::Socket(SocketError::Closed)`]: crate::SocketError::Closed
[`EncodeError`]: crate::EncodeError
[`io::get_socket()`]: crate::SocketIo#method.get_socket


### Example with one socket and no configuration
```
# use socketioxide::{SocketIo, extract::*};
# use serde_json::Value;
# use std::sync::Arc;
let (_, io) = SocketIo::new_svc();
io.ns("/", |socket: SocketRef| {
    socket.on("test", |socket: SocketRef, Data::<Value>(data)| async move {
        // Emit a test message and wait for an acknowledgement with the timeout specified in the config
        match socket.emit_with_ack::<_, Value>("test", &data).unwrap().await {
            Ok(ack) => println!("Ack received {:?}", ack),
            Err(err) => println!("Ack error {:?}", err),
        }
   });
});
```

### Example with timeout configuration
```
# use socketioxide::{SocketIo, extract::*};
# use serde_json::Value;
# use std::sync::Arc;
# use tokio::time::Duration;
let (_, io) = SocketIo::new_svc();
io.ns("/", |socket: SocketRef| {
    socket.on("test", |socket: SocketRef, Data::<Value>(data)| async move {
        // Emit a test message and wait for an acknowledgement with the timeout specified in the config
        match socket.timeout(Duration::from_millis(2)).emit_with_ack::<_, Value>("test", &data).unwrap().await {
            Ok(ack) => println!("Ack received {:?}", ack),
            Err(err) => println!("Ack error {:?}", err),
        }
   });
});
```

### Example with multiple sockets
```
# use socketioxide::{SocketIo, extract::*};
# use serde_json::Value;
# use futures_util::stream::StreamExt;
let (_, io) = SocketIo::new_svc();
io.ns("/", |socket: SocketRef| {
    socket.on("test", |socket: SocketRef, Data::<Value>(data)| async move {
        // Emit a test message in the room1 and room3 rooms,
        // except for the room2 room with the binary payload received
        let ack_stream = socket.to("room1")
            .to("room3")
            .except("room2")
            .emit_with_ack::<_, String>("message-back", &data)
            .unwrap();
        ack_stream.for_each(|(id, ack)| async move {
            match ack {
                Ok(ack) => println!("Ack received, socket {} {:?}", id, ack),
                Err(err) => println!("Ack error, socket {} {:?}", id, err),
            }
        }).await;
    });
});
