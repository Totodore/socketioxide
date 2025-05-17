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

# socketioxide 0.16.1
* feat: add `Config::ws_read_buffer_size` to set the read buffer size for each websocket.
* deps: bump `engineioxide` to 0.16.1.

# socketioxide 0.16.0
* feat(*breaking*): remote adapters, see [this article](https://github.com/Totodore/socketioxide/discussions/440) for more details.
* deps: bump `thiserror` to 2.0
* deps: bump `axum` to 0.8
* deps: bump `engineioxide` to 0.16.0
* docs: fix grammar/typos
