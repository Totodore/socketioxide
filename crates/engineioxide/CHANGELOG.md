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
