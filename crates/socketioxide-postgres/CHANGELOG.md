# socketioxide-postgres 0.2.0
* fix(*breaking*): `Driver::close` now takes the channels to stop listening to. With the tokio-postgres driver,
deleting a namespace stopped the notifications of every namespace of the node (`UNLISTEN *`) [#806](https://github.com/Totodore/socketioxide/pull/806).
* fix: leak of a response handler when `broadcast_with_ack` failed or was cancelled [#802](https://github.com/Totodore/socketioxide/pull/802).
* fix: apply the ack timeout of the caller on the remote nodes [#822](https://github.com/Totodore/socketioxide/pull/822).
* deps: bump `socketioxide-core` to 0.20
* deps: `sqlx` depends on a `ctutils` version affected by RUSTSEC-2026-0385.
Run `cargo update -p ctutils` to get the fixed version [#814](https://github.com/Totodore/socketioxide/pull/814).

# socketioxide-postgres 0.1.2
* fix: leak of response handler entries for rooms and fetch_sockets requests

# socketioxide-postgres 0.1.1
* deps: bump `socketioxide-core` to 0.19

# socketioxide-postgres 0.1.0
* Initial release!
