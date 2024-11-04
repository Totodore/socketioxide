# Emit a message one or many clients

If you provide tuple-like data (tuple, arrays), it will be considered as multiple arguments.
Therefore if you want to send an array as the _first_ argument of the payload,
you need to wrap it in an array or a tuple. [`Vec`] will be always considered as a single argument though.

# Emitting binary data
To emit binary data, you must use a data type that implements [`Serialize`] as binary data.
Currently if you use `Vec<u8>` it will be considered as a number sequence and not binary data.
To counter that you must either use a special type like [`Bytes`] or use the [`serde_bytes`] crate.
If you want to emit generic data that may contains binary, use [`rmpv::Value`] rather
than [`serde_json::Value`] otherwise the binary data will also be serialized as a number sequence.

# Errors
* When encoding the data a [`SendError::Serialize`] may be returned.
* If the underlying engine.io connection is closed a [`SendError::Socket(SocketError::Closed)`]
  will be returned and the provided data to be send will be given back in the error.
* If the packet buffer is full, a [`SendError::Socket(SocketError::InternalChannelFull)`]
  will be returned and the provided data to be send will be given back in the error.
  See [`SocketIoBuilder::max_buffer_size`] option for more infos on internal buffer config.

[`SocketIoBuilder::max_buffer_size`]: crate::SocketIoBuilder#method.max_buffer_size
[`SendError::Serialize`]: crate::SendError::Serialize
[`SendError::Socket(SocketError::Closed)`]: crate::SocketError::Closed
[`SendError::Socket(SocketError::InternalChannelFull)`]: crate::SocketError::InternalChannelFull
[`Bytes`]: bytes::Bytes
[`serde_bytes`]: https://docs.rs/serde_bytes
[`rmpv::Value`]: https://docs.rs/rmpv
[`serde_json::Value`]: https://docs.rs/serde_json/latest/serde_json/value

# Single-socket example
```
# use socketioxide::{SocketIo, extract::*};
# use serde_json::Value;
# use std::sync::Arc;
fn handler(socket: SocketRef, Data::<Value>(data)) {
    // Emit a test message to the client
    socket.emit("test", &data).ok();
    // Emit a test message with multiple arguments to the client
    socket.emit("test", &("world", "hello", 1)).ok();
    // Emit a test message with an array as the first argument
    let arr = [1, 2, 3, 4];
    socket.emit("test", &[arr]).ok();
}

let (_, io) = SocketIo::new_svc();
io.ns("/", |socket: SocketRef| {
    socket.on("test", handler);
});
```

# Single-socket binary example with the `bytes` crate
```
# use socketioxide::{SocketIo, extract::*};
# use serde_json::Value;
# use std::sync::Arc;
# use bytes::Bytes;
fn handler(socket: SocketRef, Data::<(String, Bytes, Bytes)>(data)) {
    // Emit a test message to the client
    socket.emit("test", &data).ok();
    // Emit a test message with multiple arguments to the client
    socket.emit("test", &("world", "hello", Bytes::from_static(&[1, 2, 3, 4]))).ok();
    // Emit a test message with an array as the first argument
    let arr = [1, 2, 3, 4];
    socket.emit("test", &[arr]).ok();
}

let (_, io) = SocketIo::new_svc();
io.ns("/", |socket: SocketRef| {
    socket.on("test", handler);
});
```

# Broadcast example
```
# use socketioxide::{SocketIo, extract::*};
# use serde_json::Value;
# use std::sync::Arc;
# use bytes::Bytes;
fn handler(socket: SocketRef, Data::<(String, Bytes, Bytes)>(data)) {
    // Emit a test message in the room1 and room3 rooms, except for the room2 room with the binary payload received
    socket.to("room1").to("room3").except("room2").emit("test", &data);

    // Emit a test message with multiple arguments to the client
    socket.to("room1").emit("test", &("world", "hello", 1)).ok();

    // Emit a test message with an array as the first argument
    let arr = [1, 2, 3, 4];
    socket.to("room2").emit("test", &[arr]).ok();
}

let (_, io) = SocketIo::new_svc();
io.ns("/", |s: SocketRef| s.on("test", handler));
```
