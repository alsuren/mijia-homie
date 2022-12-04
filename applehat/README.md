# Applehat

[![crates.io page](https://img.shields.io/crates/v/applehat.svg)](https://crates.io/crates/applehat)

`applehat` is a service to use a Rainbow HAT on a Raspberry Pi to show data from sensors on an MQTT
broker following the [Homie convention](https://homieiot.github.io/).

## Installation

It is recommended to install the latest released Debian package from our releases.

Alternatively, you may install with cargo install, but that will require some more setup:

```sh
$ cargo install applehat
```

## Usage

There should be a config file under `/etc/applehat`:

- `applehat.toml` contains the main configuration for the service, such as which MQTT broker to
  connect to. See [applehat.example.toml](applehat.example.toml) for an example of the settings that
  are supported.

After editing these config files you will need to restart the service:

```sh
$ sudo systemctl restart applehat.service
```

You may find it helpful to watch the logs to see whether it is managing to connect:

```sh
$ sudo journalctl -u applehat.service --output=cat --follow
```

## License

Licensed under either of

- [Apache License, Version 2.0](http://www.apache.org/licenses/LICENSE-2.0)
- [MIT license](http://opensource.org/licenses/MIT)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the
work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.
