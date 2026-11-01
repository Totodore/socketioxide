# socketioxide-redis 0.4.4
* fix: the fred driver stopped the delivery of cluster messages for every namespace when a handler channel was full or closed [#799](https://github.com/Totodore/socketioxide/pull/799).
* fix: leak of a response handler when `broadcast_with_ack` failed or was cancelled [#802](https://github.com/Totodore/socketioxide/pull/802).
* fix: apply the ack timeout of the caller on the remote nodes [#822](https://github.com/Totodore/socketioxide/pull/822).
* deps: bump `socketioxide-core` to 0.20

# socketioxide-redis 0.4.3
* fix: remote socket emit times out even when the remote node answered [#782](https://github.com/Totodore/socketioxide/pull/782)

# socketioxide-redis 0.4.2
* deps: bump `socketioxide-core` to 0.19

# socketioxide-redis 0.4.1
* fix: race condition between ack timeout and adapter request timeout when broadcasting with acks.

# socketioxide-redis 0.4.0
* deps: bump `redis` to 1.0!

# socketioxide-redis 0.3.0
* deps: bump `redis` to 0.32

# socketioxide-redis 0.2.2
* deps: bump `redis` to 0.30.
* deps: bump `socketioxide-core` to 0.17
* MSRV: rust-version is now 1.86 with edition 2024

# socketioxide-redis 0.2.1
* doc: add an incompatibility warning with the `@socket.io/redis-adapter` package.

# socketioxide-redis 0.2
* deps: bump `redis` to 0.28.2
* feat(*breaking*): the redis cluster adapter constructor now takes a `&redis::cluster::ClusterClient`
to match all other adapter constructors.

# socketioxide-redis 0.1.0
* Initial release!
