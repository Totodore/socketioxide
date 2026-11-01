# socketioxide 0.18.9
* fix(security): bump `socketioxide-parser-msgpack` to 0.17.4, fixing an unauthenticated remote panic in the opt-in
MsgPack parser [#796](https://github.com/Totodore/socketioxide/pull/796) [#797](https://github.com/Totodore/socketioxide/pull/797). See [Github Advisory](https://github.com/Totodore/socketioxide/security/advisories/GHSA-7xpx-rcqc-8qjh).
* fix(security): bump `engineioxide` to 0.17.9, fixing a remote panic when parsing multibyte or truncated packets [#794](https://github.com/Totodore/socketioxide/pull/794).
* fix: a duplicate CONNECT packet ran the middlewares and the connect handler again and replaced the
first socket without calling its disconnect handler [#801](https://github.com/Totodore/socketioxide/pull/801).
* fix: emitting to several rooms sent the packet several times to a socket in several of them, through `socketioxide-core` 0.20 [#813](https://github.com/Totodore/socketioxide/pull/813).
* fix: bump `socketioxide-parser-common` to 0.17.3: a string packet received during a binary packet now closes the connection [#811](https://github.com/Totodore/socketioxide/pull/811).
* fix: remove the acks abandoned by a timeout from the pending ack map of a socket [#820](https://github.com/Totodore/socketioxide/pull/820).
* deps: bump `socketioxide-core` to 0.20

# socketioxide 0.18.8
* feat: `SocketIoService` now implements the `tower`/`hyper` `Service` traits for `(stream, request)` pairs,
to create a websocket connection from an already upgraded IO stream [#786](https://github.com/Totodore/socketioxide/pull/786).
* fix: rooms joined from a connect middleware were left in the adapter when the middleware rejected
the connection. The socket never enters the namespace on that path, so nothing ever cleaned them up.

# socketioxide 0.18.7
* fix(security): bump `socketioxide-parser-commong` to 0.17.2, fixing unlimited binary data attachments which could lead
to denial of service. See [Github Advisory](https://github.com/Totodore/socketioxide/security/advisories/GHSA-55mf-67qm-4wpg).

# socketioxide 0.18.6
* fix(security): bump `socketioxide-parser-msgpack` to 0.17.2, fixing an unauthenticated remote DoS in the
opt-in MsgPack parser (deeply nested packets caused a stack overflow, CWE-674). See [Github Advisory](https://github.com/Totodore/socketioxide/security/advisories/GHSA-c6g7-r2mf-pf5g)

# socketioxide 0.18.5
* feat: add `volatile()` flags to emit without guaranteeing delivery, this allows to avoid filling internal
buffers and improve performance [#602](https://github.com/Totodore/socketioxide/issues/602).
* chore(deps): bump cmov from 0.5.3 to 0.5.4

# socketioxide 0.18.4
* docs: document accessing state from custom extractors
* MSRV: rust-version is now 1.94

# socketioxide 0.18.3
* fix: race condition when emitting with acknowledgement.
* feat: expose global configured ack timeout to adapter implementations

# socketioxide 0.18.2
* deps: bump rand from 0.9.1 to 0.10.0

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
