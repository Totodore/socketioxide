# Emit a message to one or many clients

If you provide tuple-like data (tuples, arrays), it will be considered as multiple arguments.
Therefore, if you want to send an array as the _first_ argument of the payload,
you need to wrap it in an array or a tuple. A [`Vec`] will always be considered a single argument.

# Emitting binary data
To emit binary data, you must use a data type that implements [`Serialize`] as binary data.
Currently, if you use `Vec<u8>`, it will be considered a sequence of numbers rather than binary data.
To handle this, you can either use a special type like [`Bytes`] or the [`serde_bytes`] crate.
If you want to emit generic data that may contain binary, use [`rmpv::Value`] instead of
[`serde_json::Value`], as binary data will otherwise be serialized as a sequence of numbers.

# Errors
* When encoding the data, a [`SendError::Serialize`] may be returned.
* If the underlying engine.io connection is closed, a [`SendError::Socket(SocketError::Closed)`]
  will be returned, and the data you attempted to send will be included in the error.
* If the packet buffer is full, a [`SendError::Socket(SocketError::InternalChannelFull)`]
  will be returned, and the data you attempted to send will be included in the error.
  See the [`SocketIoBuilder::max_buffer_size`] option for more information on internal buffer configuration.

[`SocketIoBuilder::max_buffer_size`]: crate::SocketIoBuilder#method.max_buffer_size
[`SendError::Serialize`]: crate::SendError::Serialize
[`SendError::Socket(SocketError::Closed)`]: crate::SocketError::Closed
[`SendError::Socket(SocketError::InternalChannelFull)`]: crate::SocketError::InternalChannelFull
[`Bytes`]: bytes::Bytes
[`serde_bytes`]: https://docs.rs/serde_bytes
[`rmpv::Value`]: https://docs.rs/rmpv
[`serde_json::Value`]: https://docs.rs/serde_json/latest/serde_json/value

# Single-socket example
```rust
# use socketioxide::{SocketIo, extract::*};
# use serde_json::Value;
# use std::sync::Arc;
fn handler(socket: SocketRef, Data(data): Data::<Value>) {
    // Emit a test message to the client
    socket.emit("test", &data).ok();
    // Emit a test message with multiple arguments to the client
    socket.emit("test", &("world", "hello", 1)).ok();
    // Emit a test message with an array as the first argument
    let arr = [1, 2, 3, 4];
    socket.emit("test", &[arr]).ok();
}

let (_, io) = SocketIo::new_svc();
io.ns("/", |socket: SocketRef| socket.on("test", handler));
```

# Single-socket binary example with the `bytes` crate
```rust
# use socketioxide::{SocketIo, extract::*};
# use serde_json::Value;
# use std::sync::Arc;
# use bytes::Bytes;
fn handler(socket: SocketRef, Data(data): Data::<(String, Bytes, Bytes)>) {
    // Emit a test message to the client
    socket.emit("test", &data).ok();
    // Emit a test message with multiple arguments to the client
    socket.emit("test", &("world", "hello", Bytes::from_static(&[1, 2, 3, 4]))).ok();
    // Emit a test message with an array as the first argument
    let arr = [1, 2, 3, 4];
    socket.emit("test", &[arr]).ok();
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
# use std::sync::Arc;
# use bytes::Bytes;
async fn handler(socket: SocketRef, Data(data): Data::<(String, Bytes, Bytes)>) {
    // Emit a test message in the room1 and room3 rooms, except for room2, with the received binary payload
    socket.to("room1").to("room3").except("room2").emit("test", &data).await;

    // Emit a test message with multiple arguments to the client
    socket.to("room1").emit("test", &("world", "hello", 1)).await;

    // Emit a test message with an array as the first argument
    let arr = [1, 2, 3, 4];
    socket.to("room2").emit("test", &[arr]).await;
}

let (_, io) = SocketIo::new_svc();
io.ns("/", |s: SocketRef| s.on("test", handler));
```
