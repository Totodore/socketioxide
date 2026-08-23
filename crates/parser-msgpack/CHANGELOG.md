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
