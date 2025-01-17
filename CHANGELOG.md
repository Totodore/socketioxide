_From now all crate versions are disjoined._

# socketioxide 0.16.0
* feat(*breaking*): remote adapters, see [this article](https://github.com/Totodore/socketioxide/discussions/440) for more details.
* deps: bump `thiserror` to 2.0
* deps: bump `axum` to 0.8
* deps: bump `engineioxide` to 0.16.0
* docs: fix grammar/typos

# socketioxide-parser-msgpack 0.16.0
* feat(*breaking*): remote adapters

# socketioxide-parser-common 0.16.0
* feat(*breaking*): remote adapters

# socketioxide-core 0.16.0
* feat(*breaking*): remote adapters

# socketioxide-redis 0.1.0
* Initial release!

# engineioxide 0.16.0
* deps: bump `thiserror` to 2.0
* deps: bump `tokio-tungstenite` to 0.26
* docs: fix grammar/typos
* fix(engineio): heartbeat start delay

# 0.15.1
## socketioxide
* deps: remove smallvec deps
* doc: fix some links

## engineioxide
* fix: issue [#390](https://github.com/Totodore/socketioxide/issues/390). First ping was sent twice because of tokio interval behavior defaulting to bursting when interval tick is missed.

# 0.15.0
## socketioxide
* **(Breaking)**: New parsing system. You can now serialize and deserialize binary data inside your own types.
It also improve performances by avoiding unnecessary allocations.
* fix: missing extractor error logs for async message handlers.
* feat: add custom compiler error for unimplemented handler traits.
* deps: switch from `tower` to `tower-service` and `tower-layer` subcrates.
* deps: bump `tokio` to 1.40.
* deps: bump `http` to 1.1.
* deps: bump `hyper` to 1.5.

# 0.14.0

## socketioxide
* **(Breaking)**: State reworked to avoid having unsafe global mutable state (issue [#317](https://github.com/Totodore/socketioxide/issues/317)). Therefore State types must now implement `Clone` and will be cloned for each handler where the state is used.
* **(Breaking)**: Extensions reworked to avoid `Send` + `Sync` requirements on extensions (issue [#295](https://github.com/Totodore/socketioxide/issues/295)). They are now extracted by `Cloning`. Therefore all the type must implement `Clone`. An `Extension` extractor is available to get an extension directly without calling `socket.extensions.get()`.
* feat: New `HttpExtension` types to extract extensions from the http request.
* feat: `SocketIo` can now be used as an extractor. So you can now easily access the entire socket.io context from your handlers.
* feat: Dynamic namespaces. You can know set dynamic namespaces with the [`dyn_ns`](https://docs.rs/socketioxide/latest/socketioxide/struct.SocketIo.html#method.dyn_ns) function. You can specify patterns with the `{name}` syntax thanks to the [matchit](https://github.com/ibraheemdev/matchit) crate. The dynamic namespace will create a child namespace for any path that matches the given pattern with the given handler.

## engineioxide
* deps: bump `tokio-tungstenite` from `0.21.0` to `0.23.0`.

# 0.13.1

## engineioxide
* fix: issue [#320](https://github.com/Totodore/socketioxide/issues/320). Remove unnecessary panic when receiving unexpected websocket messages. This might happen with some specific socket.io clients.

# 0.13.0

## socketioxide
* fix: issue [#311](https://github.com/Totodore/socketioxide/issues/311), the `delete_ns` fn was deadlocking the entire server when called from inside a `disconnect_handler`.
* feat: the `delete_ns` is now gracefully closing the adapter as well as all its sockets before being removed.
* feat: the API use `Bytes` rather than `Vec<u8>` to represent binary payloads. This allow to avoid unnecessary copies.
* deps: use `futures-util` and `futures-core` rather than the whole `futures` crate.

## engineioxide
* feat: the API use `Bytes/Str` rather than `Vec<u8>` and `String` to represent payloads. This allow to avoid unnecessary copies.
* deps: use `futures-util` and `futures-core` rather than the whole `futures` crate.

# 0.12.0
**MSRV**: Minimum supported Rust version is now 1.75.

## socketioxide
* **(Breaking)**: Introduction of [connect middlewares](https://docs.rs/socketioxide/latest/socketioxide/#middlewares). It allows to execute code before the connection to the namespace is established. It is useful to check the request, to authenticate the user, to log the connection etc. It is possible to add multiple middlewares and to chain them.
* The `SocketRef` extractor is now `Clone`. Be careful to drop clones when the socket is disconnected to avoid any memory leak.

# 0.11.1
## socketioxide
* fix(#232): under heavy traffic, the adjacent binary packet to the head packet requirement for engine.io was not respected. It was leading to a protocol error.

# 0.11.0
## socketioxide
* fix: a panic was raised sometimes under heavy traffic with socketio v5 when the connect timeout handler is destroyed but that the chan sender is still alive.
* **(Breaking)**: Emit errors now contains the provided data if there is an issue with the internal channel (for example if it is full) or if the socket closed.
* **(Breaking)**: Operators are now splitted between `Operators` and `BroadcastOperators` in order to split logic and fn signatures between broadcast and non-broadcast operators.

## engineioxide
* fix #277: with engine.io v3, the message byte prefix `0x4` was not added to the binary payload with `ws` transport.
* bump dependency `base64` to 0.22.0.

# 0.10.2
## socketioxide
* New [`rooms`](https://docs.rs/socketioxide/latest/socketioxide/struct.SocketIo.html#method.rooms) fn to get all the rooms of a namespace.

# 0.10.1
## socketioxide
* New [`as_str`](https://docs.rs/socketioxide/latest/socketioxide/socket/struct.Sid.html#method.as_str) fn for `Sid`.
* Http request is now cloned for the websocket transport (it was not possible before http v1). Therefore it is possible to get headers/extensions of the initial request.

# 0.10.0
## socketioxide
* Rework for `emit_with_ack` fns. It now returns an `AckStream` that can be used either as a future when expecting one ack or as a stream when expecting multiple acks. When expecting multiple acks the `AckStream` will yield `AckResult`s as well as their corresponding socket `id`.

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
