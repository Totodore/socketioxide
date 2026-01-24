# socketioxide 0.18.1
* fix: issue [#627](https://github.com/Totodore/socketioxide/issues/627). In case of namespace connect failure, 
the socket was not properly removed from the adapter and the namespace.

# socketioxide 0.18.0
* feat(**breaking**): remove sync handlers to gain 50% of compilation speed.
* feat: Socket rooms will be removed only **after** the disconnect handler execution,
making them available in the disconnect handler (thanks @Mettwasser).

# socketioxide 0.17.2
* fix: issue [#527](https://github.com/Totodore/socketioxide/issues/527). Socketioxide failed to build with the `tracing` flag enable and with `tracing-attributes` set to `v0.1.28`.

# socketioxide 0.17.1
* feat: add `SocketIo::nsps` to get a list of operators on all the current namespaces.
* feat: add `BroadcastOperators::ns_path` to get the namespace path of the current namespace.
* feat: implement `Debug + Clone` for `BroadcastOperators` to reuse it easily.

# socketioxide 0.17.0
* deps: bump `socketioxide-core` to 0.17
* feat: add [`SocketIo::on_fallback`](https://docs.rs/socketioxide/latest/socketioxide/struct.SocketIo.html#method.on_fallback)
and [`Event`](https://docs.rs/socketioxide/latest/socketioxide/extract/struct.Event.html) extractor to add a fallback event handler and
dynamically extract the incoming event.
* MSRV: rust-version is now 1.86 with edition 2024

## socketioxide 0.16.3
* fix: issue [#527](https://github.com/Totodore/socketioxide/issues/527). Socketioxide failed to build with the `tracing` flag enable and with `tracing-attributes` set to `v0.1.28`.

# socketioxide 0.16.1
* feat: add `Config::ws_read_buffer_size` to set the read buffer size for each websocket.
* deps: bump `engineioxide` to 0.16.1.

# socketioxide 0.16.0
* feat(*breaking*): remote adapters, see [this article](https://github.com/Totodore/socketioxide/discussions/440) for more details.
* deps: bump `thiserror` to 2.0
* deps: bump `axum` to 0.8
* deps: bump `engineioxide` to 0.16.0
* docs: fix grammar/typos
