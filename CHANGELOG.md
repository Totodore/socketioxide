# 0.6.0
## socketioxide
* New API for creating the socket.io layer/service. A cheaply clonable `SocketIo` struct is now returned with the layer/service and allows to access namespaces/rooms/sockets everywhere in the application. Moreover, it is now possible to add and remove namespaces dynamically through the `SocketIo` struct.
## engineioxide
* The socket parameter for the handler is now an `Arc<Socket>`.

# 0.5.1
## socketioxide
* Fix a data race bug causing a protocol error when upgrading. A Noop engine.io packet was sent trough the websocket connection after an upgrade. Now all noop packets passing trough the websocket transport are filtered out.
* Fix a bug with binary packets with namespaces : namespace was put before the payload count whereas it should be put after according to the payload datagram.

# 0.5.0
## socketioxide
* A [`on_disconnect`](https://docs.rs/socketioxide/latest/socketioxide/struct.Socket.html#method.on_disconnect) function is now available on the `Socket` instance. The provided callback will be called when the socket disconnects with the reasons for the disconnection. This is useful for logging or cleanup data.
* A [`connect_timeout`](https://docs.rs/socketioxide/latest/socketioxide/struct.SocketIoConfigBuilder.html#method.connect_timeout) option is now available in the config options. It is the maximum time to wait for a socket.io handshake before closing the connection. The default is 45 seconds.
* A [`NsHandlers`](https://docs.rs/socketioxide/latest/socketioxide/struct.NsHandlers.html) struct was added. It describes namespace handlers passed to the SocketIoLayer/SocketIoService. Before, it was a type alias for a HashMap and it was containing types that were not supposed to be public.

## engineioxide
* A [`DisconnectReason`](https://docs.rs/engineioxide/latest/engineioxide/enum.DisconnectReason.html) enum is passed to the `on_disconnect` callback of the engine handler.
* Bump `tokio-tungstenite` to 0.20.1.
