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
