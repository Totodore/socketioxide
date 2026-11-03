# socketioxide-parser-msgpack 0.17.4
* fix(security): a packet declaring more bytes than it contains, or ending with an `id` key without value, panicked
the connection task, without any authentication [#796](https://github.com/Totodore/socketioxide/pull/796) [#797](https://github.com/Totodore/socketioxide/pull/797). See [Github Advisory](https://github.com/Totodore/socketioxide/security/advisories/GHSA-7xpx-rcqc-8qjh).
* fix: check the event name length in `read_event` [#798](https://github.com/Totodore/socketioxide/pull/798).
* fix: the type byte of Ext8/16/32 values was not skipped, which corrupted the decoding of the next keys [#810](https://github.com/Totodore/socketioxide/pull/810).
* deps: bump `socketioxide-core` to 0.20

# socketioxide-parser-msgpack 0.17.3
* deps: bump `socketioxide-core` to 0.19

# socketioxide-parser-msgpack 0.17.2
* fix(security): bound the packet decoder recursion depth to prevent an unauthenticated remote DoS via
deeply nested MsgPack packets (uncontrolled recursion → stack overflow, CWE-674). See [Github Advisory](https://github.com/Totodore/socketioxide/security/advisories/GHSA-c6g7-r2mf-pf5g)

# socketioxide-parser-msgpack 0.17.1
* deps: bump `socketioxide-core` to 0.18

# socketioxide-parser-msgpack 0.17
* deps: bump `socketioxide-core` to 0.17
* MSRV: rust-version is now 1.86 with edition 2024

# socketioxide-parser-msgpack 0.16.0
* feat(*breaking*): remote adapters
