# Emit a message to one or many clients and wait for one or more acknowledgments.

See [`emit()`](#method.emit) for more details on emitting messages.

The acknowledgment has a timeout specified in the config (5 seconds by default)
(see [`SocketIoBuilder::ack_timeout`]) or can be set with the [`timeout()`](#method.timeout) operator.

To receive acknowledgments, an [`AckStream`] is returned. It can be used in two ways:
* As a [`Stream`]: This will yield all the acknowledgment responses, along with the corresponding socket ID, received from the client. This is useful when broadcasting to multiple sockets and expecting more than one acknowledgment. To get the socket from this ID, use [`io::get_socket()`].
* As a [`Future`]: This will yield the first acknowledgment response received from the client, useful when expecting only one acknowledgment.

# Errors
If packet encoding fails, an [`ParserError`] is **immediately** returned.

If the socket is full or if it is closed before receiving the acknowledgment,
a [`SendError::Socket`] will be **immediately** returned, and the value to send will be given back.

If the client does not respond before the timeout, the [`AckStream`] will yield
an [`AckError::Timeout`]. If the data sent by the client is not deserializable as `V`,
an [`AckError::Decode`] will be yielded.

[`SocketIoBuilder::ack_timeout`]: crate::SocketIoBuilder#method.ack_timeout
[`Stream`]: futures_core::stream::Stream
[`Future`]: std::future::Future
[`AckError`]: crate::AckError
[`AckError::Decode`]: crate::AckError::Decode
[`AckError::Timeout`]: crate::AckError::Timeout
[`AckError::Socket`]: crate::AckError::Socket
[`AckError::Socket(SocketError::Closed)`]: crate::SocketError::Closed
[`SendError::Socket`]: crate::SendError::Socket
[`ParserError`]: crate::ParserError
[`io::get_socket()`]: crate::SocketIo#method.get_socket

# Single-socket example
```rust
# use socketioxide::{SocketIo, extract::*};
# use serde_json::Value;
# use std::sync::Arc;
async fn handler(socket: SocketRef, Data(data): Data::<Value>) {
    // Emit a test message and wait for an acknowledgment with the timeout specified in the global config
    match socket.emit_with_ack::<_, Value>("test", &data).unwrap().await {
        Ok(ack) => println!("Ack received {:?}", ack),
        Err(err) => println!("Ack error {:?}", err),
    }
}

let (_, io) = SocketIo::new_svc();
io.ns("/", |socket: SocketRef| socket.on("test", handler));
```

# Single-socket example with custom acknowledgment timeout
```rust
# use socketioxide::{SocketIo, extract::*};
# use serde_json::Value;
# use std::sync::Arc;
# use tokio::time::Duration;
async fn handler(socket: SocketRef, Data(data): Data::<Value>) {
    // Emit a test message and wait for an acknowledgment with the timeout specified here
    match socket.timeout(Duration::from_millis(2)).emit_with_ack::<_, Value>("test", &data).unwrap().await {
        Ok(ack) => println!("Ack received {:?}", ack),
        Err(err) => println!("Ack error {:?}", err),
    }
}

let (_, io) = SocketIo::new_svc();
io.ns("/", |socket: SocketRef| socket.on("test", handler));
```

# Broadcast example

Here the emit method will return a `Future` that must be awaited because socket.io may communicate
with remote instances if you use horizontal scaling through remote adapters.

```rust
# use socketioxide::{SocketIo, extract::*};
# use serde_json::Value;
# use futures_util::stream::StreamExt;
async fn handler(socket: SocketRef, Data(data): Data::<Value>) {
    // Emit a test message in the room1 and room3 rooms,
    // except for room2, with the binary payload received
    let ack_stream = socket.to("room1")
        .to("room3")
        .except("room2")
        .emit_with_ack::<_, String>("message-back", &data)
        .await
        .unwrap();
    ack_stream.for_each(async |(id, ack)| {
        match ack {
            Ok(ack) => println!("Ack received, socket {} {:?}", id, ack),
            Err(err) => println!("Ack error, socket {} {:?}", id, err),
        }
    }).await;
}

let (_, io) = SocketIo::new_svc();
io.ns("/", |s: SocketRef| s.on("test", handler));
```
