# socketioxide-mongodb 0.1.5
* fix: `close` was a no-op: a deleted namespace kept its change stream and its heartbeats running [#803](https://github.com/Totodore/socketioxide/pull/803).
* fix: leak of a response handler when `broadcast_with_ack` failed or was cancelled [#802](https://github.com/Totodore/socketioxide/pull/802).
* fix: apply the ack timeout of the caller on the remote nodes [#822](https://github.com/Totodore/socketioxide/pull/822).
* deps: bump `socketioxide-core` to 0.20
* deps: `mongodb` depends on `rustls-webpki` and `webpki-roots` versions affected by RUSTSEC-2026-0382 and RUSTSEC-2026-0381.
Run `cargo update -p rustls-webpki -p webpki-roots` to get the fixed versions [#800](https://github.com/Totodore/socketioxide/pull/800).

# socketioxide-mongodb 0.1.4
* deps: bump `socketioxide-core` to 0.19

# socketioxide-mongodb 0.1.3
* fix: rustdoc failed build

# socketioxide-mongodb 0.1.2
* fix: race condition between ack timeout and adapter request timeout when broadcasting with acks.

# socketioxide-mongodb 0.1.1
* fix: compilation error without default features.
* chore(deps): bump bson to 3.0.0

# socketioxide-mongodb 0.1.0
* Initial release!
