# Generated bindings for BlueZ

[![crates.io page](https://img.shields.io/crates/v/bluez-generated.svg)](https://crates.io/crates/bluez-generated)
[![docs.rs page](https://docs.rs/bluez-generated/badge.svg)](https://docs.rs/bluez-generated)

Generated async D-Bus bindings for talking to BlueZ on Linux.

Bindings are generated from introspection data, using
[`dbus-codegen`](https://crates.io/crates/dbus-codegen). This means that it is relatively easy to
maintain, but it only covers interfaces that I have the devices for.

## Adding Interfaces

If there is an interface that you need which is not generated, it should be reasonably
straightforward to generate them and send a pull request. See
[introspect.sh](https://github.com/alsuren/mijia-homie/blob/master/bluez-generated/introspect.sh)
for details. It's also perfectly reasonable to generate the interfaces you need and vendor them into
your project.

## Future Direction

Only async bindings are generated. Blocking bindings could also be generated, but I'm unlikely
to use them, so they would need to be contributed by someone else.

It would be nice to generate some strongly typed bindings around `add_match()` for subscribing to
`PropertiesChanged` signals as as stream for a particular property.

## License

Licensed under either of

- [Apache License, Version 2.0](http://www.apache.org/licenses/LICENSE-2.0)
- [MIT license](http://opensource.org/licenses/MIT)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the
work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.
