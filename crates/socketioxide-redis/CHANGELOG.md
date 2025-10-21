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
