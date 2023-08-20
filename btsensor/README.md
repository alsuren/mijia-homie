# BLE sensor advertisement library

[![crates.io page](https://img.shields.io/crates/v/btsensor.svg)](https://crates.io/crates/btsensor)
[![docs.rs page](https://docs.rs/btsensor/badge.svg)](https://docs.rs/btsensor)

A library for decoding sensor readings from BLE advertisements.

Currently supports:

- [BTHome](https://bthome.io/) (v1 and v2, unencrypted)
- [atc1441 format](https://github.com/atc1441/ATC_MiThermometer#advertising-format-of-the-custom-firmware)
- [pvvx custom format](https://github.com/pvvx/ATC_MiThermometer#custom-format-all-data-little-endian).

The actual BLE scanning is up to you, so this library doesn't depend on any
particular Bluetooth library or platform. It just provides types and functions
to decode the data you give it.

## Usage

```rust
use std::collections::HashMap;
use btsensor::{bthome, Reading};

// In a real program, this service data would be obtained from a BLE scan.
let service_data: HashMap<Uuid, Vec<u8>> = [(
    bthome::v1::UNENCRYPTED_UUID,
    vec![0x23, 0x02, 0xC4, 0x09, 0x03, 0x03, 0xBF, 0x13],
)]
.into_iter()
.collect();

let decoded = Reading::decode(&service_data).unwrap();
println!("{}", decoded);
```

For a more complete example, see the [examples](examples/) directory.

## License

Licensed under either of

- [Apache License, Version 2.0](http://www.apache.org/licenses/LICENSE-2.0)
- [MIT license](http://opensource.org/licenses/MIT)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the
work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.
