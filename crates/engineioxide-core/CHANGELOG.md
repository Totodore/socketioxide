# engineioxide-core 0.2.3
* fix(security): `Packet::parse` panicked on a 6 bytes packet starting with a multibyte char, and on the
v3 packet `b`. Any client could panic its connection task after the handshake [#794](https://github.com/Totodore/socketioxide/pull/794).
* fix: `Str::slice` now panics on a range that is out of bounds or not on a char boundary, instead of
building an invalid `Str` [#794](https://github.com/Totodore/socketioxide/pull/794).
* docs: a session id is made of 96 random bits, not 128 [#818](https://github.com/Totodore/socketioxide/pull/818).

# engineioxide-core 0.2.2
* refactor: move polling payload encoder/decoder to `core`.
* chore(deps): bump cmov from 0.5.3 to 0.5.4

# engineioxide-core 0.2.1
* deps: bump rand from 0.9.1 to 0.10.0

# engineioxide-core 0.2.0
* MSRV: rust-version is now 1.86 with edition 2024

# engineioxide-core 0.1.0
* New crate to share engineioxide core types.
