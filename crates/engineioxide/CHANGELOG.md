# engineioxide 0.17.4
* MSRV: rust-version is now 1.94
* fix: issue [#731](https://github.com/Totodore/socketioxide/issues/731). Memory leak with unclaimed engine.io sessions whose WebSocket upgrade is started but never completed.
* fix: missing packet separator when polling a parked v4 encoder.
* fix: incorrectly merged binary/string packets when polling a parked v3 encoder.
* fix: v3 encoded payload size is never re-computed for max payload size threshold.
* fix: v3 decoder infinite loop with malformed packet
* fix: base64 binary packet protocol mismatch with payload starting with `4`.
* fix: fix potential panic in debug with malicious v3 packets.
* fix: expect b64 query param to be equal to 1 or true to enable b64 encoding.

# engineioxide 0.17.3
* fix: issue [#719](https://github.com/Totodore/socketioxide/issues/719). Because of an unpinned dependency on `tokio-util`
building engineioxide would fail with `tokio-util < 0.7.14`.

# engineioxide 0.17.2
* fix: `Noop` packet was emitted only one time during the upgrade mechanism, leading to an infinite upgrade loop when the client sent multiple http polling requests.
* fix: the socket closing process was incorrect in some edge cases, leading to leaky sockets. This fix ensures that sockets are properly closed and resources are released.
* feat: improve traces in the upgrade and closing mechanisms.

# engineioxide 0.17.1
* fix: upgrade process was timing out when the client made accidentally more
than one polling requests while upgrading (#497).

# engineioxide 0.17
* MSRV: rust-version is now 1.86 with edition 2024

# engineioxide 0.16.2
* fix: pause heartbeat when the socket is being upgraded to avoid the client
from resending polling requests to respond to ping packets.
* deps: use engineioxide-core 0.1 as a dependency

# engineioxide 0.16.1
* feat: add `Config::ws_read_buffer_size` to set the read buffer size for each websocket.
It will default to 4KiB (previously 128KiB). You can increase it if you have high message throughput and less sockets.

# engineioxide 0.16.0
* deps: bump `thiserror` to 2.0
* deps: bump `tokio-tungstenite` to 0.26
* docs: fix grammar/typos
* fix(engineio): heartbeat start delay
