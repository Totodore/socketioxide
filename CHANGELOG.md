# 0.9.1
## socketioxide
* Add `SocketIo::get_socket` and `Operators::get_socket` methods to get a socket ref from its id.
* Switch to `pin-project-lite` instead of `pin-project`.

# 0.9.0
* Bump `hyper` to 1.0.1. Therefore it is now possible to use frameworks based on hyper v1.*. Check the [compatibility table](./README.md#compatibility) for more details.

# 0.8.0
## socketioxide
* Add `transport_type` and `protocol` fn on the `Socket` struct. It allows to know the transport type and the protocol used by the socket.
* Dynamic `DisconnectHandler`. Now the `on_disconnect` handler take a dynamic handler that maybe async and contain any type that implements `FromDisconnectParts`. It allows to extract data from the disconnection, like the socket, the reason of the disconnection, the state etc.
* New `state` feature flag. It enables global state management. It is useful to share data between handlers. It is disabled by default.

## engineioxide
* Packet encoding/decoding optimizations.

# 0.7.3
## socketioxide
* Fix [#189](https://github.com/Totodore/socketioxide/issues/189). Async message handlers were never called because the returned future was not spawned with `tokio::spawn`.

# 0.7.2
## socketioxide
* The `on_disconnect` callback now takes a `SocketRef` rather than an `Arc<Socket>` to match other handlers. It also avoids that the user clone the socket and create a memory leak.
* Documentation improvements (see https://docs.rs/socketioxide).
* Bump library dependencies (see [release notes](https://github.com/totodore/socketioxide/releases/tag/v0.7.2)).

# 0.7.1
## socketioxide
* Fix [#154](https://github.com/Totodore/socketioxide/issues/154), build was broken when using the `hyper-v1` feature flag because of `hyper-util` dependency which is not published on crates.io.

# 0.7.0
## socketioxide
* The `extensions` field on sockets has been moved to a separate optional feature flag named `extensions`
* All the `tracing` internal calls have been moved to a separate optional feature flag named `tracing`
* A compatibility layer is now available for hyper v1 under the feature flag `hyper-v1`. You can call `with_hyper_v1` on the `SocketIoLayer` or the `SocketIoService` to get a layer/service working with hyper v1. Therefore, it is now possible to use [`salvo`](http://salvo.rs) as an http server. The default is still hyper v0.
* Socket.io packet encoding/decoding has been optimized, it is now between [~15% and ~50%](https://github.com/Totodore/socketioxide/pull/124#issuecomment-1784298574) faster than before.
* The v5 feature flag is removed, it is now enabled by default. It is made to avoid destructive feature flags, and to be sure that `--no-default-features` will always work without enabling anything else.
* All the handlers now have dynamic parameters. It is now possible to use any type that implements `FromMessageParts` or `FromMessage` as a parameter for a message handler and `FromConnectPart` for a connect handler. This is useful to extract data from the event, like the socket, the data, an acknowledgment, etc.
* All the handlers are now optionally async.
* The request data to initialize the socket.io connection is available with `Socket::req_parts()`.
* MSRV is now 1.67.0
* Bump tokio from 1.33.0 to 1.34.0
* Bump serde from 1.0.190 to 1.0.192
* Bump serde_json from 1.0.107 to 1.0.108

## engineioxide
* All the `tracing` internal calls have been moved to a separate optional feature flag named `tracing`
* A compatibility layer is now available for hyper v1 under the feature flag `hyper-v1`. You can call `with_hyper_v1` on the `EngineIoLayer` or the `EngineIoService` to get a layer/service working with hyper v1. The default is still hyper v0.
* Sid generation is now done manually without external crates
* The v4 feature flag is removed, it is now enabled by default. It is made to avoid destructive feature flags, and to be sure that `--no-default-features` will always work without enabling anything else.
* Fix an upgrade synchronization bug when using the websocket transport which was leading the client to wait for a ping packet (30s by default) before upgrading to websocket.
* The `on_connect` handler was called twice when upgrading to websocket. It is now called only once.

# 0.6.0
## socketioxide
* New API for creating the socket.io layer/service. A cheaply clonable `SocketIo` struct is now returned with the layer/service and allows to access namespaces/rooms/sockets everywhere in the application. Moreover, it is now possible to add and remove namespaces dynamically through the `SocketIo` struct.
* The socket.io v4 protocol is now available under the feature flag `v4`, it matches every socket.io js version from 1.0.3 to current . The `v5` protocol is still the default and is more performant, it matches every socket.io js version from v3.0.0 to current.

## engineioxide
* The socket parameter for the handler is now an `Arc<Socket>`.
* The `max_payload` option is now applied when encoding a packet. Before, it was only applied when decoding a packet.
* With `websocket` transport, packets are now bufferred before being flushed. Before, they were flushed one by one.

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
