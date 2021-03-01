# Homie device InfluxDB logger

[![crates.io page](https://img.shields.io/crates/v/homie-influx.svg)](https://crates.io/crates/homie-influx)
[![Download](https://api.bintray.com/packages/homie-rs/homie-rs/homie-influx/images/download.svg) ](https://bintray.com/homie-rs/homie-rs/homie-influx/_latestVersion)

`homie-influx` is a service to connect to an MQTT broker, discover devices following the
[Homie convention](https://homieiot.github.io/), and record their property value changes to an
InfluxDB database.

See [the main project readme](https://github.com/alsuren/mijia-homie#readme) for more details and
background.

## Installation

It is recommended to install the latest release from our Debian repository:

```sh
$ curl -L https://bintray.com/user/downloadSubjectPublicKey?username=homie-rs | sudo apt-key add -
$ echo "deb https://dl.bintray.com/homie-rs/homie-rs stable main" | sudo tee /etc/apt/sources.list.d/homie-rs.list
$ sudo apt update && sudo apt install homie-influx
```

Alternatively, you may install with cargo install, but that will require some more setup:

```sh
$ cargo install homie-influx
```

## Usage

If you have installed the Debian package, the service will be set up with systemd for you already.
Otherwise, copy the `homie-influx` binary to `/usr/bin`, copy `debian-scripts/homie-influx.service`
to `/lib/systemd/system`, create a `homie-influx` user to run as, and create `/etc/homie-influx` for
configuration files.

There should be two config files under `/etc/homie-influx`:

- `homie-influx.toml` contains the main configuration for the service, such as which MQTT broker and
  InfluxDB server to connect to. See [homie-influx.example.toml](homie-influx.example.toml) for an
  example of the settings that are supported.
- `mappings.toml` contains a map of Homie base topics to InfluxDB databases. By default it will look
  for devices under the standard `homie` base topic and write to an InfluxDB database called `test`.
  You can add multiple base topics to handle multiple users.

After editing these config files you will need to restart the service:

```sh
$ sudo systemctl restart homie-influx.service
```

You may find it helpful to watch the logs to see whether it is managing to connect:

```sh
$ sudo journalctl -u homie-influx.service --output=cat --follow
```


## Format

This service publishes up to six measurements: `integer`, `float`, `boolean`, `string`, `enum`, `color`,
corresponding to the Homie datatypes. Each message is published as an InfluxDB point with the appropriate
name and timestamp, the value as a `value` field, and device / node / property info included as tags.

In order to support Grafana clients, boolean points also have an additional `value_int` field,
which is an integer, 1 for true or 0 for false.


## License

Licensed under either of

- [Apache License, Version 2.0](http://www.apache.org/licenses/LICENSE-2.0)
- [MIT license](http://opensource.org/licenses/MIT)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the
work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.
