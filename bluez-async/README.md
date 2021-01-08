# BlueZ async client

[![crates.io page](https://img.shields.io/crates/v/bluez-async.svg)](https://crates.io/crates/bluez-async)
[![docs.rs page](https://docs.rs/bluez-async/badge.svg)](https://docs.rs/bluez-async)

`bluez-async` is an async wrapper around the D-Bus interface of BlueZ, the Linux Bluetooth daemon.
It provides type-safe interfaces to a subset of the Bluetooth client (i.e. central, in Bluetooth
terminology) interfaces exposed by BlueZ, focussing on the Generic Attribute Profile (GATT) of
Bluetooth Low Energy (BLE).

See the [examples](examples/) directory for examples of how to use it.

This crate is incomplete, experimental, and may not be supported.

## License

Licensed under either of

- [Apache License, Version 2.0](http://www.apache.org/licenses/LICENSE-2.0)
- [MIT license](http://opensource.org/licenses/MIT)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
