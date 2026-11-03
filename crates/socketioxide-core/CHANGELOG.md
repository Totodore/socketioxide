# socketioxide-core 0.20.0
* feat(*breaking*): add an `ack_timeout` field to `RequestOut` and `RequestIn` and a parameter to
`RemoteRequestHandler::recv_broadcast_with_ack`, to apply the ack timeout of the caller on the remote nodes [#822](https://github.com/Totodore/socketioxide/pull/822).
During a rolling upgrade, a node on a previous version rejects msgpack requests with a custom timeout.
* fix: a socket in several of the targeted rooms was selected several times when broadcasting [#813](https://github.com/Totodore/socketioxide/pull/813).

# socketioxide-core 0.19.1
* feat: add a new request module to share logic request handling logic between all adapters. 

# socketioxide-core 0.19.0
* feat(*breaking*): Add a new `ParseConfig` arg to parser `decode_{str/bin}` methods. 

# socketioxide-core 0.18.2
* feat: add new `Volatile` broadcast flag 

# socketioxide-core 0.18.1
* feat: add a `is_binary` method to `Packet` to check if the packet is binary.
* MSRV: rust-version is now 1.94

# socketioxide-core 0.18.0
* feat(*breaking*): expose global configured ack timeout to adapter implementations

# socketioxide-core 0.17.0
* feat(*breaking*): remote-adapter packets are now refactored in the core crate. Any adapter implementation can use
it through the `remote-adapter` flag.
* MSRV: rust-version is now 1.86 with edition 2024

# socketioxide-core 0.16.1
* deps: use engineioxide-core 0.1 rather than engineioxide

# socketioxide-core 0.16.0
* feat(*breaking*): remote adapters
